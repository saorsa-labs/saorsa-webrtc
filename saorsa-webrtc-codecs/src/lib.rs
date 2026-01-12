#![deny(clippy::panic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(unsafe_code)]

//! Video and audio codec implementations
//!
//! # Implementation Status
//!
//! The current codec implementations are **stub/simulation implementations** designed for:
//! - Development and testing without external codec libraries
//! - API design validation and interface stabilization
//! - Performance testing of the transport layer
//!
//! ## OpenH264 (Video)
//! - **Current**: Simulation using compression (achieves ~25% size reduction for testing)
//! - **Production**: Will integrate with real openh264 crate when `h264` feature is enabled
//! - **Status**: Safe for development/testing, not for production video calls
//!
//! ## Opus (Audio)
//! - **Current**: Simulation with frame size validation and format conversion
//! - **Production**: Will integrate with real opus crate when `opus` feature is enabled
//! - **Status**: Safe for development/testing, not for production audio calls
//!
//! ## Migration Path
//!
//! To use real codecs in production:
//! 1. Enable the respective features: `features = ["h264", "opus"]`
//! 2. Replace stub implementations in `openh264.rs` and `opus.rs`
//! 3. Run integration tests with actual codec libraries
//!
//! The stub implementations maintain the same API surface, so migration is transparent to users.

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
pub use opus::{AudioFrame, Channels, OpusDecoder, OpusEncoder, OpusEncoderConfig, SampleRate};
