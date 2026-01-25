//! Media stream management for WebRTC
//!
//! This module handles audio, video, and screen share media streams.
//!
//! # Architecture
//!
//! The module provides a `TrackBackend` trait that abstracts the underlying transport:
//! - `QuicTrackBackend` - Uses QUIC streams via `QuicMediaTransport`
//! - `LegacyWebRtcBackend` - Uses `TrackLocalStaticSample` (deprecated)
//!
//! Tracks (`VideoTrack`, `AudioTrack`) use the `TrackBackend` abstraction,
//! allowing seamless switching between transport backends.
//!
//! **Note:** The legacy-webrtc feature is deprecated and will be removed.
//! New code should use `QuicTrackBackend` for all media transport.

use crate::link_transport::StreamType;
use crate::quic_media_transport::QuicMediaTransport;
use crate::types::MediaType;
use async_trait::async_trait;
use saorsa_webrtc_codecs::{
    OpenH264Decoder, OpenH264Encoder, VideoCodec, VideoDecoder, VideoEncoder, VideoFrame,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{broadcast, RwLock};
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

/// Media-related errors
#[derive(Error, Debug)]
pub enum MediaError {
    /// Device not found
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    /// Stream error
    #[error("Stream error: {0}")]
    StreamError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Backend not connected
    #[error("Backend not connected")]
    NotConnected,

    /// Receive not supported by this backend
    #[error("Receive not supported by this backend")]
    ReceiveNotSupported,

    /// Send failed
    #[error("Send failed: {0}")]
    SendFailed(String),
}

// ============================================================================
// Track Backend Abstraction
// ============================================================================

/// Statistics for a track backend
///
/// Tracks send/receive statistics for monitoring and debugging.
#[derive(Debug, Clone, Default)]
pub struct TrackStats {
    /// Total bytes sent through this track
    pub bytes_sent: u64,
    /// Total bytes received through this track
    pub bytes_received: u64,
    /// Total packets sent
    pub packets_sent: u64,
    /// Total packets received
    pub packets_received: u64,
}

/// Backend abstraction for media track transport
///
/// This trait defines the interface for sending and receiving media data,
/// allowing tracks to work with either QUIC streams or legacy WebRTC.
/// Implementations must be Send + Sync for use across async tasks.
///
/// # Implementations
///
/// - `QuicTrackBackend` - Uses QUIC streams via `QuicMediaTransport`
/// - `LegacyWebRtcBackend` - Uses `TrackLocalStaticSample` (deprecated)
///
/// # Example
///
/// ```ignore
/// use saorsa_webrtc_core::media::{TrackBackend, TrackStats};
///
/// async fn send_data(backend: &dyn TrackBackend, data: &[u8]) -> Result<(), MediaError> {
///     if backend.is_connected() {
///         backend.send(data).await?;
///     }
///     Ok(())
/// }
/// ```
#[async_trait]
pub trait TrackBackend: Send + Sync {
    /// Send RTP packet data through this track
    ///
    /// # Arguments
    ///
    /// * `data` - The RTP packet bytes to send
    ///
    /// # Errors
    ///
    /// Returns error if send fails (e.g., not connected, transport error)
    async fn send(&self, data: &[u8]) -> Result<(), MediaError>;

    /// Receive RTP packet data from this track
    ///
    /// Blocks until data is available or an error occurs.
    ///
    /// # Errors
    ///
    /// Returns error if receive fails or backend doesn't support receive
    async fn recv(&self) -> Result<Vec<u8>, MediaError>;

    /// Check if the backend is connected and ready for use
    ///
    /// # Returns
    ///
    /// `true` if the backend can send/receive data, `false` otherwise
    fn is_connected(&self) -> bool;

    /// Get the backend type name for debugging/logging
    ///
    /// # Returns
    ///
    /// A static string identifying the backend type (e.g., "quic", "webrtc")
    fn backend_type(&self) -> &'static str;

    /// Get current track statistics
    ///
    /// # Returns
    ///
    /// A snapshot of the current send/receive statistics
    fn stats(&self) -> TrackStats;
}

// ============================================================================
// QUIC Track Backend Implementation
// ============================================================================

/// QUIC-native backend for media tracks
///
/// Uses `QuicMediaTransport` to send/receive RTP packets over QUIC streams.
/// This is the preferred backend for new code as it integrates with the
/// unified QUIC transport layer.
///
/// # Stream Mapping
///
/// Media types are mapped to QUIC stream types:
/// - `MediaType::Audio` → `StreamType::Audio`
/// - `MediaType::Video` → `StreamType::Video`
/// - `MediaType::ScreenShare` → `StreamType::Screen`
/// - `MediaType::DataChannel` → `StreamType::Data`
///
/// # Example
///
/// ```ignore
/// use saorsa_webrtc_core::media::QuicTrackBackend;
/// use saorsa_webrtc_core::quic_media_transport::QuicMediaTransport;
/// use saorsa_webrtc_core::types::MediaType;
///
/// let transport = Arc::new(QuicMediaTransport::new());
/// let backend = QuicTrackBackend::new(transport, MediaType::Audio);
/// ```
pub struct QuicTrackBackend {
    /// The underlying QUIC media transport
    transport: Arc<QuicMediaTransport>,
    /// The stream type for this track
    stream_type: StreamType,
    /// Track statistics (protected by RwLock for interior mutability)
    stats: Arc<RwLock<TrackStats>>,
}

impl QuicTrackBackend {
    /// Create a new QUIC track backend
    ///
    /// # Arguments
    ///
    /// * `transport` - The QUIC media transport to use
    /// * `media_type` - The media type for this track
    ///
    /// # Returns
    ///
    /// A new `QuicTrackBackend` configured for the specified media type
    #[must_use]
    pub fn new(transport: Arc<QuicMediaTransport>, media_type: MediaType) -> Self {
        Self {
            transport,
            stream_type: Self::media_type_to_stream_type(media_type),
            stats: Arc::new(RwLock::new(TrackStats::default())),
        }
    }

    /// Create a new QUIC track backend with explicit stream type
    ///
    /// Use this when you need direct control over the stream type mapping.
    ///
    /// # Arguments
    ///
    /// * `transport` - The QUIC media transport to use
    /// * `stream_type` - The specific stream type to use
    #[must_use]
    pub fn with_stream_type(transport: Arc<QuicMediaTransport>, stream_type: StreamType) -> Self {
        Self {
            transport,
            stream_type,
            stats: Arc::new(RwLock::new(TrackStats::default())),
        }
    }

    /// Map MediaType to StreamType
    ///
    /// # Arguments
    ///
    /// * `media_type` - The media type to map
    ///
    /// # Returns
    ///
    /// The corresponding stream type
    #[must_use]
    pub fn media_type_to_stream_type(media_type: MediaType) -> StreamType {
        match media_type {
            MediaType::Audio => StreamType::Audio,
            MediaType::Video => StreamType::Video,
            MediaType::ScreenShare => StreamType::Screen,
            MediaType::DataChannel => StreamType::Data,
        }
    }

    /// Get the stream type for this backend
    #[must_use]
    pub fn stream_type(&self) -> StreamType {
        self.stream_type
    }

    /// Get a reference to the underlying transport
    #[must_use]
    pub fn transport(&self) -> &Arc<QuicMediaTransport> {
        &self.transport
    }

    // =========================================================================
    // Stream Lifecycle Management
    // =========================================================================

    /// Ensure the stream is open for this track
    ///
    /// Opens the appropriate stream type on the transport if not already open.
    /// For audio tracks, opens an audio stream; for video, opens a video stream, etc.
    ///
    /// # Errors
    ///
    /// Returns error if stream cannot be opened or transport is not connected.
    pub async fn ensure_stream(&self) -> Result<(), MediaError> {
        if !self.transport.is_connected().await {
            return Err(MediaError::NotConnected);
        }

        // Open the appropriate stream for this track type
        self.transport
            .open_stream(self.stream_type)
            .await
            .map_err(|e| MediaError::StreamError(format!("Failed to open stream: {}", e)))?;

        tracing::debug!(
            stream_type = ?self.stream_type,
            "Stream opened for track backend"
        );
        Ok(())
    }

    /// Close the stream for this track
    ///
    /// Closes the stream on the transport, freeing resources.
    /// The track can still be used after this, but will need to reopen the stream.
    ///
    /// # Returns
    ///
    /// `true` if the stream was closed, `false` if it wasn't open.
    pub async fn close_stream(&self) -> bool {
        let closed = self.transport.close_stream(self.stream_type).await;

        if closed {
            tracing::debug!(
                stream_type = ?self.stream_type,
                "Stream closed for track backend"
            );
        }
        closed
    }

    /// Check if the stream is currently open
    ///
    /// Uses `ensure_stream_open` to check if the stream exists.
    ///
    /// # Returns
    ///
    /// `true` if the stream is open and ready, `false` otherwise.
    pub async fn is_stream_open(&self) -> bool {
        // Check by looking at the open stream types
        let open_types = self.transport.open_stream_types().await;
        open_types.contains(&self.stream_type)
    }
}

#[async_trait]
impl TrackBackend for QuicTrackBackend {
    async fn send(&self, data: &[u8]) -> Result<(), MediaError> {
        // Use the transport's send_rtp method
        self.transport
            .send_rtp(self.stream_type, data)
            .await
            .map_err(|e| MediaError::SendFailed(e.to_string()))?;

        // Update statistics
        {
            let mut stats = self.stats.write().await;
            stats.bytes_sent += data.len() as u64;
            stats.packets_sent += 1;
        }

        Ok(())
    }

    async fn recv(&self) -> Result<Vec<u8>, MediaError> {
        // Currently, recv_rtp returns an error indicating integration is needed.
        // In a full implementation, this would receive from the transport.
        // For now, we return the appropriate error.
        self.transport
            .recv_rtp()
            .await
            .map(|(_, data)| {
                // Note: In production, we would update stats here
                // This path currently isn't reached as recv_rtp returns error
                data
            })
            .map_err(|e| MediaError::StreamError(e.to_string()))
    }

    fn is_connected(&self) -> bool {
        // Check if the transport is connected synchronously
        // We use try_read to avoid blocking
        futures::executor::block_on(async { self.transport.is_connected().await })
    }

    fn backend_type(&self) -> &'static str {
        "quic"
    }

    fn stats(&self) -> TrackStats {
        // Use try_read to get stats without blocking
        // If we can't acquire the lock, return default stats
        futures::executor::block_on(async { self.stats.read().await.clone() })
    }
}

