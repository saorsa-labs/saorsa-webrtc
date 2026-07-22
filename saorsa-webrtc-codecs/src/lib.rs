#![deny(clippy::panic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(unsafe_code)]

//! Video and audio codec implementations
//!
//! # Implementation Status
//!
//! ## Opus (Audio)
//! - **Real libopus** encode/decode via the `opus` crate — enabled by
//!   default (`opus` feature). Verified by `tests/opus_interop.rs`
//!   (raw-libopus interop, round-trip SNR, compression assertions).
//! - A test-only pass-through simulation survives in `opus::stub` behind
//!   `cfg(any(test, feature = "stub-codecs"))`; it performs no compression
//!   and is unreachable in default builds.
//!
//! ## OpenH264 (Video)
//! - **Still a stub/simulation** (out of scope for the audio revival —
//!   see `docs/design/revival-v0-v1.md`): validates sizes/formats, does
//!   not perform real video coding. Not for production video calls.

pub mod openh264;
pub mod opus;

use bytes::Bytes;

/// Codec error types
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("Dimension mismatch: frame ({frame_width}x{frame_height}) vs config ({cfg_width}x{cfg_height})")]
    DimensionMismatch {
        frame_width: u32,
        frame_height: u32,
        cfg_width: u32,
        cfg_height: u32,
    },
    #[error("Invalid codec data: {0}")]
    InvalidData(&'static str),
    #[error("Numeric overflow in codec operation")]
    Overflow,
    #[error("Codec initialization failed: {0}")]
    InitFailed(String),
    #[error("Feature not implemented: {0}")]
    NotImplemented(&'static str),
    #[error("Invalid dimensions: width={0}, height={1}")]
    InvalidDimensions(u32, u32),
    #[error("Data size exceeds maximum allowed: {actual} > {max}")]
    SizeExceeded { actual: usize, max: usize },
}

/// Codec result type
pub type Result<T> = std::result::Result<T, CodecError>;

/// Maximum allowed dimensions for safety
pub const MAX_WIDTH: u32 = 8192;
pub const MAX_HEIGHT: u32 = 8192;
pub const MAX_RGB_SIZE: usize = 100 * 1024 * 1024; // 100MB

/// Video codec selection
#[derive(Debug, Clone, Copy)]
pub enum VideoCodec {
    H264,
}

/// Audio codec selection
#[derive(Debug, Clone, Copy)]
pub enum AudioCodec {
    Opus,
}

/// Video frame
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timestamp: u64,
}

/// Video encoder trait
pub trait VideoEncoder: Send + Sync {
    fn encode(&mut self, frame: &VideoFrame) -> Result<Bytes>;
    fn request_keyframe(&mut self);
}

/// Video decoder trait
pub trait VideoDecoder: Send + Sync {
    fn decode(&mut self, data: &[u8]) -> Result<VideoFrame>;
}

pub use openh264::{OpenH264Decoder, OpenH264Encoder};
pub use opus::{AudioFrame, Channels, OpusEncoderConfig, SampleRate};
#[cfg(feature = "opus")]
pub use opus::{OpusDecoder, OpusEncoder};
