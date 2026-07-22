//! Microphone capture → 20 ms mono 48 kHz [`AudioFrame`]s.

use crate::chunker::FrameChunker;
use crate::convert::{downmix_to_mono_i16, f32_to_i16, u16_to_i16};
use crate::resampler::MonoResampler;
use crate::{AudioError, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use saorsa_webrtc_codecs::AudioFrame;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Capture configuration.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Device name from [`crate::devices::input_devices`]; `None` = default.
    pub device: Option<String>,
    /// Frame-channel capacity; when the consumer lags this far behind,
    /// newest frames are dropped (and counted) to keep latency bounded.
    pub channel_capacity: usize,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            device: None,
            channel_capacity: 32,
        }
    }
}

/// Handle for a running capture pipeline. Dropping it (or calling
/// [`AudioCapture::stop`]) tears the stream down.
#[derive(Debug)]
pub struct AudioCapture {
    shutdown: Option<std::sync::mpsc::Sender<()>>,
    owner: Option<std::thread::JoinHandle<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
    dropped_frames: Arc<AtomicU64>,
    device_name: String,
}

impl AudioCapture {
    /// Start capturing. Returns the handle and the frame stream.
    ///
    /// The receiver yields mono 48 kHz 20 ms frames regardless of the
    /// device's native rate/channel count (downmix + resample inside).
    pub fn start(config: CaptureConfig) -> Result<(Self, mpsc::Receiver<AudioFrame>)> {
        let device = crate::devices::find_input(config.device.as_deref())?;
        let device_name = device.name().unwrap_or_else(|_| "<unnamed>".into());
        let native = device
            .default_input_config()
            .map_err(|e| AudioError::UnsupportedFormat(e.to_string()))?;
        let rate = native.sample_rate().0;
        let channels = usize::from(native.channels());
        let format = native.sample_format();

        let (frame_tx, frame_rx) = mpsc::channel::<AudioFrame>(config.channel_capacity.max(1));
        let (raw_tx, raw_rx) = crossbeam_channel::unbounded::<Vec<i16>>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<()>>();
        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();
        let dropped = Arc::new(AtomicU64::new(0));

        // Worker: raw interleaved blocks → mono → 48 kHz → 20 ms frames.
        let worker_dropped = Arc::clone(&dropped);
        let worker = std::thread::Builder::new()
            .name("sw-audio-capture-worker".into())
            .spawn(move || {
                let mut resampler = if rate == 48_000 {
                    None
                } else {
                    match MonoResampler::new(rate) {
                        Ok(r) => Some(r),
                        Err(e) => {
                            tracing::error!("capture resampler init failed: {e}");
                            return;
                        }
                    }
                };
                let mut chunker = FrameChunker::new();
                while let Ok(block) = raw_rx.recv() {
                    let mono = downmix_to_mono_i16(&block, channels);
                    let at_48k = match resampler.as_mut() {
                        None => mono,
                        Some(r) => {
                            if let Err(e) = r.push(&mono) {
                                tracing::error!("capture resample failed: {e}");
                                return;
                            }
                            r.pop_output()
                        }
                    };
                    chunker.push(&at_48k);
                    while let Some(frame) = chunker.pop_frame() {
                        if frame_tx.try_send(frame).is_err() {
                            worker_dropped.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            })
            .map_err(|e| AudioError::BuildStream(e.to_string()))?;

        // Owner thread: builds and holds the !Send cpal stream.
        let owner = std::thread::Builder::new()
            .name("sw-audio-capture-stream".into())
            .spawn(move || {
                let err_fn = |e| tracing::warn!("capture stream error: {e}");
                let stream = match format {
                    cpal::SampleFormat::I16 => device.build_input_stream(
                        &native.clone().into(),
                        {
                            let tx = raw_tx.clone();
                            move |data: &[i16], _: &_| {
                                let _ = tx.try_send(data.to_vec());
                            }
                        },
                        err_fn,
                        None,
                    ),
                    cpal::SampleFormat::U16 => device.build_input_stream(
                        &native.clone().into(),
                        {
                            let tx = raw_tx.clone();
                            move |data: &[u16], _: &_| {
                                let _ = tx.try_send(data.iter().map(|&s| u16_to_i16(s)).collect());
                            }
                        },
                        err_fn,
                        None,
                    ),
                    cpal::SampleFormat::F32 => device.build_input_stream(
                        &native.clone().into(),
                        {
                            let tx = raw_tx.clone();
                            move |data: &[f32], _: &_| {
                                let _ = tx.try_send(data.iter().map(|&s| f32_to_i16(s)).collect());
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
                // The callback owns clones of raw_tx; drop ours so worker
                // exit tracks stream teardown exactly.
                drop(raw_tx);
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
                // Park until shutdown; sender-drop also unblocks us.
                let _ = shutdown_rx.recv();
                drop(stream);
            })
            .map_err(|e| AudioError::BuildStream(e.to_string()))?;

        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = owner.join();
                let _ = worker.join();
                return Err(e);
            }
            Err(_) => return Err(AudioError::ThreadStartup),
        }

        tracing::info!(
            device = %device_name,
            rate,
            channels,
            "audio capture started (emitting 48 kHz mono 20 ms frames)"
        );
        Ok((
            Self {
                shutdown: Some(shutdown_tx),
                owner: Some(owner),
                worker: Some(worker),
                dropped_frames: dropped,
                device_name,
            },
            frame_rx,
        ))
    }

    /// Frames dropped because the consumer lagged (latency bound).
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames.load(Ordering::Relaxed)
    }

    /// The device this capture runs on.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Stop the stream and join the pipeline threads.
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
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.teardown();
    }
}
