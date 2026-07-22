//! WP-V0.2 interop proof: our Opus wrapper produces real libopus packets.
//!
//! (a) our encoder → RAW `opus` crate decoder → 440 Hz tone energy/shape;
//! (b) round-trip SNR sanity through our own decoder;
//! (c) compression: a 20 ms frame at 64 kbps is really compressed — the
//!     historical pass-through stub fails this by construction.

#![cfg(feature = "opus")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use saorsa_webrtc_codecs::{
    AudioFrame, Channels, OpusDecoder, OpusEncoder, OpusEncoderConfig, SampleRate,
};

const RATE: u32 = 48_000;
const FRAME: usize = 960; // 20 ms @ 48 kHz mono
const TONE_HZ: f32 = 440.0;

fn tone(samples: usize, phase_offset: usize) -> Vec<i16> {
    (0..samples)
        .map(|i| {
            ((((i + phase_offset) as f32) * TONE_HZ * 2.0 * std::f32::consts::PI / RATE as f32)
                .sin()
                * 16000.0) as i16
        })
        .collect()
}

fn frame_at(idx: usize) -> AudioFrame {
    AudioFrame {
        data: tone(FRAME, idx * FRAME),
        sample_rate: SampleRate::Hz48000,
        channels: Channels::Mono,
        timestamp: (idx * 20) as u64,
    }
}

fn rms(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| (f64::from(*s)).powi(2)).sum();
    (sum / samples.len() as f64).sqrt()
}

fn zero_crossings(samples: &[i16]) -> usize {
    samples
        .windows(2)
        .filter(|w| (w[0] >= 0) != (w[1] >= 0))
        .count()
}

/// (a) Our encoder's packets decode on a raw `opus` crate decoder and the
/// audio survives: energy within 3 dB of the source and zero-crossing rate
/// consistent with a 440 Hz tone (codec warm-up frames skipped).
#[test]
fn our_encoder_interops_with_raw_libopus_decoder() {
    let mut enc = OpusEncoder::new(OpusEncoderConfig::default()).unwrap();
    let mut raw_dec = opus::Decoder::new(RATE, opus::Channels::Mono).unwrap();

    let total_frames = 10;
    let skip = 5; // pre-skip/warm-up
    let mut decoded_tail: Vec<i16> = Vec::new();
    let mut src_tail: Vec<i16> = Vec::new();

    for idx in 0..total_frames {
        let f = frame_at(idx);
        let packet = enc.encode(&f).unwrap();
        let mut out = vec![0i16; FRAME];
        let n = raw_dec.decode(&packet, &mut out, false).unwrap();
        assert_eq!(n, FRAME, "raw decoder must yield a full 20 ms frame");
        if idx >= skip {
            decoded_tail.extend_from_slice(&out[..n]);
            src_tail.extend_from_slice(&f.data);
        }
    }

    let src_rms = rms(&src_tail);
    let dec_rms = rms(&decoded_tail);
    let db = 20.0 * (dec_rms / src_rms).log10();
    assert!(
        db.abs() < 3.0,
        "energy drift {db:.2} dB (src {src_rms:.0}, dec {dec_rms:.0})"
    );

    // 440 Hz over 100 ms ⇒ ~88 crossings; allow ±20%.
    let zc = zero_crossings(&decoded_tail);
    let expected = (2.0 * TONE_HZ * (decoded_tail.len() as f32 / RATE as f32)) as isize;
    let delta = (zc as isize - expected).abs();
    assert!(
        delta <= expected / 5,
        "zero crossings {zc} vs expected ~{expected}"
    );
}

/// (b) Round trip through our own decoder: SNR sanity on the steady-state
/// tail. Opus is lossy AND introduces algorithmic delay (~6.5 ms pre-skip),
/// so the decoded stream is time-shifted relative to the source — SNR is
/// measured at the best cross-correlation lag, which is the honest
/// waveform-fidelity number.
#[test]
fn roundtrip_snr_sanity() {
    let mut enc = OpusEncoder::new(OpusEncoderConfig::default()).unwrap();
    let mut dec = OpusDecoder::new(SampleRate::Hz48000, Channels::Mono).unwrap();

    let total_frames = 10;
    let skip = 5;
    let mut src: Vec<i16> = Vec::new();
    let mut out: Vec<i16> = Vec::new();

    for idx in 0..total_frames {
        let f = frame_at(idx);
        let packet = enc.encode(&f).unwrap();
        let d = dec.decode(&packet).unwrap();
        assert_eq!(d.data.len(), FRAME);
        if idx >= skip {
            src.extend_from_slice(&f.data);
            out.extend_from_slice(&d.data);
        }
    }

    // Find the delay (search up to 25 ms) by maximizing correlation.
    let max_lag = (RATE as usize) / 40; // 1200 samples
    let window = src.len() - max_lag;
    let mut best_lag = 0usize;
    let mut best_corr = f64::MIN;
    for lag in 0..max_lag {
        let corr: f64 = (0..window)
            .map(|i| f64::from(src[i]) * f64::from(out[i + lag]))
            .sum();
        if corr > best_corr {
            best_corr = corr;
            best_lag = lag;
        }
    }

    let mut signal_energy = 0.0f64;
    let mut noise_energy = 0.0f64;
    for i in 0..window {
        let s = f64::from(src[i]);
        let d = f64::from(out[i + best_lag]);
        signal_energy += s * s;
        noise_energy += (s - d) * (s - d);
    }
    assert!(signal_energy > 0.0);
    let snr_db = 10.0 * (signal_energy / noise_energy.max(1e-9)).log10();
    assert!(
        snr_db > 10.0,
        "round-trip SNR {snr_db:.1} dB at lag {best_lag} — expected > 10 dB on a pure tone"
    );
}

/// (c) Real compression: every steady-state 20 ms packet at 64 kbps stays
/// ≤ 200 bytes (raw PCM would be 1,920). The first two packets are exempt
/// (cold-start transients may exceed nominal bitrate). The pass-through
/// stub emits 1,937-byte "packets" and cannot pass regardless.
#[test]
fn packets_are_actually_compressed() {
    let mut enc = OpusEncoder::new(OpusEncoderConfig::default()).unwrap();
    let mut max_steady = 0usize;
    for idx in 0..10 {
        let packet = enc.encode(&frame_at(idx)).unwrap();
        if idx >= 2 {
            max_steady = max_steady.max(packet.len());
        }
    }
    assert!(
        max_steady <= 200,
        "largest steady-state 20 ms packet was {max_steady} bytes; expected ≤ 200 at 64 kbps"
    );
}
