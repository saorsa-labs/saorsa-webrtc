//! V2-opener interop proof: our OpenH264 wrapper produces real H.264.
//!
//! (a) our encoder → RAW `openh264` crate decoder → dimensions + PSNR on a
//!     synthetic moving gradient;
//! (b) compression: real rate control ≪ raw I420 — the historical
//!     pass-through stub (fixed RGB/4 output, 57,616 B at 320×240) fails
//!     every bound here by construction;
//! (c) NAL structure: the first access unit carries SPS(7)/PPS(8)/IDR(5),
//!     steady-state frames are non-IDR(1), and `request_keyframe()`
//!     produces a fresh IDR.

#![cfg(feature = "h264")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use openh264::formats::YUVSource;
use saorsa_webrtc_codecs::{
    OpenH264Decoder, OpenH264Encoder, OpenH264EncoderConfig, VideoDecoder, VideoEncoder, VideoFrame,
};

const W: u32 = 320;
const H: u32 = 240;
const RAW_I420: usize = (W as usize * H as usize * 3) / 2; // 115,200
const STUB_PACKET: usize = (W as usize * H as usize * 3) / 4 + 16; // 57,616

/// Smooth moving gradient — compresses well, changes every frame.
fn gradient_frame(idx: usize) -> VideoFrame {
    let (w, h) = (W as usize, H as usize);
    let mut data = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * 3;
            data[o] = (((x + idx * 4) * 255) / w) as u8;
            data[o + 1] = ((y * 255) / h) as u8;
            data[o + 2] = ((((x + y) / 2) + idx * 2) % 256) as u8;
        }
    }
    VideoFrame {
        data,
        width: W,
        height: H,
        timestamp: (idx as u64) * 33,
    }
}

fn psnr_rgb(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let mse: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = f64::from(*x) - f64::from(*y);
            d * d
        })
        .sum::<f64>()
        / a.len() as f64;
    if mse == 0.0 {
        return f64::INFINITY;
    }
    10.0 * (255.0f64 * 255.0 / mse).log10()
}

/// Annex-B NAL unit types present in a packet.
fn nal_types(packet: &[u8]) -> Vec<u8> {
    let mut types = Vec::new();
    let mut i = 0;
    while i + 3 < packet.len() {
        let start = if packet[i..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if packet[i..].starts_with(&[0, 0, 1]) {
            3
        } else {
            i += 1;
            continue;
        };
        if i + start < packet.len() {
            types.push(packet[i + start] & 0x1F);
        }
        i += start;
    }
    types
}

fn encoder() -> OpenH264Encoder {
    OpenH264Encoder::with_config(OpenH264EncoderConfig {
        width: W,
        height: H,
        bitrate_bps: 512_000,
        max_fps: 30.0,
    })
    .unwrap()
}

/// (a) Our packets decode on a decoder built directly from the raw
/// `openh264` crate API, with correct dimensions and sane fidelity.
#[test]
fn our_encoder_interops_with_raw_openh264_decoder() {
    let mut enc = encoder();
    let api = openh264_raw_api();
    let mut raw_dec =
        openh264::decoder::Decoder::with_api_config(api, openh264::decoder::DecoderConfig::new())
            .unwrap();

    let mut checked = 0;
    for idx in 0..12 {
        let frame = gradient_frame(idx);
        let packet = enc.encode(&frame).unwrap();
        let decoded = raw_dec.decode(&packet).unwrap();
        let Some(yuv) = decoded else {
            continue; // decoder may buffer; must not happen after warm-up
        };
        let (dw, dh) = yuv.dimensions();
        assert_eq!((dw, dh), (W as usize, H as usize), "dimension mismatch");
        if idx >= 2 {
            let mut rgb = vec![0u8; W as usize * H as usize * 3];
            yuv.write_rgb8(&mut rgb);
            let p = psnr_rgb(&frame.data, &rgb);
            assert!(p > 25.0, "PSNR too low at frame {idx}: {p:.1} dB");
            checked += 1;
        }
    }
    assert!(checked >= 8, "too few frames decoded: {checked}");
}

fn openh264_raw_api() -> openh264::OpenH264API {
    openh264::OpenH264API::from_source()
}

/// (b) Real compression. The stub emits a fixed 57,616-byte packet at this
/// geometry and fails all three bounds structurally.
#[test]
fn packets_are_actually_compressed() {
    let mut enc = encoder();
    let mut sizes = Vec::new();
    for idx in 0..12 {
        let packet = enc.encode(&gradient_frame(idx)).unwrap();
        sizes.push(packet.len());
    }
    let idr = sizes[0];
    let deltas = &sizes[2..];
    let delta_mean = deltas.iter().sum::<usize>() / deltas.len();
    let total: usize = sizes.iter().sum();

    println!("IDR={idr}B delta_mean={delta_mean}B total(12)={total}B raw_i420={RAW_I420}B stub={STUB_PACKET}B");

    assert!(idr < 40_000, "IDR not compressed: {idr}");
    assert!(
        delta_mean < 8_000,
        "delta frames not compressed: {delta_mean}"
    );
    assert!(total < 80_000, "stream not compressed: {total}");
    assert!(
        sizes.iter().all(|s| *s < STUB_PACKET),
        "stub-sized packet seen"
    );
}

/// (c) NAL structure: SPS/PPS/IDR up front, non-IDR steady state, and a
/// fresh IDR after `request_keyframe()`.
#[test]
fn nal_structure_idr_and_keyframe_request() {
    let mut enc = encoder();

    let first = enc.encode(&gradient_frame(0)).unwrap();
    let first_nals = nal_types(&first);
    assert!(
        first_nals.contains(&7),
        "first packet missing SPS: {first_nals:?}"
    );
    assert!(
        first_nals.contains(&8),
        "first packet missing PPS: {first_nals:?}"
    );
    assert!(
        first_nals.contains(&5),
        "first packet missing IDR: {first_nals:?}"
    );

    for idx in 1..5 {
        let _ = enc.encode(&gradient_frame(idx)).unwrap();
    }
    let steady = enc.encode(&gradient_frame(5)).unwrap();
    let steady_nals = nal_types(&steady);
    assert!(
        steady_nals.contains(&1),
        "steady frame not non-IDR: {steady_nals:?}"
    );
    assert!(
        !steady_nals.contains(&5),
        "unexpected IDR mid-stream: {steady_nals:?}"
    );

    enc.request_keyframe();
    let kf = enc.encode(&gradient_frame(6)).unwrap();
    let kf_nals = nal_types(&kf);
    assert!(
        kf_nals.contains(&5),
        "request_keyframe did not force IDR: {kf_nals:?}"
    );
}

/// Round trip through our own decoder: dimensions and the documented
/// out-of-band timestamp contract (decoded timestamp is 0).
#[test]
fn roundtrip_through_our_decoder() {
    let mut enc = encoder();
    let mut dec = OpenH264Decoder::new().unwrap();

    let frame = gradient_frame(0);
    let packet = enc.encode(&frame).unwrap();
    let decoded = dec.decode(&packet).unwrap();

    assert_eq!(decoded.width, W);
    assert_eq!(decoded.height, H);
    assert_eq!(decoded.data.len(), frame.data.len());
    assert_eq!(decoded.timestamp, 0, "timestamps travel out-of-band");
    let p = psnr_rgb(&frame.data, &decoded.data);
    assert!(p > 25.0, "round-trip PSNR too low: {p:.1} dB");
}