// Ensure QuicTrackBackend is Send + Sync at compile time
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<QuicTrackBackend>();
};

// ============================================================================
// Legacy WebRTC Backend Implementation
// ============================================================================

/// Legacy WebRTC backend for media tracks
///
/// Wraps `TrackLocalStaticSample` for backward compatibility during migration
/// from legacy WebRTC to QUIC-native transport.
///
/// # Deprecation Notice
///
/// This backend is deprecated and will be removed when the `legacy-webrtc`
/// feature is fully phased out. New code should use `QuicTrackBackend` instead.
///
/// # Limitations
///
/// - **Receive not supported**: WebRTC tracks in this mode are send-only.
///   Calling `recv()` will return `MediaError::ReceiveNotSupported`.
/// - **Blocking on async**: Some operations use blocking synchronization.
#[deprecated(
    since = "0.3.0",
    note = "Use QuicTrackBackend for new code. Legacy WebRTC will be removed."
)]
pub struct LegacyWebRtcBackend {
    /// The underlying WebRTC track
    track: Arc<TrackLocalStaticSample>,
    /// The media type of this track
    track_type: MediaType,
    /// Track statistics
    stats: Arc<RwLock<TrackStats>>,
    /// Connected flag (WebRTC tracks are always "connected" once created)
    connected: bool,
}

#[allow(deprecated)]
impl LegacyWebRtcBackend {
    /// Create a new legacy WebRTC backend
    ///
    /// # Arguments
    ///
    /// * `track` - The WebRTC track to wrap
    /// * `track_type` - The media type of the track
    ///
    /// # Returns
    ///
    /// A new `LegacyWebRtcBackend` wrapping the track
    #[must_use]
    pub fn new(track: Arc<TrackLocalStaticSample>, track_type: MediaType) -> Self {
        Self {
            track,
            track_type,
            stats: Arc::new(RwLock::new(TrackStats::default())),
            connected: true, // WebRTC tracks are connected once created
        }
    }

    /// Get the underlying WebRTC track
    #[must_use]
    pub fn webrtc_track(&self) -> &Arc<TrackLocalStaticSample> {
        &self.track
    }

    /// Get the media type of this track
    #[must_use]
    pub fn track_type(&self) -> MediaType {
        self.track_type.clone()
    }
}

#[allow(deprecated)]
#[async_trait]
impl TrackBackend for LegacyWebRtcBackend {
    async fn send(&self, data: &[u8]) -> Result<(), MediaError> {
        use webrtc::media::Sample;

        if !self.connected {
            return Err(MediaError::NotConnected);
        }

        // Create a media sample from the data
        let sample = Sample {
            data: bytes::Bytes::copy_from_slice(data),
            duration: std::time::Duration::from_millis(20), // Typical audio frame
            ..Default::default()
        };

        // Write the sample to the WebRTC track
        self.track
            .write_sample(&sample)
            .await
            .map_err(|e| MediaError::SendFailed(e.to_string()))?;

        // Update statistics
        {
            let mut stats = self.stats.write().await;
            stats.bytes_sent += data.len() as u64;
            stats.packets_sent += 1;
        }

        Ok(())
    }

    async fn recv(&self) -> Result<Vec<u8>, MediaError> {
        // Legacy WebRTC tracks don't support direct receive.
        // In WebRTC, receiving is handled by RTP receivers and track events.
        Err(MediaError::ReceiveNotSupported)
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn backend_type(&self) -> &'static str {
        "webrtc"
    }

    fn stats(&self) -> TrackStats {
        futures::executor::block_on(async { self.stats.read().await.clone() })
    }
}

// Ensure LegacyWebRtcBackend is Send + Sync at compile time
#[allow(deprecated)]
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LegacyWebRtcBackend>();
};

/// Media events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaEvent {
    /// Device connected
    DeviceConnected {
        /// Device identifier
        device_id: String,
    },
    /// Device disconnected
    DeviceDisconnected {
        /// Device identifier
        device_id: String,
    },
    /// Stream started
    StreamStarted {
        /// Stream identifier
        stream_id: String,
    },
    /// Stream stopped
    StreamStopped {
        /// Stream identifier
        stream_id: String,
    },
}

/// Audio device
#[derive(Debug, Clone)]
pub struct AudioDevice {
    /// Device identifier
    pub id: String,
    /// Device name
    pub name: String,
}

/// Video device
#[derive(Debug, Clone)]
pub struct VideoDevice {
    /// Device identifier
    pub id: String,
    /// Device name
    pub name: String,
}

/// Audio track with backend abstraction
///
/// An audio track that can use either QUIC or legacy WebRTC as its transport backend.
/// Audio processing (encoding/decoding) is typically handled by external codecs (e.g., Opus).
///
/// # Creating an AudioTrack
///
/// ```ignore
/// // With QUIC backend (recommended)
/// let transport = Arc::new(QuicMediaTransport::new());
/// let track = AudioTrack::with_quic("audio-1", transport);
///
/// // With legacy WebRTC backend (deprecated)
/// let track = AudioTrack::with_webrtc("audio-1", webrtc_track);
/// ```
pub struct AudioTrack {
    /// Track identifier
    pub id: String,
    /// Transport backend (QUIC or legacy WebRTC)
    backend: Arc<dyn TrackBackend>,
}

impl AudioTrack {
    /// Create a new audio track with the specified backend
    ///
    /// # Arguments
    ///
    /// * `id` - Unique track identifier
    /// * `backend` - The transport backend to use
    #[must_use]
    pub fn new_with_backend(id: String, backend: Arc<dyn TrackBackend>) -> Self {
        Self { id, backend }
    }

    /// Create a new audio track with QUIC backend
    ///
    /// This is the preferred method for new code.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique track identifier
    /// * `transport` - The QUIC media transport
    #[must_use]
    pub fn with_quic(id: impl Into<String>, transport: Arc<QuicMediaTransport>) -> Self {
        let backend = Arc::new(QuicTrackBackend::new(transport, MediaType::Audio));
        Self::new_with_backend(id.into(), backend)
    }

    /// Create a new audio track with legacy WebRTC backend
    ///
    /// **Deprecated**: Use `with_quic` for new code.
    #[deprecated(since = "0.3.0", note = "Use with_quic for new code")]
    #[allow(deprecated)]
    #[must_use]
    pub fn with_webrtc(id: impl Into<String>, webrtc_track: Arc<TrackLocalStaticSample>) -> Self {
        let backend = Arc::new(LegacyWebRtcBackend::new(webrtc_track, MediaType::Audio));
        Self::new_with_backend(id.into(), backend)
    }

    /// Get the underlying backend
    #[must_use]
    pub fn backend(&self) -> &Arc<dyn TrackBackend> {
        &self.backend
    }

    /// Check if the track is connected
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.backend.is_connected()
    }

    /// Get track statistics
    #[must_use]
    pub fn stats(&self) -> TrackStats {
        self.backend.stats()
    }

    /// Send audio data through the backend
    ///
    /// # Arguments
    ///
    /// * `data` - Encoded audio data (e.g., Opus packets)
    ///
    /// # Errors
    ///
    /// Returns error if backend is not connected or send fails.
    pub async fn send_audio(&self, data: &[u8]) -> Result<(), MediaError> {
        self.backend.send(data).await
    }

    /// Receive audio data from the backend
    ///
    /// # Errors
    ///
    /// Returns error if backend doesn't support receive or receive fails.
    pub async fn recv_audio(&self) -> Result<Vec<u8>, MediaError> {
        self.backend.recv().await
    }
}

/// Video track with backend abstraction
///
/// A video track that can use either QUIC or legacy WebRTC as its transport backend.
/// The track maintains optional encoder/decoder for codec integration and delegates
/// actual transport to the backend.
///
/// # Creating a VideoTrack
///
/// ```ignore
/// // With QUIC backend (recommended)
/// let transport = Arc::new(QuicMediaTransport::new());
/// let track = VideoTrack::with_quic("video-1", transport, 1280, 720);
///
/// // With legacy WebRTC backend (deprecated)
/// let track = VideoTrack::with_webrtc("video-1", webrtc_track, 1280, 720);
/// ```
pub struct VideoTrack {
    /// Track identifier
    pub id: String,
    /// Transport backend (QUIC or legacy WebRTC)
    backend: Arc<dyn TrackBackend>,
    /// Video encoder (optional)
    pub encoder: Option<Box<dyn VideoEncoder>>,
    /// Video decoder (optional)
    pub decoder: Option<Box<dyn VideoDecoder>>,
    /// Track width
    pub width: u32,
    /// Track height
    pub height: u32,
}

