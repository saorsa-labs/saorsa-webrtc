//! [`AudioFrame`] playout → speakers, via a bounded ring with silence on
//! underrun.

use crate::convert::i16_to_f32;
use crate::ring::PlayoutRing;
use crate::{AudioError, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use saorsa_webrtc_codecs::AudioFrame;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Playout configuration.
#[derive(Debug, Clone)]
pub struct PlayoutConfig {
    /// Device name from [`crate::devices::output_devices`]; `None` = default.
    pub device: Option<String>,
    /// Ring depth in milliseconds of 48 kHz mono audio (jitter absorption
    /// happens upstream; this is the device-feed cushion).
    pub buffer_ms: u32,
    /// Frame-channel capacity between the async sender and the ring feeder.
    pub channel_capacity: usize,
}

impl Default for PlayoutConfig {
    fn default() -> Self {
        Self {
            device: None,
            buffer_ms: 200,
            channel_capacity: 32,
        }
    }
}

/// Handle for a running playout pipeline. Dropping it (or calling
/// [`AudioPlayout::stop`]) tears the stream down.
#[derive(Debug)]
pub struct AudioPlayout {
    shutdown: Option<std::sync::mpsc::Sender<()>>,
    owner: Option<std::thread::JoinHandle<()>>,
    feeder: Option<std::thread::JoinHandle<()>>,
    ring: Arc<PlayoutRing>,
    device_name: String,
}

impl AudioPlayout {
    /// Start playout. Returns the handle and the frame sink.
    ///
    /// Frames are expected mono 48 kHz (this crate's capture format). The
    /// output device must support 48 kHz; mono is duplicated across output
    /// channels.
    pub fn start(config: PlayoutConfig) -> Result<(Self, mpsc::Sender<AudioFrame>)> {
        let device = crate::devices::find_output(config.device.as_deref())?;
        let device_name = device.name().unwrap_or_else(|_| "<unnamed>".into());

        let stream_config = pick_48k_config(&device)?;
        let out_channels = usize::from(stream_config.0.channels);
        let format = stream_config.1;

        let ring = PlayoutRing::new((48 * config.buffer_ms.max(20)) as usize);
        let (frame_tx, mut frame_rx) = mpsc::channel::<AudioFrame>(config.channel_capacity.max(1));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<()>>();
        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();

        // Feeder: async channel → ring (blocking_recv is runtime-free).
        let feeder_ring = Arc::clone(&ring);
        let feeder = std::thread::Builder::new()
            .name("sw-audio-playout-feeder".into())
            .spawn(move || {
                while let Some(frame) = frame_rx.blocking_recv() {
                    feeder_ring.push(&frame.data);
                }
            })
            .map_err(|e| AudioError::BuildStream(e.to_string()))?;

        // Owner thread holds the !Send stream; callback pops mono samples
        // and fans out across device channels.
        let cb_ring = Arc::clone(&ring);
        let owner = std::thread::Builder::new()
            .name("sw-audio-playout-stream".into())
            .spawn(move || {
                let err_fn = |e| tracing::warn!("playout stream error: {e}");
                let cfg = stream_config.0;
                let stream = match format {
                    cpal::SampleFormat::I16 => device.build_output_stream(
                        &cfg,
                        move |out: &mut [i16], _: &_| {
                            let frames = out.len() / out_channels.max(1);
                            let mut mono = vec![0i16; frames];
                            cb_ring.pop_into(&mut mono);
                            for (i, frame) in out.chunks_exact_mut(out_channels).enumerate() {
                                frame.fill(mono[i]);
                            }
                        },
                        err_fn,
                        None,
                    ),
                    cpal::SampleFormat::F32 => device.build_output_stream(
                        &cfg,
                        move |out: &mut [f32], _: &_| {
                            let frames = out.len() / out_channels.max(1);
                            let mut mono = vec![0i16; frames];
                            cb_ring.pop_into(&mut mono);
                            for (i, frame) in out.chunks_exact_mut(out_channels).enumerate() {
                                frame.fill(i16_to_f32(mono[i]));
                            }
                        },
                        err_fn,
                        None,
                    ),
                    other => {
                        let _ = ready_tx.send(Err(AudioError::UnsupportedFormat(format!(
                            "sample format {other:?}"
                        ))));
                        return;
                    }
                };
                let stream = match stream {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(AudioError::BuildStream(e.to_string())));
                        return;
                    }
                };
                if let Err(e) = stream.play() {
                    let _ = ready_tx.send(Err(AudioError::PlayStream(e.to_string())));
                    return;
                }
                let _ = ready_tx.send(Ok(()));
                let _ = shutdown_rx.recv();
                drop(stream);
            })
            .map_err(|e| AudioError::BuildStream(e.to_string()))?;

        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                drop(frame_tx);
                let _ = owner.join();
                let _ = feeder.join();
                return Err(e);
            }
            Err(_) => return Err(AudioError::ThreadStartup),
        }

        tracing::info!(device = %device_name, channels = out_channels, "audio playout started (48 kHz)");
        Ok((
            Self {
                shutdown: Some(shutdown_tx),
                owner: Some(owner),
                feeder: Some(feeder),
                ring,
                device_name,
            },
            frame_tx,
        ))
    }

    /// Samples replaced by silence because the ring ran dry.
    pub fn underrun_samples(&self) -> u64 {
        self.ring.underrun_samples()
    }

    /// Samples evicted because the producer outran the device.
    pub fn overrun_samples(&self) -> u64 {
        self.ring.overrun_samples()
    }

    /// The device this playout runs on.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Stop the stream and join the pipeline threads. The frame sender must
    /// be dropped by the caller for the feeder to exit; this method only
    /// waits a bounded time for it.
    pub fn stop(mut self) {
        self.teardown();
    }

    fn teardown(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.owner.take() {
            let _ = h.join();
        }
        // The feeder exits when the caller drops the Sender; join without
        // blocking forever if they haven't yet.
        if let Some(h) = self.feeder.take() {
            if h.is_finished() {
                let _ = h.join();
            } else {
                tracing::debug!("playout feeder still waiting on sender drop; detaching");
            }
        }
    }
}

impl Drop for AudioPlayout {
    fn drop(&mut self) {
        self.teardown();
    }
}

/// Choose a 48 kHz output config, preferring the device default when it is
/// already 48 kHz.
fn pick_48k_config(device: &cpal::Device) -> Result<(cpal::StreamConfig, cpal::SampleFormat)> {
    let default = device
        .default_output_config()
        .map_err(|e| AudioError::UnsupportedFormat(e.to_string()))?;
    if default.sample_rate().0 == 48_000 {
        return Ok((default.clone().into(), default.sample_format()));
    }
    let supported = device
        .supported_output_configs()
        .map_err(|e| AudioError::Enumeration(e.to_string()))?;
    for range in supported {
        if range.min_sample_rate().0 <= 48_000 && range.max_sample_rate().0 >= 48_000 {
            let cfg = range.with_sample_rate(cpal::SampleRate(48_000));
            let fmt = cfg.sample_format();
            return Ok((cfg.into(), fmt));
        }
    }
    Err(AudioError::UnsupportedFormat(format!(
        "output device offers no 48 kHz config (default {} Hz)",
        default.sample_rate().0
    )))
}
