//! Audio capture and playout for Saorsa WebRTC (WP-V1.4).
//!
//! Thin, DSP-free glue between the OS audio device (via [`cpal`]) and the
//! codec layer's [`AudioFrame`] type:
//!
//! - **Capture:** device input → format conversion → mono downmix →
//!   48 kHz resample (when the device rate differs) → 20 ms
//!   [`AudioFrame`] chunks on an async channel.
//! - **Playout:** [`AudioFrame`]s from an async channel → bounded ring
//!   buffer → device output. Underruns insert silence and are counted —
//!   they never panic and never block the audio callback.
//!
//! # No DSP — headset-first
//!
//! There is deliberately **no echo cancellation, gain control, or noise
//! suppression** here. With open speakers the far end will hear itself
//! (acoustic echo); use a headset. DSP is a future workpackage, not an
//! omission by accident.
//!
//! # Threading model
//!
//! [`cpal::Stream`] is not `Send` on all platforms, so each stream lives on
//! its own dedicated OS thread. Audio callbacks do no allocation-heavy or
//! blocking work: capture callbacks forward raw sample blocks over an
//! unbounded lock-free channel to a worker task; playout callbacks pop from
//! a mutex-guarded ring whose critical section is a bounded `copy`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod capture;
pub mod chunker;
pub mod convert;
pub mod devices;
pub mod playout;
pub mod resampler;
pub mod ring;

pub use capture::{AudioCapture, CaptureConfig};
pub use playout::{AudioPlayout, PlayoutConfig};
pub use saorsa_webrtc_codecs::{AudioFrame, Channels, SampleRate};

/// Errors from audio capture/playout.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    /// No default device of the requested direction exists.
    #[error("no default {0} device available")]
    NoDefaultDevice(&'static str),
    /// A device with the requested name was not found.
    #[error("audio device not found: {0}")]
    DeviceNotFound(String),
    /// Device enumeration failed.
    #[error("device enumeration failed: {0}")]
    Enumeration(String),
    /// The device offers no configuration we can use.
    #[error("no usable stream config on device: {0}")]
    UnsupportedFormat(String),
    /// Building the OS stream failed.
    #[error("failed to build audio stream: {0}")]
    BuildStream(String),
    /// Starting the OS stream failed.
    #[error("failed to start audio stream: {0}")]
    PlayStream(String),
    /// The resampler could not be constructed.
    #[error("resampler init failed: {0}")]
    Resampler(String),
    /// The audio thread ended before acknowledging startup.
    #[error("audio thread terminated during startup")]
    ThreadStartup,
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, AudioError>;

/// Samples per 20 ms frame at 48 kHz mono — the frame size this crate emits
/// and consumes ([`SampleRate::Hz48000`], [`Channels::Mono`]).
pub const FRAME_SAMPLES_48K_MONO_20MS: usize = 960;

/// The fixed frame duration this crate produces.
pub const FRAME_MS: u32 = 20;