impl VideoTrack {
    /// Create a new video track with the specified backend
    ///
    /// # Arguments
    ///
    /// * `id` - Unique track identifier
    /// * `backend` - The transport backend to use
    /// * `width` - Video width in pixels
    /// * `height` - Video height in pixels
    #[must_use]
    pub fn new_with_backend(
        id: String,
        backend: Arc<dyn TrackBackend>,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            id,
            backend,
            encoder: None,
            decoder: None,
            width,
            height,
        }
    }

    /// Create a new video track with QUIC backend
    ///
    /// This is the preferred method for new code.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique track identifier
    /// * `transport` - The QUIC media transport
    /// * `width` - Video width in pixels
    /// * `height` - Video height in pixels
    #[must_use]
    pub fn with_quic(
        id: impl Into<String>,
        transport: Arc<QuicMediaTransport>,
        width: u32,
        height: u32,
    ) -> Self {
        let backend = Arc::new(QuicTrackBackend::new(transport, MediaType::Video));
        Self::new_with_backend(id.into(), backend, width, height)
    }

    /// Create a new video track with legacy WebRTC backend
    ///
    /// **Deprecated**: Use `with_quic` for new code.
    #[deprecated(since = "0.3.0", note = "Use with_quic for new code")]
    #[allow(deprecated)]
    #[must_use]
    pub fn with_webrtc(
        id: impl Into<String>,
        webrtc_track: Arc<TrackLocalStaticSample>,
        width: u32,
        height: u32,
    ) -> Self {
        let backend = Arc::new(LegacyWebRtcBackend::new(webrtc_track, MediaType::Video));
        Self::new_with_backend(id.into(), backend, width, height)
    }

    /// Create a new video track (legacy compatibility)
    ///
    /// **Deprecated**: Use `with_quic` or `new_with_backend` instead.
    #[deprecated(since = "0.3.0", note = "Use with_quic or new_with_backend instead")]
    #[allow(deprecated)]
    pub fn new(
        id: String,
        webrtc_track: Arc<TrackLocalStaticSample>,
        width: u32,
        height: u32,
    ) -> Self {
        Self::with_webrtc(id, webrtc_track, width, height)
    }

    /// Get the underlying backend
    #[must_use]
    pub fn backend(&self) -> &Arc<dyn TrackBackend> {
        &self.backend
    }

    /// Check if the track is connected
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.backend.is_connected()
    }

    /// Get track statistics
    #[must_use]
    pub fn stats(&self) -> TrackStats {
        self.backend.stats()
    }

    /// Send encoded video frame through the backend
    ///
    /// If an encoder is configured, use `encode_and_send` instead.
    ///
    /// # Errors
    ///
    /// Returns error if backend is not connected or send fails.
    pub async fn send_frame(&self, frame_data: &[u8]) -> Result<(), MediaError> {
        self.backend.send(frame_data).await
    }

    /// Receive encoded video frame from the backend
    ///
    /// # Errors
    ///
    /// Returns error if backend doesn't support receive or receive fails.
    pub async fn recv_frame(&self) -> Result<Vec<u8>, MediaError> {
        self.backend.recv().await
    }

    /// Encode a frame and send it
    ///
    /// # Errors
    ///
    /// Returns error if encoding fails or backend send fails.
    pub async fn encode_and_send(&mut self, raw_frame: &[u8]) -> Result<(), MediaError> {
        let encoded = self
            .encode_frame(raw_frame)
            .map_err(|e| MediaError::ConfigError(format!("Encoding failed: {}", e)))?;
        self.backend.send(&encoded).await
    }

    /// Add H.264 encoder to this track
    pub fn with_h264_encoder(mut self) -> anyhow::Result<Self> {
        let encoder = OpenH264Encoder::new()?;
        // Configure encoder with track dimensions
        // Note: In the full implementation, this would configure the encoder
        // For now, we assume the encoder can handle the dimensions
        self.encoder = Some(Box::new(encoder));
        Ok(self)
    }

    /// Add H.264 decoder to this track
    pub fn with_h264_decoder(mut self) -> anyhow::Result<Self> {
        let decoder = OpenH264Decoder::new()?;
        self.decoder = Some(Box::new(decoder));
        Ok(self)
    }

    /// Encode a video frame
    pub fn encode_frame(&mut self, frame_data: &[u8]) -> anyhow::Result<Vec<u8>> {
        if let Some(encoder) = &mut self.encoder {
            let frame = VideoFrame {
                data: frame_data.to_vec(),
                width: self.width,
                height: self.height,
                timestamp: 0, // TODO: Add timestamp
            };
            let encoded = encoder.encode(&frame)?;
            Ok(encoded.to_vec())
        } else {
            // No encoder - return raw data
            Ok(frame_data.to_vec())
        }
    }

    /// Decode a video frame
    pub fn decode_frame(&mut self, encoded_data: &[u8]) -> anyhow::Result<Vec<u8>> {
        if let Some(decoder) = &mut self.decoder {
            let frame = decoder.decode(encoded_data)?;
            Ok(frame.data)
        } else {
            // No decoder - assume raw data
            Ok(encoded_data.to_vec())
        }
    }
}

// ============================================================================
// Generic Track Type
// ============================================================================

/// Unified track type for type-safe track handling
///
/// `GenericTrack` unifies audio, video, and screen share tracks into a single
/// enum, enabling type-safe handling of different track types with a common
/// interface.
///
/// # Track Types
///
/// - `Audio` - Audio track (Opus, etc.)
/// - `Video` - Video track (H.264, VP8, etc.)
/// - `Screen` - Screen share (uses VideoTrack internally)
///
/// # Example
///
/// ```ignore
/// let audio = GenericTrack::audio(AudioTrack::with_quic("audio-1", transport.clone()));
/// let video = GenericTrack::video(VideoTrack::with_quic("video-1", transport.clone(), 1280, 720));
///
/// for track in [audio, video] {
///     println!("Track: {} ({})", track.id(), track.media_type());
///     if track.is_connected() {
///         // Send data...
///     }
/// }
/// ```
pub enum GenericTrack {
    /// Audio track
    Audio(AudioTrack),
    /// Video track
    Video(VideoTrack),
    /// Screen share (uses VideoTrack internally)
    Screen(VideoTrack),
}

impl std::fmt::Debug for GenericTrack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Audio(track) => f
                .debug_struct("GenericTrack::Audio")
                .field("id", &track.id)
                .finish(),
            Self::Video(track) => f
                .debug_struct("GenericTrack::Video")
                .field("id", &track.id)
                .field("width", &track.width)
                .field("height", &track.height)
                .finish(),
            Self::Screen(track) => f
                .debug_struct("GenericTrack::Screen")
                .field("id", &track.id)
                .field("width", &track.width)
                .field("height", &track.height)
                .finish(),
        }
    }
}

impl GenericTrack {
    /// Create an audio track wrapper
    #[must_use]
    pub fn audio(track: AudioTrack) -> Self {
        Self::Audio(track)
    }

    /// Create a video track wrapper
    #[must_use]
    pub fn video(track: VideoTrack) -> Self {
        Self::Video(track)
    }

    /// Create a screen share track wrapper
    #[must_use]
    pub fn screen(track: VideoTrack) -> Self {
        Self::Screen(track)
    }

    /// Get the track ID
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Audio(track) => &track.id,
            Self::Video(track) | Self::Screen(track) => &track.id,
        }
    }

    /// Get the media type
    #[must_use]
    pub fn media_type(&self) -> MediaType {
        match self {
            Self::Audio(_) => MediaType::Audio,
            Self::Video(_) => MediaType::Video,
            Self::Screen(_) => MediaType::ScreenShare,
        }
    }

    /// Get the underlying backend
    #[must_use]
    pub fn backend(&self) -> &Arc<dyn TrackBackend> {
        match self {
            Self::Audio(track) => track.backend(),
            Self::Video(track) | Self::Screen(track) => track.backend(),
        }
    }

    /// Check if the track is connected
    #[must_use]
    pub fn is_connected(&self) -> bool {
        match self {
            Self::Audio(track) => track.is_connected(),
            Self::Video(track) | Self::Screen(track) => track.is_connected(),
        }
    }

    /// Get track statistics
    #[must_use]
    pub fn stats(&self) -> TrackStats {
        match self {
            Self::Audio(track) => track.stats(),
            Self::Video(track) | Self::Screen(track) => track.stats(),
        }
    }

    /// Send data through the track
    ///
    /// # Errors
    ///
    /// Returns error if backend is not connected or send fails.
    pub async fn send(&self, data: &[u8]) -> Result<(), MediaError> {
        match self {
            Self::Audio(track) => track.send_audio(data).await,
            Self::Video(track) | Self::Screen(track) => track.send_frame(data).await,
        }
    }

    /// Receive data from the track
    ///
    /// # Errors
    ///
    /// Returns error if backend doesn't support receive or receive fails.
    pub async fn recv(&self) -> Result<Vec<u8>, MediaError> {
        match self {
            Self::Audio(track) => track.recv_audio().await,
            Self::Video(track) | Self::Screen(track) => track.recv_frame().await,
        }
    }

    /// Check if this is an audio track
    #[must_use]
    pub fn is_audio(&self) -> bool {
        matches!(self, Self::Audio(_))
    }

    /// Check if this is a video track
    #[must_use]
    pub fn is_video(&self) -> bool {
        matches!(self, Self::Video(_))
    }

    /// Check if this is a screen share track
    #[must_use]
    pub fn is_screen(&self) -> bool {
        matches!(self, Self::Screen(_))
    }

    /// Get as audio track reference
    #[must_use]
    pub fn as_audio(&self) -> Option<&AudioTrack> {
        match self {
            Self::Audio(track) => Some(track),
            _ => None,
        }
    }

    /// Get as video track reference
    #[must_use]
    pub fn as_video(&self) -> Option<&VideoTrack> {
        match self {
            Self::Video(track) => Some(track),
            _ => None,
        }
    }

    /// Get as screen track reference
    #[must_use]
    pub fn as_screen(&self) -> Option<&VideoTrack> {
        match self {
            Self::Screen(track) => Some(track),
            _ => None,
        }
    }
}

