//! Manual loopback check: mic → Opus encode → decode → speakers.
//!
//! Run on a real machine (Studio validation): `cargo run -p
//! saorsa-webrtc-audio --example loopback [seconds]`. Use a headset — there
//! is no echo cancellation by design.

use saorsa_webrtc_audio::{AudioCapture, AudioPlayout, CaptureConfig, PlayoutConfig};
use saorsa_webrtc_codecs::{Channels, OpusDecoder, OpusEncoder, OpusEncoderConfig, SampleRate};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    println!("input devices:");
    for d in saorsa_webrtc_audio::devices::input_devices()? {
        println!(
            "  {}{}",
            d.name,
            if d.is_default { " (default)" } else { "" }
        );
    }
    println!("output devices:");
    for d in saorsa_webrtc_audio::devices::output_devices()? {
        println!(
            "  {}{}",
            d.name,
            if d.is_default { " (default)" } else { "" }
        );
    }

    let (capture, mut frames) = AudioCapture::start(CaptureConfig::default())?;
    let (playout, sink) = AudioPlayout::start(PlayoutConfig::default())?;
    println!(
        "loopback {}s: {} -> opus -> {}",
        secs,
        capture.device_name(),
        playout.device_name()
    );

    let mut encoder = OpusEncoder::new(OpusEncoderConfig::default())?;
    let mut decoder = OpusDecoder::new(SampleRate::Hz48000, Channels::Mono)?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    rt.block_on(async {
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(200), frames.recv()).await {
                Ok(Some(frame)) => {
                    let packet = encoder.encode(&frame)?;
                    let decoded = decoder.decode(&packet)?;
                    if sink.send(decoded).await.is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(_) => {} // no frame in 200 ms; keep waiting
            }
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;

    println!(
        "done. capture drops={} playout underrun={} overrun={}",
        capture.dropped_frames(),
        playout.underrun_samples(),
        playout.overrun_samples()
    );
    capture.stop();
    drop(sink);
    playout.stop();
    Ok(())
}
