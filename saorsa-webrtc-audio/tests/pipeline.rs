//! Device-independent pipeline tests: synthetic PCM through the same
//! stages the live capture/playout paths use (convert → downmix → resample
//! → chunk → Opus round-trip → ring). CI has no audio hardware; the only
//! real-device test is `#[ignore]`d.

use saorsa_webrtc_audio::chunker::FrameChunker;
use saorsa_webrtc_audio::convert::{downmix_to_mono_i16, f32_to_i16};
use saorsa_webrtc_audio::resampler::MonoResampler;
use saorsa_webrtc_audio::ring::PlayoutRing;
use saorsa_webrtc_audio::FRAME_SAMPLES_48K_MONO_20MS;
use saorsa_webrtc_codecs::{Channels, OpusDecoder, OpusEncoder, OpusEncoderConfig, SampleRate};

fn tone_f32(rate: u32, hz: f32, secs: f32) -> Vec<f32> {
    (0..(rate as f32 * secs) as usize)
        .map(|i| 0.5 * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin())
        .collect()
}

/// The full synthetic capture path: 44.1 kHz stereo f32 device blocks →
/// i16 → mono → 48 kHz → 20 ms frames → Opus encode/decode → playout ring.
#[test]
fn synthetic_capture_to_playout_pipeline() {
    let device_rate = 44_100u32;
    let mono_tone = tone_f32(device_rate, 440.0, 1.0);
    // Interleave to stereo as a device would deliver.
    let stereo: Vec<i16> = mono_tone
        .iter()
        .flat_map(|&s| {
            let v = f32_to_i16(s);
            [v, v]
        })
        .collect();

    let mut resampler = MonoResampler::new(device_rate).expect("resampler");
    let mut chunker = FrameChunker::new();
    let mut frames = Vec::new();

    // Feed in irregular block sizes like real callbacks.
    for block in stereo.chunks(1234) {
        let mono = downmix_to_mono_i16(block, 2);
        resampler.push(&mono).expect("resample");
        chunker.push(&resampler.pop_output());
        while let Some(f) = chunker.pop_frame() {
            frames.push(f);
        }
    }

    // ~1 s of audio → ~50 frames (resampler retains a small tail).
    assert!(
        (45..=50).contains(&frames.len()),
        "expected ≈50 frames, got {}",
        frames.len()
    );
    assert!(frames
        .iter()
        .all(|f| f.data.len() == FRAME_SAMPLES_48K_MONO_20MS));
    // Timestamps are a contiguous 20 ms sample clock.
    for (i, f) in frames.iter().enumerate() {
        assert_eq!(f.timestamp, i as u64 * 20);
    }

    // Opus round-trip each frame, then run the playout ring stage.
    let mut enc = OpusEncoder::new(OpusEncoderConfig::default()).expect("encoder");
    let mut dec = OpusDecoder::new(SampleRate::Hz48000, Channels::Mono).expect("decoder");
    let ring = PlayoutRing::new(48 * 200);
    let mut callback_buf = vec![0i16; 480]; // 10 ms device quantum
    let mut popped = 0usize;
    for f in &frames {
        let packet = enc.encode(f).expect("encode");
        assert!(
            packet.len() < f.data.len() * 2 / 4,
            "opus packet not compressed: {} bytes",
            packet.len()
        );
        let decoded = dec.decode(&packet).expect("decode");
        ring.push(&decoded.data);
        // Consume roughly as produced to keep the ring bounded.
        while ring.len() >= callback_buf.len() {
            ring.pop_into(&mut callback_buf);
            popped += callback_buf.len();
        }
    }
    assert!(popped > 40 * FRAME_SAMPLES_48K_MONO_20MS);
    assert_eq!(ring.underrun_samples(), 0, "consumer never outran producer");
}

/// Native 48 kHz path must bypass resampling losslessly.
#[test]
fn native_48k_path_is_lossless_before_codec() {
    // 4800 = 5 exact frames of 960 — every sample must chunk out verbatim.
    let samples: Vec<i16> = (0..4800).map(|i| (i % 3000) as i16).collect();
    let mut chunker = FrameChunker::new();
    chunker.push(&samples);
    let mut out = Vec::new();
    while let Some(f) = chunker.pop_frame() {
        out.extend_from_slice(&f.data);
    }
    assert_eq!(out, samples);
}

/// Real-device smoke — requires audio hardware; run manually on a Studio.
#[test]
#[ignore = "requires real audio devices"]
fn real_device_capture_produces_frames() {
    let (capture, mut rx) =
        saorsa_webrtc_audio::AudioCapture::start(Default::default()).expect("capture start");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("rt");
    let got = rt.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await
    });
    capture.stop();
    let frame = got
        .expect("timed out waiting for a frame")
        .expect("stream closed");
    assert_eq!(frame.data.len(), FRAME_SAMPLES_48K_MONO_20MS);
}