/// WebRTC media track wrapper
#[derive(Debug, Clone)]
pub struct WebRtcTrack {
    /// Local WebRTC track
    pub track: Arc<TrackLocalStaticSample>,
    /// Track type
    pub track_type: MediaType,
    /// Track ID
    pub id: String,
}

/// Media stream
#[derive(Debug, Clone)]
pub struct MediaStream {
    /// Stream identifier
    pub id: String,
}

/// Media stream manager
///
/// Manages media tracks for a call, supporting both QUIC-native and legacy WebRTC backends.
/// The manager can be configured with a QUIC transport to create QUIC-backed tracks.
///
/// # Creating QUIC-backed tracks
///
/// ```ignore
/// let transport = Arc::new(QuicMediaTransport::new());
/// let mut manager = MediaStreamManager::with_quic_transport(transport);
///
/// let audio = manager.create_quic_audio_track().await?;
/// let video = manager.create_quic_video_track(1280, 720).await?;
/// ```
pub struct MediaStreamManager {
    event_sender: broadcast::Sender<MediaEvent>,
    #[allow(dead_code)]
    audio_devices: Vec<AudioDevice>,
    #[allow(dead_code)]
    video_devices: Vec<VideoDevice>,
    webrtc_tracks: Vec<WebRtcTrack>,
    /// QUIC transport for creating QUIC-backed tracks
    quic_transport: Option<Arc<QuicMediaTransport>>,
    /// Generic tracks (QUIC-backed)
    tracks: Vec<GenericTrack>,
}

impl MediaStreamManager {
    /// Create new media stream manager
    #[must_use]
    pub fn new() -> Self {
        let (event_sender, _) = broadcast::channel(100);
        Self {
            event_sender,
            audio_devices: Vec::new(),
            video_devices: Vec::new(),
            webrtc_tracks: Vec::new(),
            quic_transport: None,
            tracks: Vec::new(),
        }
    }

    /// Create a new media stream manager with QUIC transport
    ///
    /// This is the preferred constructor for new code.
    #[must_use]
    pub fn with_quic_transport(transport: Arc<QuicMediaTransport>) -> Self {
        let (event_sender, _) = broadcast::channel(100);
        Self {
            event_sender,
            audio_devices: Vec::new(),
            video_devices: Vec::new(),
            webrtc_tracks: Vec::new(),
            quic_transport: Some(transport),
            tracks: Vec::new(),
        }
    }

    /// Set the QUIC transport for this manager
    ///
    /// Allows setting or updating the QUIC transport after creation.
    pub fn set_quic_transport(&mut self, transport: Arc<QuicMediaTransport>) {
        self.quic_transport = Some(transport);
    }

    /// Check if QUIC transport is available
    #[must_use]
    pub fn has_quic_transport(&self) -> bool {
        self.quic_transport.is_some()
    }

    /// Get all generic tracks (QUIC-backed)
    #[must_use]
    pub fn get_tracks(&self) -> &[GenericTrack] {
        &self.tracks
    }

    /// Get the QUIC transport, if set
    #[must_use]
    pub fn quic_transport(&self) -> Option<&Arc<QuicMediaTransport>> {
        self.quic_transport.as_ref()
    }

    /// Initialize media devices
    ///
    /// # Errors
    ///
    /// Returns error if device initialization fails
    #[tracing::instrument(skip(self))]
    pub async fn initialize(&self) -> Result<(), MediaError> {
        tracing::debug!("Enumerating media devices");

        // For now, add some fake devices for testing
        // In a real implementation, this would enumerate actual hardware devices
        let audio_device = AudioDevice {
            id: "default-audio".to_string(),
            name: "Default Audio Device".to_string(),
        };

        let video_device = VideoDevice {
            id: "default-video".to_string(),
            name: "Default Video Device".to_string(),
        };

        // Emit device connected events
        let _ = self.event_sender.send(MediaEvent::DeviceConnected {
            device_id: audio_device.id.clone(),
        });

        let _ = self.event_sender.send(MediaEvent::DeviceConnected {
            device_id: video_device.id.clone(),
        });

        tracing::debug!(
            audio_devices = 1,
            video_devices = 1,
            "Media devices enumerated"
        );
        Ok(())
    }

    /// Get available audio devices
    #[must_use]
    pub fn get_audio_devices(&self) -> &[AudioDevice] {
        // Return empty for now, as we can't enumerate real devices easily
        // In a real implementation, this would return actual devices
        &[]
    }

    /// Get available video devices
    #[must_use]
    pub fn get_video_devices(&self) -> &[VideoDevice] {
        // Return empty for now
        &[]
    }

    /// Create a new audio track
    ///
    /// # Errors
    ///
    /// Returns error if track creation fails
    pub async fn create_audio_track(&mut self) -> Result<&WebRtcTrack, MediaError> {
        let track_id = format!("audio-{}", self.webrtc_tracks.len());
        tracing::info!(track_id = %track_id, "Creating audio track");

        let codec = RTCRtpCodecCapability {
            mime_type: "audio/opus".to_string(),
            clock_rate: 48000,
            channels: 2,
            sdp_fmtp_line: "".to_string(),
            rtcp_feedback: vec![],
        };
        tracing::debug!(codec = %codec.mime_type, clock_rate = codec.clock_rate, "Audio codec configured");

        let track = Arc::new(TrackLocalStaticSample::new(
            codec,
            track_id.clone(),
            "audio".to_string(),
        ));

        let webrtc_track = WebRtcTrack {
            track,
            track_type: MediaType::Audio,
            id: track_id.clone(),
        };

        self.webrtc_tracks.push(webrtc_track);
        tracing::info!(track_id = %track_id, "Audio track created");

        self.webrtc_tracks.last().ok_or(MediaError::StreamError(
            "Failed to get last track after push".to_string(),
        ))
    }

    /// Create a new video track
    ///
    /// # Errors
    ///
    /// Returns error if track creation fails
    pub async fn create_video_track(&mut self) -> Result<&WebRtcTrack, MediaError> {
        let track_id = format!("video-{}", self.webrtc_tracks.len());
        tracing::info!(track_id = %track_id, "Creating video track");

        let codec = RTCRtpCodecCapability {
            mime_type: "video/VP8".to_string(),
            clock_rate: 90000,
            channels: 0,
            sdp_fmtp_line: "".to_string(),
            rtcp_feedback: vec![],
        };
        tracing::debug!(codec = %codec.mime_type, clock_rate = codec.clock_rate, "Video codec configured");

        let track = Arc::new(TrackLocalStaticSample::new(
            codec,
            track_id.clone(),
            "video".to_string(),
        ));

        let webrtc_track = WebRtcTrack {
            track,
            track_type: MediaType::Video,
            id: track_id.clone(),
        };

        self.webrtc_tracks.push(webrtc_track);
        tracing::info!(track_id = %track_id, "Video track created");

        self.webrtc_tracks.last().ok_or(MediaError::StreamError(
            "Failed to get last track after push".to_string(),
        ))
    }

    /// Create a new video track with codec support
    ///
    /// **Note**: This method creates a legacy WebRTC-backed video track.
    /// For QUIC-native tracks, use `create_quic_video_track`.
    ///
    /// # Errors
    ///
    /// Returns error if track creation fails
    #[allow(deprecated)]
    pub async fn create_video_track_with_codec(
        &mut self,
        codec: VideoCodec,
        width: u32,
        height: u32,
    ) -> Result<VideoTrack, MediaError> {
        let track_id = format!("video-{}", self.webrtc_tracks.len());

        // Use H.264 codec for WebRTC when encoding is enabled
        let mime_type = match codec {
            VideoCodec::H264 => "video/H264".to_string(),
            // VideoCodec::VP8 => "video/VP8".to_string(),
            // VideoCodec::VP9 => "video/VP9".to_string(),
        };

        let codec_capability = RTCRtpCodecCapability {
            mime_type,
            clock_rate: 90000,
            channels: 0,
            sdp_fmtp_line: "".to_string(),
            rtcp_feedback: vec![],
        };

        let webrtc_track = Arc::new(TrackLocalStaticSample::new(
            codec_capability,
            track_id.clone(),
            "video".to_string(),
        ));

        // Use deprecated legacy constructor (this method is for backward compatibility)
        let mut video_track = VideoTrack::new(track_id, webrtc_track, width, height);

        // Add encoder based on codec
        match codec {
            VideoCodec::H264 => {
                video_track = video_track
                    .with_h264_encoder()
                    .map_err(|e| MediaError::ConfigError(e.to_string()))?;
            }
        }

        Ok(video_track)
    }

    /// Get all WebRTC tracks
    #[must_use]
    pub fn get_webrtc_tracks(&self) -> &[WebRtcTrack] {
        &self.webrtc_tracks
    }

    /// Subscribe to media events
    #[must_use]
    pub fn subscribe_events(&self) -> broadcast::Receiver<MediaEvent> {
        self.event_sender.subscribe()
    }

    /// Remove a track by ID
    ///
    /// Returns true if the track was found and removed
    pub fn remove_track(&mut self, track_id: &str) -> bool {
        // First try to remove from webrtc_tracks
        if let Some(pos) = self.webrtc_tracks.iter().position(|t| t.id == track_id) {
            let track = &self.webrtc_tracks[pos];
            tracing::info!(track_id = %track_id, track_type = ?track.track_type, "Removing WebRTC track");
            self.webrtc_tracks.remove(pos);
            return true;
        }

        // Then try to remove from generic tracks
        if let Some(pos) = self.tracks.iter().position(|t| t.id() == track_id) {
            let track = &self.tracks[pos];
            tracing::info!(track_id = %track_id, track_type = ?track.media_type(), "Removing generic track");
            self.tracks.remove(pos);
            return true;
        }

        tracing::warn!("Track not found for removal: {}", track_id);
        false
    }

    // =========================================================================
    // QUIC Track Creation Methods
    // =========================================================================

    /// Create a QUIC-backed audio track
    ///
    /// Requires QUIC transport to be set via `with_quic_transport` or `set_quic_transport`.
    ///
    /// # Errors
    ///
    /// Returns error if QUIC transport is not configured.
    pub fn create_quic_audio_track(&mut self) -> Result<&GenericTrack, MediaError> {
        let transport = self
            .quic_transport
            .as_ref()
            .ok_or_else(|| MediaError::ConfigError("QUIC transport not configured".to_string()))?;

        let track_id = format!("audio-{}", self.tracks.len() + self.webrtc_tracks.len());
        tracing::info!(track_id = %track_id, "Creating QUIC audio track");

        let audio_track = AudioTrack::with_quic(&track_id, Arc::clone(transport));
        let generic = GenericTrack::audio(audio_track);

        self.tracks.push(generic);

        // Emit stream started event
        let _ = self.event_sender.send(MediaEvent::StreamStarted {
            stream_id: track_id.clone(),
        });

        tracing::info!(track_id = %track_id, "QUIC audio track created");
        self.tracks.last().ok_or(MediaError::StreamError(
            "Failed to get last track after push".to_string(),
        ))
    }

    /// Create a QUIC-backed video track
    ///
    /// Requires QUIC transport to be set via `with_quic_transport` or `set_quic_transport`.
    ///
    /// # Arguments
    ///
    /// * `width` - Video width in pixels
    /// * `height` - Video height in pixels
    ///
    /// # Errors
    ///
    /// Returns error if QUIC transport is not configured.
    pub fn create_quic_video_track(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<&GenericTrack, MediaError> {
        let transport = self
            .quic_transport
            .as_ref()
            .ok_or_else(|| MediaError::ConfigError("QUIC transport not configured".to_string()))?;

        let track_id = format!("video-{}", self.tracks.len() + self.webrtc_tracks.len());
        tracing::info!(track_id = %track_id, width = width, height = height, "Creating QUIC video track");

        let video_track = VideoTrack::with_quic(&track_id, Arc::clone(transport), width, height);
        let generic = GenericTrack::video(video_track);

        self.tracks.push(generic);

        // Emit stream started event
        let _ = self.event_sender.send(MediaEvent::StreamStarted {
            stream_id: track_id.clone(),
        });

        tracing::info!(track_id = %track_id, "QUIC video track created");
        self.tracks.last().ok_or(MediaError::StreamError(
            "Failed to get last track after push".to_string(),
        ))
    }

    /// Create a QUIC-backed screen share track
    ///
    /// Screen shares are similar to video tracks but use a different stream type.
    ///
    /// # Arguments
    ///
    /// * `width` - Screen width in pixels
    /// * `height` - Screen height in pixels
    ///
    /// # Errors
    ///
    /// Returns error if QUIC transport is not configured.
    pub fn create_quic_screen_track(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<&GenericTrack, MediaError> {
        let transport = self
            .quic_transport
            .as_ref()
            .ok_or_else(|| MediaError::ConfigError("QUIC transport not configured".to_string()))?;

        let track_id = format!("screen-{}", self.tracks.len() + self.webrtc_tracks.len());
        tracing::info!(track_id = %track_id, width = width, height = height, "Creating QUIC screen track");

        // Use QuicTrackBackend with Screen stream type directly
        let backend = Arc::new(QuicTrackBackend::with_stream_type(
            Arc::clone(transport),
            StreamType::Screen,
        ));
        let video_track = VideoTrack::new_with_backend(track_id.clone(), backend, width, height);
        let generic = GenericTrack::screen(video_track);

        self.tracks.push(generic);

        // Emit stream started event
        let _ = self.event_sender.send(MediaEvent::StreamStarted {
            stream_id: track_id.clone(),
        });

        tracing::info!(track_id = %track_id, "QUIC screen track created");
        self.tracks.last().ok_or(MediaError::StreamError(
            "Failed to get last track after push".to_string(),
        ))
    }

    /// Create a QUIC-backed video track with H.264 encoding
    ///
    /// # Arguments
    ///
    /// * `width` - Video width in pixels
    /// * `height` - Video height in pixels
    ///
    /// # Errors
    ///
    /// Returns error if QUIC transport is not configured or encoder creation fails.
    pub fn create_quic_video_track_h264(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<VideoTrack, MediaError> {
        let transport = self
            .quic_transport
            .as_ref()
            .ok_or_else(|| MediaError::ConfigError("QUIC transport not configured".to_string()))?;

        let track_id = format!("video-{}", self.tracks.len() + self.webrtc_tracks.len());
        tracing::info!(track_id = %track_id, codec = "H264", "Creating QUIC video track with H.264");

        let video_track = VideoTrack::with_quic(&track_id, Arc::clone(transport), width, height)
            .with_h264_encoder()
            .map_err(|e| {
                MediaError::ConfigError(format!("H.264 encoder creation failed: {}", e))
            })?;

        Ok(video_track)
    }

    /// Get track by ID (searches both WebRTC and generic tracks)
    #[must_use]
    pub fn get_track_by_id(&self, track_id: &str) -> Option<TrackRef<'_>> {
        // Check webrtc tracks first
        if let Some(track) = self.webrtc_tracks.iter().find(|t| t.id == track_id) {
            return Some(TrackRef::WebRtc(track));
        }

        // Check generic tracks
        if let Some(track) = self.tracks.iter().find(|t| t.id() == track_id) {
            return Some(TrackRef::Generic(track));
        }

        None
    }
}

/// Reference to either a WebRTC track or a generic track
pub enum TrackRef<'a> {
    /// Legacy WebRTC track
    WebRtc(&'a WebRtcTrack),
    /// Generic track (QUIC-backed)
    Generic(&'a GenericTrack),
}

impl TrackRef<'_> {
    /// Get the track ID
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::WebRtc(t) => &t.id,
            Self::Generic(t) => t.id(),
        }
    }

    /// Get the media type
    #[must_use]
    pub fn media_type(&self) -> MediaType {
        match self {
            Self::WebRtc(t) => t.track_type.clone(),
            Self::Generic(t) => t.media_type(),
        }
    }

    /// Check if this is a WebRTC track
    #[must_use]
    pub fn is_webrtc(&self) -> bool {
        matches!(self, Self::WebRtc(_))
    }

    /// Check if this is a generic (QUIC) track
    #[must_use]
    pub fn is_generic(&self) -> bool {
        matches!(self, Self::Generic(_))
    }
}

impl Default for MediaStreamManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Track Backend Tests
// ============================================================================

#[cfg(test)]
mod track_backend_tests {
    use super::*;

    // Compile-time verification that TrackBackend is object-safe
    fn _assert_object_safe(_: &dyn TrackBackend) {}

    // Compile-time verification that dyn TrackBackend is Send + Sync
    const fn _assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_track_backend_is_send_sync() {
        _assert_send_sync::<Box<dyn TrackBackend>>();
    }

    #[test]
    fn test_track_stats_default() {
        let stats = TrackStats::default();
        assert_eq!(stats.bytes_sent, 0);
        assert_eq!(stats.bytes_received, 0);
        assert_eq!(stats.packets_sent, 0);
        assert_eq!(stats.packets_received, 0);
    }

    #[test]
    fn test_track_stats_clone() {
        let stats = TrackStats {
            bytes_sent: 100,
            bytes_received: 200,
            packets_sent: 10,
            packets_received: 20,
        };
        let cloned = stats.clone();
        assert_eq!(cloned.bytes_sent, 100);
        assert_eq!(cloned.bytes_received, 200);
        assert_eq!(cloned.packets_sent, 10);
        assert_eq!(cloned.packets_received, 20);
    }

    #[test]
    fn test_track_stats_debug() {
        let stats = TrackStats::default();
        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains("TrackStats"));
        assert!(debug_str.contains("bytes_sent"));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod quic_track_backend_tests {
    use super::*;
    use crate::link_transport::PeerConnection;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn test_peer() -> PeerConnection {
        PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080),
        }
    }

    #[test]
    fn test_media_type_to_stream_type_mapping() {
        assert_eq!(
            QuicTrackBackend::media_type_to_stream_type(MediaType::Audio),
            StreamType::Audio
        );
        assert_eq!(
            QuicTrackBackend::media_type_to_stream_type(MediaType::Video),
            StreamType::Video
        );
        assert_eq!(
            QuicTrackBackend::media_type_to_stream_type(MediaType::ScreenShare),
            StreamType::Screen
        );
        assert_eq!(
            QuicTrackBackend::media_type_to_stream_type(MediaType::DataChannel),
            StreamType::Data
        );
    }

    #[test]
    fn test_quic_backend_creation() {
        let transport = Arc::new(QuicMediaTransport::new());
        let backend = QuicTrackBackend::new(transport, MediaType::Audio);

        assert_eq!(backend.stream_type(), StreamType::Audio);
        assert_eq!(backend.backend_type(), "quic");
    }

    #[test]
    fn test_quic_backend_with_explicit_stream_type() {
        let transport = Arc::new(QuicMediaTransport::new());
        let backend = QuicTrackBackend::with_stream_type(transport, StreamType::Video);

        assert_eq!(backend.stream_type(), StreamType::Video);
    }

    #[test]
    fn test_quic_backend_initial_stats() {
        let transport = Arc::new(QuicMediaTransport::new());
        let backend = QuicTrackBackend::new(transport, MediaType::Audio);

        let stats = backend.stats();
        assert_eq!(stats.bytes_sent, 0);
        assert_eq!(stats.bytes_received, 0);
        assert_eq!(stats.packets_sent, 0);
        assert_eq!(stats.packets_received, 0);
    }

    #[test]
    fn test_quic_backend_not_connected_initially() {
        let transport = Arc::new(QuicMediaTransport::new());
        let backend = QuicTrackBackend::new(transport, MediaType::Audio);

        assert!(!backend.is_connected());
    }

    #[tokio::test]
    async fn test_quic_backend_connected_after_transport_connect() {
        let transport = Arc::new(QuicMediaTransport::new());
        transport.connect(test_peer()).await.unwrap();

        let backend = QuicTrackBackend::new(transport, MediaType::Audio);
        assert!(backend.is_connected());
    }

    #[tokio::test]
    async fn test_quic_backend_send_when_connected() {
        let transport = Arc::new(QuicMediaTransport::new());
        transport.connect(test_peer()).await.unwrap();

        let backend = QuicTrackBackend::new(transport, MediaType::Audio);
        let data = &[0x80, 0x60, 0x00, 0x01];

        let result = backend.send(data).await;
        assert!(result.is_ok());

        // Verify stats were updated
        let stats = backend.stats();
        assert_eq!(stats.bytes_sent, 4);
        assert_eq!(stats.packets_sent, 1);
    }

    #[tokio::test]
    async fn test_quic_backend_send_when_disconnected() {
        let transport = Arc::new(QuicMediaTransport::new());
        let backend = QuicTrackBackend::new(transport, MediaType::Audio);
        let data = &[0x80, 0x60, 0x00, 0x01];

        let result = backend.send(data).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_quic_backend_stats_tracking() {
        let transport = Arc::new(QuicMediaTransport::new());
        transport.connect(test_peer()).await.unwrap();

        let backend = QuicTrackBackend::new(transport, MediaType::Video);

        // Send multiple packets
        backend.send(&[0x80, 0x60]).await.unwrap();
        backend.send(&[0x80, 0x61, 0xAA, 0xBB]).await.unwrap();

        let stats = backend.stats();
        assert_eq!(stats.bytes_sent, 6); // 2 + 4
        assert_eq!(stats.packets_sent, 2);
    }

    #[test]
    fn test_quic_backend_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<QuicTrackBackend>();
    }

    #[test]
    fn test_quic_backend_transport_accessor() {
        let transport = Arc::new(QuicMediaTransport::new());
        let transport_clone = Arc::clone(&transport);
        let backend = QuicTrackBackend::new(transport, MediaType::Audio);

        // Verify we can access the transport
        assert!(Arc::ptr_eq(backend.transport(), &transport_clone));
    }

    // =========================================================================
    // Stream Lifecycle Tests
    // =========================================================================

    #[tokio::test]
    async fn test_quic_backend_stream_initially_not_open() {
        let transport = Arc::new(QuicMediaTransport::new());
        let backend = QuicTrackBackend::new(transport, MediaType::Audio);

        // Stream is not open until ensure_stream is called
        assert!(!backend.is_stream_open().await);
    }

    #[tokio::test]
    async fn test_quic_backend_ensure_stream_when_not_connected() {
        let transport = Arc::new(QuicMediaTransport::new());
        let backend = QuicTrackBackend::new(transport, MediaType::Audio);

        let result = backend.ensure_stream().await;
        assert!(result.is_err());
        assert!(matches!(result, Err(MediaError::NotConnected)));
    }

    #[tokio::test]
    async fn test_quic_backend_ensure_stream_when_connected() {
        let transport = Arc::new(QuicMediaTransport::new());
        transport.connect(test_peer()).await.unwrap();

        let backend = QuicTrackBackend::new(transport, MediaType::Audio);

        // First ensure should succeed
        let result = backend.ensure_stream().await;
        assert!(result.is_ok());

        // Stream should now be open
        assert!(backend.is_stream_open().await);
    }

    #[tokio::test]
    async fn test_quic_backend_close_stream() {
        let transport = Arc::new(QuicMediaTransport::new());
        transport.connect(test_peer()).await.unwrap();

        let backend = QuicTrackBackend::new(transport, MediaType::Video);

        // Open the stream
        backend.ensure_stream().await.unwrap();
        assert!(backend.is_stream_open().await);

        // Close the stream
        let closed = backend.close_stream().await;
        assert!(closed);
        assert!(!backend.is_stream_open().await);
    }

    #[tokio::test]
    async fn test_quic_backend_close_stream_when_not_open() {
        let transport = Arc::new(QuicMediaTransport::new());
        transport.connect(test_peer()).await.unwrap();

        let backend = QuicTrackBackend::new(transport, MediaType::Audio);

        // Stream is not open
        let closed = backend.close_stream().await;
        assert!(!closed); // Nothing to close
    }

    #[tokio::test]
    async fn test_quic_backend_stream_lifecycle() {
        let transport = Arc::new(QuicMediaTransport::new());
        transport.connect(test_peer()).await.unwrap();

        let backend = QuicTrackBackend::new(transport, MediaType::Video);

        // Open stream
        backend.ensure_stream().await.unwrap();
        assert!(backend.is_stream_open().await);

        // Send data
        let data = &[0x80, 0x60, 0x00, 0x01];
        let send_result = backend.send(data).await;
        assert!(send_result.is_ok());

        // Close stream
        let closed = backend.close_stream().await;
        assert!(closed);

        // Reopen stream
        backend.ensure_stream().await.unwrap();
        assert!(backend.is_stream_open().await);

        // Send again
        let send_result2 = backend.send(data).await;
        assert!(send_result2.is_ok());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[allow(deprecated)]
mod legacy_webrtc_backend_tests {
    use super::*;

    fn create_test_track() -> Arc<TrackLocalStaticSample> {
        let codec_capability = RTCRtpCodecCapability {
            mime_type: "audio/opus".to_string(),
            clock_rate: 48000,
            channels: 2,
            sdp_fmtp_line: "".to_string(),
            rtcp_feedback: vec![],
        };
        Arc::new(TrackLocalStaticSample::new(
            codec_capability,
            "test-audio".to_string(),
            "audio".to_string(),
        ))
    }

    #[test]
    fn test_legacy_backend_creation() {
        let track = create_test_track();
        let backend = LegacyWebRtcBackend::new(track, MediaType::Audio);

        assert_eq!(backend.backend_type(), "webrtc");
        assert!(backend.is_connected());
    }

    #[test]
    fn test_legacy_backend_track_type() {
        let track = create_test_track();
        let backend = LegacyWebRtcBackend::new(track, MediaType::Audio);

        assert_eq!(backend.track_type(), MediaType::Audio);
    }

    #[test]
    fn test_legacy_backend_initial_stats() {
        let track = create_test_track();
        let backend = LegacyWebRtcBackend::new(track, MediaType::Audio);

        let stats = backend.stats();
        assert_eq!(stats.bytes_sent, 0);
        assert_eq!(stats.bytes_received, 0);
        assert_eq!(stats.packets_sent, 0);
        assert_eq!(stats.packets_received, 0);
    }

    #[test]
    fn test_legacy_backend_webrtc_track_accessor() {
        let track = create_test_track();
        let track_clone = Arc::clone(&track);
        let backend = LegacyWebRtcBackend::new(track, MediaType::Audio);

        assert!(Arc::ptr_eq(backend.webrtc_track(), &track_clone));
    }

    #[tokio::test]
    async fn test_legacy_backend_recv_not_supported() {
        let track = create_test_track();
        let backend = LegacyWebRtcBackend::new(track, MediaType::Audio);

        let result = backend.recv().await;
        assert!(matches!(result, Err(MediaError::ReceiveNotSupported)));
    }

    #[test]
    fn test_legacy_backend_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<LegacyWebRtcBackend>();
    }

    #[test]
    fn test_legacy_backend_different_media_types() {
        let track = create_test_track();
        let audio_backend = LegacyWebRtcBackend::new(Arc::clone(&track), MediaType::Audio);
        let video_backend = LegacyWebRtcBackend::new(Arc::clone(&track), MediaType::Video);

        assert_eq!(audio_backend.track_type(), MediaType::Audio);
        assert_eq!(video_backend.track_type(), MediaType::Video);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod video_track_tests {
    use super::*;
    use crate::link_transport::PeerConnection;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn test_peer() -> PeerConnection {
        PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080),
        }
    }

    fn create_webrtc_video_track() -> Arc<TrackLocalStaticSample> {
        let codec_capability = RTCRtpCodecCapability {
            mime_type: "video/H264".to_string(),
            clock_rate: 90000,
            channels: 0,
            sdp_fmtp_line: "".to_string(),
            rtcp_feedback: vec![],
        };
        Arc::new(TrackLocalStaticSample::new(
            codec_capability,
            "test-video".to_string(),
            "video".to_string(),
        ))
    }

    #[test]
    fn test_video_track_with_quic_backend() {
        let transport = Arc::new(QuicMediaTransport::new());
        let track = VideoTrack::with_quic("video-1", transport, 1280, 720);

        assert_eq!(track.id, "video-1");
        assert_eq!(track.width, 1280);
        assert_eq!(track.height, 720);
        assert_eq!(track.backend().backend_type(), "quic");
    }

    #[test]
    #[allow(deprecated)]
    fn test_video_track_with_webrtc_backend() {
        let webrtc_track = create_webrtc_video_track();
        let track = VideoTrack::with_webrtc("video-2", webrtc_track, 640, 480);

        assert_eq!(track.id, "video-2");
        assert_eq!(track.width, 640);
        assert_eq!(track.height, 480);
        assert_eq!(track.backend().backend_type(), "webrtc");
    }

    #[test]
    fn test_video_track_new_with_backend() {
        let transport = Arc::new(QuicMediaTransport::new());
        let backend = Arc::new(QuicTrackBackend::new(transport, MediaType::Video));
        let track = VideoTrack::new_with_backend("video-3".to_string(), backend, 1920, 1080);

        assert_eq!(track.id, "video-3");
        assert_eq!(track.width, 1920);
        assert_eq!(track.height, 1080);
    }

    #[test]
    fn test_video_track_initial_stats() {
        let transport = Arc::new(QuicMediaTransport::new());
        let track = VideoTrack::with_quic("video-1", transport, 1280, 720);

        let stats = track.stats();
        assert_eq!(stats.bytes_sent, 0);
        assert_eq!(stats.packets_sent, 0);
    }

    #[test]
    fn test_video_track_not_connected_initially() {
        let transport = Arc::new(QuicMediaTransport::new());
        let track = VideoTrack::with_quic("video-1", transport, 1280, 720);

        assert!(!track.is_connected());
    }

    #[tokio::test]
    async fn test_video_track_connected_after_transport_connect() {
        let transport = Arc::new(QuicMediaTransport::new());
        transport.connect(test_peer()).await.unwrap();

        let track = VideoTrack::with_quic("video-1", transport, 1280, 720);
        assert!(track.is_connected());
    }

    #[tokio::test]
    async fn test_video_track_send_frame() {
        let transport = Arc::new(QuicMediaTransport::new());
        transport.connect(test_peer()).await.unwrap();

        let track = VideoTrack::with_quic("video-1", transport, 1280, 720);
        let frame_data = &[0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x0a];

        let result = track.send_frame(frame_data).await;
        assert!(result.is_ok());

        // Verify stats updated
        let stats = track.stats();
        assert_eq!(stats.bytes_sent, 8);
        assert_eq!(stats.packets_sent, 1);
    }

    #[test]
    fn test_video_track_encode_decode_without_codecs() {
        let transport = Arc::new(QuicMediaTransport::new());
        let mut track = VideoTrack::with_quic("video-1", transport, 1280, 720);

        // Without encoder, encode_frame returns the raw data
        let raw_data = vec![0x01, 0x02, 0x03];
        let encoded = track.encode_frame(&raw_data).unwrap();
        assert_eq!(encoded, raw_data);

        // Without decoder, decode_frame returns the raw data
        let decoded = track.decode_frame(&raw_data).unwrap();
        assert_eq!(decoded, raw_data);
    }

    #[test]
    fn test_video_track_backend_accessor() {
        let transport = Arc::new(QuicMediaTransport::new());
        let track = VideoTrack::with_quic("video-1", transport, 1280, 720);

        let backend = track.backend();
        assert_eq!(backend.backend_type(), "quic");
    }

    #[test]
    #[allow(deprecated)]
    fn test_legacy_constructor_still_works() {
        let webrtc_track = create_webrtc_video_track();
        let track = VideoTrack::new("legacy-video".to_string(), webrtc_track, 320, 240);

        assert_eq!(track.id, "legacy-video");
        assert_eq!(track.width, 320);
        assert_eq!(track.height, 240);
        assert_eq!(track.backend().backend_type(), "webrtc");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod audio_track_tests {
    use super::*;
    use crate::link_transport::PeerConnection;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn test_peer() -> PeerConnection {
        PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080),
        }
    }

    fn create_webrtc_audio_track() -> Arc<TrackLocalStaticSample> {
        let codec_capability = RTCRtpCodecCapability {
            mime_type: "audio/opus".to_string(),
            clock_rate: 48000,
            channels: 2,
            sdp_fmtp_line: "".to_string(),
            rtcp_feedback: vec![],
        };
        Arc::new(TrackLocalStaticSample::new(
            codec_capability,
            "test-audio".to_string(),
            "audio".to_string(),
        ))
    }

    #[test]
    fn test_audio_track_with_quic_backend() {
        let transport = Arc::new(QuicMediaTransport::new());
        let track = AudioTrack::with_quic("audio-1", transport);

        assert_eq!(track.id, "audio-1");
        assert_eq!(track.backend().backend_type(), "quic");
    }

    #[test]
    #[allow(deprecated)]
    fn test_audio_track_with_webrtc_backend() {
        let webrtc_track = create_webrtc_audio_track();
        let track = AudioTrack::with_webrtc("audio-2", webrtc_track);

        assert_eq!(track.id, "audio-2");
        assert_eq!(track.backend().backend_type(), "webrtc");
    }

    #[test]
    fn test_audio_track_new_with_backend() {
        let transport = Arc::new(QuicMediaTransport::new());
        let backend = Arc::new(QuicTrackBackend::new(transport, MediaType::Audio));
        let track = AudioTrack::new_with_backend("audio-3".to_string(), backend);

        assert_eq!(track.id, "audio-3");
    }

    #[test]
    fn test_audio_track_initial_stats() {
        let transport = Arc::new(QuicMediaTransport::new());
        let track = AudioTrack::with_quic("audio-1", transport);

        let stats = track.stats();
        assert_eq!(stats.bytes_sent, 0);
        assert_eq!(stats.packets_sent, 0);
    }

    #[test]
    fn test_audio_track_not_connected_initially() {
        let transport = Arc::new(QuicMediaTransport::new());
        let track = AudioTrack::with_quic("audio-1", transport);

        assert!(!track.is_connected());
    }

    #[tokio::test]
    async fn test_audio_track_connected_after_transport_connect() {
        let transport = Arc::new(QuicMediaTransport::new());
        transport.connect(test_peer()).await.unwrap();

        let track = AudioTrack::with_quic("audio-1", transport);
        assert!(track.is_connected());
    }

    #[tokio::test]
    async fn test_audio_track_send_audio() {
        let transport = Arc::new(QuicMediaTransport::new());
        transport.connect(test_peer()).await.unwrap();

        let track = AudioTrack::with_quic("audio-1", transport);
        // Simulate Opus audio packet
        let audio_data = &[0x78, 0x9c, 0x12, 0x34];

        let result = track.send_audio(audio_data).await;
        assert!(result.is_ok());

        // Verify stats updated
        let stats = track.stats();
        assert_eq!(stats.bytes_sent, 4);
        assert_eq!(stats.packets_sent, 1);
    }

    #[tokio::test]
    async fn test_audio_track_send_when_disconnected() {
        let transport = Arc::new(QuicMediaTransport::new());
        let track = AudioTrack::with_quic("audio-1", transport);

        let result = track.send_audio(&[0x01, 0x02]).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_audio_track_backend_accessor() {
        let transport = Arc::new(QuicMediaTransport::new());
        let track = AudioTrack::with_quic("audio-1", transport);

        let backend = track.backend();
        assert_eq!(backend.backend_type(), "quic");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod generic_track_tests {
    use super::*;
    use crate::link_transport::PeerConnection;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn test_peer() -> PeerConnection {
        PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080),
        }
    }

    #[test]
    fn test_generic_track_audio() {
        let transport = Arc::new(QuicMediaTransport::new());
        let audio = AudioTrack::with_quic("audio-1", transport);
        let generic = GenericTrack::audio(audio);

        assert_eq!(generic.id(), "audio-1");
        assert_eq!(generic.media_type(), MediaType::Audio);
        assert!(generic.is_audio());
        assert!(!generic.is_video());
        assert!(!generic.is_screen());
    }

    #[test]
    fn test_generic_track_video() {
        let transport = Arc::new(QuicMediaTransport::new());
        let video = VideoTrack::with_quic("video-1", transport, 1280, 720);
        let generic = GenericTrack::video(video);

        assert_eq!(generic.id(), "video-1");
        assert_eq!(generic.media_type(), MediaType::Video);
        assert!(!generic.is_audio());
        assert!(generic.is_video());
        assert!(!generic.is_screen());
    }

    #[test]
    fn test_generic_track_screen() {
        let transport = Arc::new(QuicMediaTransport::new());
        let screen = VideoTrack::with_quic("screen-1", transport, 1920, 1080);
        let generic = GenericTrack::screen(screen);

        assert_eq!(generic.id(), "screen-1");
        assert_eq!(generic.media_type(), MediaType::ScreenShare);
        assert!(!generic.is_audio());
        assert!(!generic.is_video());
        assert!(generic.is_screen());
    }

    #[test]
    fn test_generic_track_as_audio() {
        let transport = Arc::new(QuicMediaTransport::new());
        let audio = AudioTrack::with_quic("audio-1", transport);
        let generic = GenericTrack::audio(audio);

        assert!(generic.as_audio().is_some());
        assert!(generic.as_video().is_none());
        assert!(generic.as_screen().is_none());
    }

    #[test]
    fn test_generic_track_as_video() {
        let transport = Arc::new(QuicMediaTransport::new());
        let video = VideoTrack::with_quic("video-1", transport, 1280, 720);
        let generic = GenericTrack::video(video);

        assert!(generic.as_audio().is_none());
        assert!(generic.as_video().is_some());
        assert!(generic.as_screen().is_none());
    }

    #[test]
    fn test_generic_track_backend_accessor() {
        let transport = Arc::new(QuicMediaTransport::new());
        let audio = AudioTrack::with_quic("audio-1", transport);
        let generic = GenericTrack::audio(audio);

        assert_eq!(generic.backend().backend_type(), "quic");
    }

    #[test]
    fn test_generic_track_connected_status() {
        let transport = Arc::new(QuicMediaTransport::new());
        let audio = AudioTrack::with_quic("audio-1", transport);
        let generic = GenericTrack::audio(audio);

        assert!(!generic.is_connected());
    }

    #[tokio::test]
    async fn test_generic_track_connected_after_transport_connect() {
        let transport = Arc::new(QuicMediaTransport::new());
        transport.connect(test_peer()).await.unwrap();

        let audio = AudioTrack::with_quic("audio-1", transport);
        let generic = GenericTrack::audio(audio);

        assert!(generic.is_connected());
    }

    #[test]
    fn test_generic_track_initial_stats() {
        let transport = Arc::new(QuicMediaTransport::new());
        let video = VideoTrack::with_quic("video-1", transport, 1280, 720);
        let generic = GenericTrack::video(video);

        let stats = generic.stats();
        assert_eq!(stats.bytes_sent, 0);
        assert_eq!(stats.packets_sent, 0);
    }

    #[tokio::test]
    async fn test_generic_track_send() {
        let transport = Arc::new(QuicMediaTransport::new());
        transport.connect(test_peer()).await.unwrap();

        let audio = AudioTrack::with_quic("audio-1", transport);
        let generic = GenericTrack::audio(audio);

        let result = generic.send(&[0x01, 0x02, 0x03]).await;
        assert!(result.is_ok());

        let stats = generic.stats();
        assert_eq!(stats.bytes_sent, 3);
    }

    #[test]
    fn test_generic_track_collection() {
        let transport = Arc::new(QuicMediaTransport::new());
        let audio = GenericTrack::audio(AudioTrack::with_quic("audio-1", Arc::clone(&transport)));
        let video = GenericTrack::video(VideoTrack::with_quic(
            "video-1",
            Arc::clone(&transport),
            1280,
            720,
        ));
        let screen = GenericTrack::screen(VideoTrack::with_quic("screen-1", transport, 1920, 1080));

        let tracks = [audio, video, screen];

        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[0].media_type(), MediaType::Audio);
        assert_eq!(tracks[1].media_type(), MediaType::Video);
        assert_eq!(tracks[2].media_type(), MediaType::ScreenShare);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_media_stream_manager_initialize() {
        let manager = MediaStreamManager::new();

        let result = manager.initialize().await;
        assert!(result.is_ok());

        // Check that events were sent
        let _events = manager.subscribe_events();
        // Note: In a real test, we'd need to handle the async nature,
        // but for now this is a basic structure test
    }

    #[tokio::test]
    async fn test_media_stream_manager_get_devices() {
        let manager = MediaStreamManager::new();

        let audio_devices = manager.get_audio_devices();
        assert!(audio_devices.is_empty());

        let video_devices = manager.get_video_devices();
        assert!(video_devices.is_empty());
    }

    #[tokio::test]
    async fn test_media_stream_manager_create_audio_track() {
        let mut manager = MediaStreamManager::new();

        let track = manager.create_audio_track().await.unwrap();
        assert_eq!(track.track_type, MediaType::Audio);
        assert!(track.id.starts_with("audio-"));

        let tracks = manager.get_webrtc_tracks();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].track_type, MediaType::Audio);
    }

    #[tokio::test]
    async fn test_media_stream_manager_create_video_track() {
        let mut manager = MediaStreamManager::new();

        let track = manager.create_video_track().await.unwrap();
        assert_eq!(track.track_type, MediaType::Video);
        assert!(track.id.starts_with("video-"));

        let tracks = manager.get_webrtc_tracks();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].track_type, MediaType::Video);
    }

    #[tokio::test]
    async fn test_media_stream_manager_create_video_track_with_codec() {
        let mut manager = MediaStreamManager::new();

        let track = manager
            .create_video_track_with_codec(VideoCodec::H264, 640, 480)
            .await
            .unwrap();

        assert!(track.id.starts_with("video-"));
        assert_eq!(track.width, 640);
        assert_eq!(track.height, 480);
        assert!(track.encoder.is_some()); // Should have H.264 encoder
    }

    #[tokio::test]
    async fn test_media_stream_manager_multiple_tracks() {
        let mut manager = MediaStreamManager::new();

        manager.create_audio_track().await.unwrap();
        manager.create_video_track().await.unwrap();

        let tracks = manager.get_webrtc_tracks();
        assert_eq!(tracks.len(), 2);

        // Check track IDs are different
        assert_ne!(tracks[0].id, tracks[1].id);

        // Check that we have one audio and one video track
        let audio_count = tracks
            .iter()
            .filter(|t| t.track_type == MediaType::Audio)
            .count();
        let video_count = tracks
            .iter()
            .filter(|t| t.track_type == MediaType::Video)
            .count();

        assert_eq!(audio_count, 1);
        assert_eq!(video_count, 1);
    }

    // =========================================================================
    // QUIC MediaStreamManager Tests
    // =========================================================================

    #[test]
    fn test_media_stream_manager_with_quic_transport() {
        let transport = Arc::new(QuicMediaTransport::new());
        let manager = MediaStreamManager::with_quic_transport(transport);

        assert!(manager.has_quic_transport());
        assert!(manager.quic_transport().is_some());
    }

    #[test]
    fn test_media_stream_manager_set_quic_transport() {
        let mut manager = MediaStreamManager::new();
        assert!(!manager.has_quic_transport());

        let transport = Arc::new(QuicMediaTransport::new());
        manager.set_quic_transport(transport);

        assert!(manager.has_quic_transport());
    }

    #[test]
    fn test_create_quic_audio_track_without_transport() {
        let mut manager = MediaStreamManager::new();
        let result = manager.create_quic_audio_track();

        assert!(result.is_err());
        let err = result.unwrap_err();
        if let MediaError::ConfigError(msg) = err {
            assert!(msg.contains("QUIC transport not configured"));
        } else {
            unreachable!("Expected ConfigError, got {:?}", err);
        }
    }

    #[test]
    fn test_create_quic_audio_track() {
        let transport = Arc::new(QuicMediaTransport::new());
        let mut manager = MediaStreamManager::with_quic_transport(transport);

        let result = manager.create_quic_audio_track();
        assert!(result.is_ok());

        let track = result.unwrap();
        assert!(track.is_audio());
        assert_eq!(track.media_type(), MediaType::Audio);
        assert!(track.id().starts_with("audio-"));
    }

    #[test]
    fn test_create_quic_video_track() {
        let transport = Arc::new(QuicMediaTransport::new());
        let mut manager = MediaStreamManager::with_quic_transport(transport);

        let result = manager.create_quic_video_track(1280, 720);
        assert!(result.is_ok());

        let track = result.unwrap();
        assert!(track.is_video());
        assert_eq!(track.media_type(), MediaType::Video);
        assert!(track.id().starts_with("video-"));
    }

    #[test]
    fn test_create_quic_screen_track() {
        let transport = Arc::new(QuicMediaTransport::new());
        let mut manager = MediaStreamManager::with_quic_transport(transport);

        let result = manager.create_quic_screen_track(1920, 1080);
        assert!(result.is_ok());

        let track = result.unwrap();
        assert!(track.is_screen());
        assert_eq!(track.media_type(), MediaType::ScreenShare);
        assert!(track.id().starts_with("screen-"));
    }

    #[test]
    fn test_get_tracks() {
        let transport = Arc::new(QuicMediaTransport::new());
        let mut manager = MediaStreamManager::with_quic_transport(transport);

        manager.create_quic_audio_track().unwrap();
        manager.create_quic_video_track(1280, 720).unwrap();

        let tracks = manager.get_tracks();
        assert_eq!(tracks.len(), 2);
    }

    #[test]
    fn test_get_track_by_id_quic() {
        let transport = Arc::new(QuicMediaTransport::new());
        let mut manager = MediaStreamManager::with_quic_transport(transport);

        manager.create_quic_audio_track().unwrap();
        let track_id = manager.get_tracks()[0].id().to_string();

        let found = manager.get_track_by_id(&track_id);
        assert!(found.is_some());
        assert!(found.unwrap().is_generic());
    }

    #[tokio::test]
    async fn test_get_track_by_id_webrtc() {
        let mut manager = MediaStreamManager::new();
        manager.create_audio_track().await.unwrap();
        let track_id = manager.get_webrtc_tracks()[0].id.clone();

        let found = manager.get_track_by_id(&track_id);
        assert!(found.is_some());
        assert!(found.unwrap().is_webrtc());
    }

    #[test]
    fn test_remove_quic_track() {
        let transport = Arc::new(QuicMediaTransport::new());
        let mut manager = MediaStreamManager::with_quic_transport(transport);

        manager.create_quic_audio_track().unwrap();
        let track_id = manager.get_tracks()[0].id().to_string();

        assert_eq!(manager.get_tracks().len(), 1);
        let removed = manager.remove_track(&track_id);
        assert!(removed);
        assert_eq!(manager.get_tracks().len(), 0);
    }

    #[test]
    fn test_mixed_track_creation() {
        let transport = Arc::new(QuicMediaTransport::new());
        let mut manager = MediaStreamManager::with_quic_transport(transport);

        // Create QUIC tracks
        manager.create_quic_audio_track().unwrap();
        manager.create_quic_video_track(1280, 720).unwrap();

        // Should have 2 generic tracks
        assert_eq!(manager.get_tracks().len(), 2);

        // Still no WebRTC tracks
        assert_eq!(manager.get_webrtc_tracks().len(), 0);
    }
}
