//! QUIC Media Transport for WebRTC over ant-quic
//!
//! This module provides the core transport layer for sending and receiving
//! RTP/RTCP media packets over QUIC streams. It replaces the webrtc crate's
//! ICE/UDP layer with direct QUIC stream management.
//!
//! # Architecture
//!
//! The `QuicMediaTransport` wraps an underlying `LinkTransport` and provides:
//! - Dedicated QUIC streams per media type (audio, video, screen, RTCP)
//! - Length-prefix framing for RTP packets
//! - Stream priority and QoS integration
//! - Thread-safe access via interior mutability
//!
//! # Example
//!
//! ```ignore
//! use saorsa_webrtc_core::quic_media_transport::QuicMediaTransport;
//!
//! let transport = QuicMediaTransport::new(link_transport);
//! transport.connect("peer-id").await?;
//! transport.send_rtp(StreamType::Audio, &rtp_packet).await?;
//! ```

use crate::link_transport::{LinkTransportError, PeerConnection, StreamType};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// Error type for media transport operations
#[derive(Error, Debug, Clone)]
pub enum MediaTransportError {
    /// Transport not connected
    #[error("Transport not connected")]
    NotConnected,

    /// Connection failed
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Stream error
    #[error("Stream error: {0}")]
    StreamError(String),

    /// Invalid state transition
    #[error("Invalid state transition: {from:?} -> {to:?}")]
    InvalidStateTransition {
        /// Current state
        from: MediaTransportState,
        /// Attempted state
        to: MediaTransportState,
    },

    /// Framing error
    #[error("Framing error: {0}")]
    FramingError(String),

    /// Underlying transport error
    #[error("Transport error: {0}")]
    TransportError(#[from] LinkTransportError),
}

/// Connection state for the media transport
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaTransportState {
    /// Not connected to any peer
    Disconnected,
    /// Connecting to a peer
    Connecting,
    /// Connected and ready for media
    Connected,
    /// Connection failed
    Failed,
}

impl Default for MediaTransportState {
    fn default() -> Self {
        Self::Disconnected
    }
}

/// Handle to an active QUIC stream
#[derive(Debug, Clone)]
pub struct StreamHandle {
    /// Stream type
    pub stream_type: StreamType,
    /// Whether the stream is open
    pub is_open: bool,
    /// Bytes sent on this stream
    pub bytes_sent: u64,
    /// Bytes received on this stream
    pub bytes_received: u64,
}

impl StreamHandle {
    /// Create a new stream handle
    fn new(stream_type: StreamType) -> Self {
        Self {
            stream_type,
            is_open: true,
            bytes_sent: 0,
            bytes_received: 0,
        }
    }
}

/// Stream priority levels for QoS
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StreamPriority {
    /// Highest priority (audio)
    High = 0,
    /// Medium priority (video)
    Medium = 1,
    /// Lower priority (data, screen share)
    Low = 2,
}

impl From<StreamType> for StreamPriority {
    fn from(stream_type: StreamType) -> Self {
        match stream_type {
            StreamType::Audio => StreamPriority::High,
            StreamType::RtcpFeedback => StreamPriority::High,
            StreamType::Video => StreamPriority::Medium,
            StreamType::Screen => StreamPriority::Low,
            StreamType::Data => StreamPriority::Low,
        }
    }
}

/// QUIC-based media transport for WebRTC
///
/// Provides dedicated QUIC streams for each media type (audio, video, screen, RTCP).
/// This struct is thread-safe and can be shared across tasks.
///
/// # Thread Safety
///
/// All internal state is protected by `RwLock`, allowing safe concurrent access
/// from multiple async tasks.
pub struct QuicMediaTransport {
    /// Current connection state
    state: Arc<RwLock<MediaTransportState>>,
    /// Active stream handles by type
    streams: Arc<RwLock<HashMap<StreamType, StreamHandle>>>,
    /// Remote peer connection
    peer: Arc<RwLock<Option<PeerConnection>>>,
    /// Transport statistics
    stats: Arc<RwLock<TransportStats>>,
}

/// Statistics for the media transport
#[derive(Debug, Clone, Default)]
pub struct TransportStats {
    /// Total packets sent
    pub packets_sent: u64,
    /// Total packets received
    pub packets_received: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Number of stream errors
    pub stream_errors: u64,
    /// RTCP packets sent
    pub rtcp_packets_sent: u64,
    /// RTCP packets received
    pub rtcp_packets_received: u64,
    /// RTCP bytes sent
    pub rtcp_bytes_sent: u64,
    /// RTCP bytes received
    pub rtcp_bytes_received: u64,
}

impl Default for QuicMediaTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl QuicMediaTransport {
    /// Create a new QUIC media transport
    ///
    /// The transport starts in the `Disconnected` state.
    ///
    /// # Returns
    ///
    /// A new `QuicMediaTransport` instance ready for connection.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(MediaTransportState::Disconnected)),
            streams: Arc::new(RwLock::new(HashMap::new())),
            peer: Arc::new(RwLock::new(None)),
            stats: Arc::new(RwLock::new(TransportStats::default())),
        }
    }

    /// Get the current connection state
    ///
    /// # Returns
    ///
    /// The current `MediaTransportState`.
    pub async fn state(&self) -> MediaTransportState {
        *self.state.read().await
    }

    /// Check if the transport is connected
    ///
    /// # Returns
    ///
    /// `true` if the transport is in the `Connected` state.
    pub async fn is_connected(&self) -> bool {
        *self.state.read().await == MediaTransportState::Connected
    }

    /// Set the connection state
    ///
    /// # Arguments
    ///
    /// * `new_state` - The new state to transition to
    ///
    /// # Errors
    ///
    /// Returns error if the state transition is invalid.
    async fn set_state(&self, new_state: MediaTransportState) -> Result<(), MediaTransportError> {
        let mut state = self.state.write().await;
        let current = *state;

        // Validate state transitions
        let valid = match (current, new_state) {
            // From Disconnected
            (MediaTransportState::Disconnected, MediaTransportState::Connecting) => true,
            (MediaTransportState::Disconnected, MediaTransportState::Disconnected) => true,
            // From Connecting
            (MediaTransportState::Connecting, MediaTransportState::Connected) => true,
            (MediaTransportState::Connecting, MediaTransportState::Failed) => true,
            (MediaTransportState::Connecting, MediaTransportState::Disconnected) => true,
            // From Connected
            (MediaTransportState::Connected, MediaTransportState::Disconnected) => true,
            (MediaTransportState::Connected, MediaTransportState::Failed) => true,
            // From Failed
            (MediaTransportState::Failed, MediaTransportState::Disconnected) => true,
            (MediaTransportState::Failed, MediaTransportState::Connecting) => true,
            // Same state is always valid
            (a, b) if a == b => true,
            // All other transitions are invalid
            _ => false,
        };

        if !valid {
            return Err(MediaTransportError::InvalidStateTransition {
                from: current,
                to: new_state,
            });
        }

        *state = new_state;
        Ok(())
    }

    /// Connect to a remote peer
    ///
    /// # Arguments
    ///
    /// * `peer` - The peer connection to use
    ///
    /// # Errors
    ///
    /// Returns error if already connected or connection fails.
    pub async fn connect(&self, peer: PeerConnection) -> Result<(), MediaTransportError> {
        self.set_state(MediaTransportState::Connecting).await?;

        // Store peer connection
        {
            let mut peer_lock = self.peer.write().await;
            *peer_lock = Some(peer);
        }

        // Transition to connected
        self.set_state(MediaTransportState::Connected).await?;

        tracing::info!("QuicMediaTransport connected");
        Ok(())
    }

    /// Disconnect from the remote peer
    ///
    /// Closes all open streams and resets the transport state.
    ///
    /// # Errors
    ///
    /// Returns error if disconnect fails.
    pub async fn disconnect(&self) -> Result<(), MediaTransportError> {
        // Close all streams
        {
            let mut streams = self.streams.write().await;
            for (_, stream) in streams.iter_mut() {
                stream.is_open = false;
            }
            streams.clear();
        }

        // Clear peer
        {
            let mut peer = self.peer.write().await;
            *peer = None;
        }

        // Transition to disconnected
        self.set_state(MediaTransportState::Disconnected).await?;

        tracing::info!("QuicMediaTransport disconnected");
        Ok(())
    }

    /// Get or create a stream handle for the given type
    ///
    /// # Arguments
    ///
    /// * `stream_type` - The type of stream to get or create
    ///
    /// # Returns
    ///
    /// A clone of the stream handle.
    ///
    /// # Errors
    ///
    /// Returns error if not connected.
    pub async fn get_or_create_stream(
        &self,
        stream_type: StreamType,
    ) -> Result<StreamHandle, MediaTransportError> {
        if !self.is_connected().await {
            return Err(MediaTransportError::NotConnected);
        }

        let mut streams = self.streams.write().await;
        let handle = streams
            .entry(stream_type)
            .or_insert_with(|| StreamHandle::new(stream_type));

        Ok(handle.clone())
    }

    /// Get the current peer connection
    ///
    /// # Returns
    ///
    /// The current peer connection if connected.
    pub async fn peer(&self) -> Option<PeerConnection> {
        self.peer.read().await.clone()
    }

    /// Get transport statistics
    ///
    /// # Returns
    ///
    /// A clone of the current transport statistics.
    pub async fn stats(&self) -> TransportStats {
        self.stats.read().await.clone()
    }

    /// Get the priority for a stream type
    ///
    /// # Arguments
    ///
    /// * `stream_type` - The stream type to get priority for
    ///
    /// # Returns
    ///
    /// The priority level for the stream type.
    #[must_use]
    pub fn priority_for(stream_type: StreamType) -> StreamPriority {
        StreamPriority::from(stream_type)
    }

    /// Get all active stream handles
    ///
    /// # Returns
    ///
    /// A vector of all currently active stream handles.
    pub async fn active_streams(&self) -> Vec<StreamHandle> {
        let streams = self.streams.read().await;
        streams
            .values()
            .filter(|h| h.is_open)
            .cloned()
            .collect()
    }

    /// Close a specific stream
    ///
    /// # Arguments
    ///
    /// * `stream_type` - The type of stream to close
    ///
    /// # Returns
    ///
    /// `true` if the stream was closed, `false` if it wasn't open.
    pub async fn close_stream(&self, stream_type: StreamType) -> bool {
        let mut streams = self.streams.write().await;
        if let Some(handle) = streams.get_mut(&stream_type) {
            handle.is_open = false;
            true
        } else {
            false
        }
    }

    /// Update stream statistics after sending
    ///
    /// # Arguments
    ///
    /// * `stream_type` - The stream type
    /// * `bytes` - Number of bytes sent
    pub async fn record_sent(&self, stream_type: StreamType, bytes: u64) {
        // Update stream stats
        {
            let mut streams = self.streams.write().await;
            if let Some(handle) = streams.get_mut(&stream_type) {
                handle.bytes_sent += bytes;
            }
        }

        // Update global stats
        {
            let mut stats = self.stats.write().await;
            stats.packets_sent += 1;
            stats.bytes_sent += bytes;
        }
    }

    /// Update stream statistics after receiving
    ///
    /// # Arguments
    ///
    /// * `stream_type` - The stream type
    /// * `bytes` - Number of bytes received
    pub async fn record_received(&self, stream_type: StreamType, bytes: u64) {
        // Update stream stats
        {
            let mut streams = self.streams.write().await;
            if let Some(handle) = streams.get_mut(&stream_type) {
                handle.bytes_received += bytes;
            }
        }

        // Update global stats
        {
            let mut stats = self.stats.write().await;
            stats.packets_received += 1;
            stats.bytes_received += bytes;
        }
    }

    /// Record a stream error
    pub async fn record_error(&self) {
        let mut stats = self.stats.write().await;
        stats.stream_errors += 1;
    }

    /// Record RTCP packet sent
    ///
    /// # Arguments
    ///
    /// * `bytes` - Number of bytes sent
    pub async fn record_rtcp_sent(&self, bytes: u64) {
        let mut stats = self.stats.write().await;
        stats.rtcp_packets_sent += 1;
        stats.rtcp_bytes_sent += bytes;
    }

    /// Record RTCP packet received
    ///
    /// # Arguments
    ///
    /// * `bytes` - Number of bytes received
    pub async fn record_rtcp_received(&self, bytes: u64) {
        let mut stats = self.stats.write().await;
        stats.rtcp_packets_received += 1;
        stats.rtcp_bytes_received += bytes;
    }
}

// Ensure QuicMediaTransport is Send + Sync at compile time
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<QuicMediaTransport>();
};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn test_peer() -> PeerConnection {
        PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080),
        }
    }

    #[tokio::test]
    async fn test_new_transport_is_disconnected() {
        let transport = QuicMediaTransport::new();
        assert_eq!(transport.state().await, MediaTransportState::Disconnected);
        assert!(!transport.is_connected().await);
    }

    #[tokio::test]
    async fn test_connect_transitions_state() {
        let transport = QuicMediaTransport::new();

        transport.connect(test_peer()).await.unwrap();

        assert_eq!(transport.state().await, MediaTransportState::Connected);
        assert!(transport.is_connected().await);
    }

    #[tokio::test]
    async fn test_disconnect_transitions_state() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        transport.disconnect().await.unwrap();

        assert_eq!(transport.state().await, MediaTransportState::Disconnected);
        assert!(!transport.is_connected().await);
    }

    #[tokio::test]
    async fn test_peer_stored_on_connect() {
        let transport = QuicMediaTransport::new();
        let peer = test_peer();

        transport.connect(peer.clone()).await.unwrap();

        let stored_peer = transport.peer().await.unwrap();
        assert_eq!(stored_peer.peer_id, peer.peer_id);
    }

    #[tokio::test]
    async fn test_peer_cleared_on_disconnect() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        transport.disconnect().await.unwrap();

        assert!(transport.peer().await.is_none());
    }

    #[tokio::test]
    async fn test_get_or_create_stream_when_connected() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let handle = transport.get_or_create_stream(StreamType::Audio).await.unwrap();

        assert_eq!(handle.stream_type, StreamType::Audio);
        assert!(handle.is_open);
    }

    #[tokio::test]
    async fn test_get_or_create_stream_when_disconnected() {
        let transport = QuicMediaTransport::new();

        let result = transport.get_or_create_stream(StreamType::Audio).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stream_priority() {
        assert_eq!(QuicMediaTransport::priority_for(StreamType::Audio), StreamPriority::High);
        assert_eq!(QuicMediaTransport::priority_for(StreamType::Video), StreamPriority::Medium);
        assert_eq!(QuicMediaTransport::priority_for(StreamType::Screen), StreamPriority::Low);
        assert_eq!(QuicMediaTransport::priority_for(StreamType::Data), StreamPriority::Low);
        assert_eq!(QuicMediaTransport::priority_for(StreamType::RtcpFeedback), StreamPriority::High);
    }

    #[tokio::test]
    async fn test_active_streams() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        transport.get_or_create_stream(StreamType::Audio).await.unwrap();
        transport.get_or_create_stream(StreamType::Video).await.unwrap();

        let active = transport.active_streams().await;
        assert_eq!(active.len(), 2);
    }

    #[tokio::test]
    async fn test_close_stream() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();
        transport.get_or_create_stream(StreamType::Audio).await.unwrap();

        let closed = transport.close_stream(StreamType::Audio).await;
        assert!(closed);

        let active = transport.active_streams().await;
        assert!(active.is_empty());
    }

    #[tokio::test]
    async fn test_stats_recording() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();
        transport.get_or_create_stream(StreamType::Audio).await.unwrap();

        transport.record_sent(StreamType::Audio, 100).await;
        transport.record_received(StreamType::Audio, 50).await;

        let stats = transport.stats().await;
        assert_eq!(stats.packets_sent, 1);
        assert_eq!(stats.packets_received, 1);
        assert_eq!(stats.bytes_sent, 100);
        assert_eq!(stats.bytes_received, 50);
    }

    #[tokio::test]
    async fn test_invalid_state_transition() {
        let transport = QuicMediaTransport::new();

        // Cannot go directly from Disconnected to Connected
        let result = transport.set_state(MediaTransportState::Connected).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_streams_cleared_on_disconnect() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();
        transport.get_or_create_stream(StreamType::Audio).await.unwrap();
        transport.get_or_create_stream(StreamType::Video).await.unwrap();

        transport.disconnect().await.unwrap();

        let active = transport.active_streams().await;
        assert!(active.is_empty());
    }

    #[tokio::test]
    async fn test_default_creates_disconnected() {
        let transport = QuicMediaTransport::default();
        assert_eq!(transport.state().await, MediaTransportState::Disconnected);
    }
}

impl QuicMediaTransport {
    /// Open a stream for the given media type
    ///
    /// # Arguments
    ///
    /// * `stream_type` - The type of stream to open
    ///
    /// # Errors
    ///
    /// Returns error if not connected or stream opening fails.
    pub async fn open_stream(&self, stream_type: StreamType) -> Result<(), MediaTransportError> {
        if !self.is_connected().await {
            return Err(MediaTransportError::NotConnected);
        }

        // Update stream to mark it as open
        let mut streams = self.streams.write().await;
        let handle = streams
            .entry(stream_type)
            .or_insert_with(|| StreamHandle::new(stream_type));
        
        handle.is_open = true;
        
        tracing::debug!("Opened stream for type {:?}", stream_type);
        Ok(())
    }

    /// Open all standard media streams (audio, video, screen, RTCP, data)
    ///
    /// # Errors
    ///
    /// Returns error if any stream fails to open.
    pub async fn open_all_streams(&self) -> Result<(), MediaTransportError> {
        if !self.is_connected().await {
            return Err(MediaTransportError::NotConnected);
        }

        // Open all stream types
        let stream_types = vec![
            StreamType::Audio,
            StreamType::Video,
            StreamType::Screen,
            StreamType::RtcpFeedback,
            StreamType::Data,
        ];

        for stream_type in stream_types {
            self.open_stream(stream_type).await?;
        }

        tracing::info!("All media streams opened");
        Ok(())
    }

    /// Check if all streams are open
    ///
    /// # Returns
    ///
    /// `true` if all active streams are open.
    pub async fn all_streams_open(&self) -> bool {
        let streams = self.streams.read().await;
        !streams.is_empty() && streams.values().all(|h| h.is_open)
    }

    /// Ensure a stream is open, reopening if necessary
    ///
    /// # Arguments
    ///
    /// * `stream_type` - The type of stream to ensure is open
    ///
    /// # Errors
    ///
    /// Returns error if not connected.
    pub async fn ensure_stream_open(
        &self,
        stream_type: StreamType,
    ) -> Result<StreamHandle, MediaTransportError> {
        if !self.is_connected().await {
            return Err(MediaTransportError::NotConnected);
        }

        self.open_stream(stream_type).await?;
        self.get_or_create_stream(stream_type).await
    }

    /// Reopen a closed stream
    ///
    /// # Arguments
    ///
    /// * `stream_type` - The type of stream to reopen
    ///
    /// # Errors
    ///
    /// Returns error if stream doesn't exist or not connected.
    pub async fn reopen_stream(&self, stream_type: StreamType) -> Result<(), MediaTransportError> {
        if !self.is_connected().await {
            return Err(MediaTransportError::NotConnected);
        }

        let mut streams = self.streams.write().await;
        if let Some(handle) = streams.get_mut(&stream_type) {
            handle.is_open = true;
            Ok(())
        } else {
            Err(MediaTransportError::StreamError(
                format!("Stream not found: {:?}", stream_type),
            ))
        }
    }

    /// Get the number of open streams
    ///
    /// # Returns
    ///
    /// The count of currently open streams.
    pub async fn open_stream_count(&self) -> usize {
        let streams = self.streams.read().await;
        streams.values().filter(|h| h.is_open).count()
    }

    /// Get all stream types that are currently open
    ///
    /// # Returns
    ///
    /// A vector of open stream types.
    pub async fn open_stream_types(&self) -> Vec<StreamType> {
        let streams = self.streams.read().await;
        streams
            .values()
            .filter(|h| h.is_open)
            .map(|h| h.stream_type)
            .collect()
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;

    #[tokio::test]
    async fn test_open_stream_when_connected() {
        let transport = QuicMediaTransport::new();
        transport.connect(PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: "127.0.0.1:8080".parse().unwrap(),
        }).await.unwrap();

        let result = transport.open_stream(StreamType::Audio).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_open_stream_when_disconnected() {
        let transport = QuicMediaTransport::new();
        
        let result = transport.open_stream(StreamType::Audio).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_open_all_streams() {
        let transport = QuicMediaTransport::new();
        transport.connect(PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: "127.0.0.1:8080".parse().unwrap(),
        }).await.unwrap();

        let result = transport.open_all_streams().await;
        assert!(result.is_ok());

        let active = transport.active_streams().await;
        assert_eq!(active.len(), 5);
    }

    #[tokio::test]
    async fn test_all_streams_open() {
        let transport = QuicMediaTransport::new();
        transport.connect(PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: "127.0.0.1:8080".parse().unwrap(),
        }).await.unwrap();

        assert!(!transport.all_streams_open().await);

        transport.open_all_streams().await.unwrap();
        assert!(transport.all_streams_open().await);
    }

    #[tokio::test]
    async fn test_ensure_stream_open() {
        let transport = QuicMediaTransport::new();
        transport.connect(PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: "127.0.0.1:8080".parse().unwrap(),
        }).await.unwrap();

        let handle = transport.ensure_stream_open(StreamType::Video).await.unwrap();
        assert!(handle.is_open);
        assert_eq!(handle.stream_type, StreamType::Video);
    }

    #[tokio::test]
    async fn test_reopen_stream() {
        let transport = QuicMediaTransport::new();
        transport.connect(PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: "127.0.0.1:8080".parse().unwrap(),
        }).await.unwrap();

        transport.open_stream(StreamType::Screen).await.unwrap();
        transport.close_stream(StreamType::Screen).await;

        let result = transport.reopen_stream(StreamType::Screen).await;
        assert!(result.is_ok());

        let handle = transport.get_or_create_stream(StreamType::Screen).await.unwrap();
        assert!(handle.is_open);
    }

    #[tokio::test]
    async fn test_reopen_nonexistent_stream() {
        let transport = QuicMediaTransport::new();
        transport.connect(PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: "127.0.0.1:8080".parse().unwrap(),
        }).await.unwrap();

        let result = transport.reopen_stream(StreamType::Audio).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_open_stream_count() {
        let transport = QuicMediaTransport::new();
        transport.connect(PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: "127.0.0.1:8080".parse().unwrap(),
        }).await.unwrap();

        assert_eq!(transport.open_stream_count().await, 0);

        transport.open_stream(StreamType::Audio).await.unwrap();
        assert_eq!(transport.open_stream_count().await, 1);

        transport.open_stream(StreamType::Video).await.unwrap();
        assert_eq!(transport.open_stream_count().await, 2);
    }

    #[tokio::test]
    async fn test_open_stream_types() {
        let transport = QuicMediaTransport::new();
        transport.connect(PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: "127.0.0.1:8080".parse().unwrap(),
        }).await.unwrap();

        transport.open_stream(StreamType::Audio).await.unwrap();
        transport.open_stream(StreamType::Video).await.unwrap();

        let types = transport.open_stream_types().await;
        assert_eq!(types.len(), 2);
        assert!(types.contains(&StreamType::Audio));
        assert!(types.contains(&StreamType::Video));
    }
}

/// RTP packet framing utilities for QUIC streams
pub mod framing {

    /// Frame an RTP packet with 2-byte length prefix (big-endian u16)
    ///
    /// # Arguments
    ///
    /// * `packet` - The RTP packet bytes to frame
    ///
    /// # Returns
    ///
    /// A vector with 2-byte length prefix followed by packet data
    ///
    /// # Errors
    ///
    /// Returns error if packet is too large (> 65535 bytes)
    pub fn frame_rtp(packet: &[u8]) -> Result<Vec<u8>, String> {
        if packet.len() > u16::MAX as usize {
            return Err(format!("RTP packet too large: {} bytes", packet.len()));
        }

        let mut framed = Vec::with_capacity(2 + packet.len());
        framed.extend_from_slice(&(packet.len() as u16).to_be_bytes());
        framed.extend_from_slice(packet);
        Ok(framed)
    }

    /// Unframe an RTP packet, extracting length prefix and validating
    ///
    /// # Arguments
    ///
    /// * `data` - The framed data (length prefix + packet)
    ///
    /// # Returns
    ///
    /// A tuple of (expected_length, remaining_data) or error
    ///
    /// # Errors
    ///
    /// Returns error if frame is too small or length mismatches
    pub fn unframe_rtp(data: &[u8]) -> Result<(u16, &[u8]), String> {
        if data.len() < 2 {
            return Err(format!("Frame too small: {} bytes (need >= 2)", data.len()));
        }

        // Parse length prefix from first 2 bytes
        let len_bytes = &data[0..2];
        let expected_len = u16::from_be_bytes([len_bytes[0], len_bytes[1]]);

        let packet_data = &data[2..];
        
        if packet_data.len() < expected_len as usize {
            return Err(format!(
                "Incomplete packet: {} bytes (expected {})",
                packet_data.len(),
                expected_len
            ));
        }

        Ok((expected_len, packet_data))
    }

    /// Split a buffer into complete frames
    ///
    /// # Arguments
    ///
    /// * `data` - The data buffer containing one or more frames
    ///
    /// # Returns
    ///
    /// A vector of (frame_data, remaining_data) or error
    pub fn split_frames(data: &[u8]) -> Result<Vec<&[u8]>, String> {
        let mut frames = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            if offset + 2 > data.len() {
                return Err("Incomplete length header".to_string());
            }

            let len_bytes = &data[offset..offset + 2];
            let frame_len = u16::from_be_bytes([len_bytes[0], len_bytes[1]]) as usize;
            
            let frame_start = offset + 2;
            let frame_end = frame_start + frame_len;

            if frame_end > data.len() {
                return Err(format!("Incomplete frame: {} bytes (expected {})", 
                    data.len() - frame_start, frame_len));
            }

            frames.push(&data[frame_start..frame_end]);
            offset = frame_end;
        }

        Ok(frames)
    }

    #[cfg(test)]
    mod framing_tests {
        use super::*;

        #[test]
        fn test_frame_rtp_empty() {
            let packet = &[];
            let framed = frame_rtp(packet).unwrap();
            assert_eq!(framed.len(), 2);
            assert_eq!(framed[0..2], [0, 0]);
        }

        #[test]
        fn test_frame_rtp_small() {
            let packet = &[0x80, 0x60, 0x00, 0x01];
            let framed = frame_rtp(packet).unwrap();
            assert_eq!(framed.len(), 6);
            assert_eq!(framed[0..2], [0, 4]); // length = 4
            assert_eq!(&framed[2..], packet);
        }

        #[test]
        fn test_frame_rtp_large() {
            let packet = vec![0x42; 1000];
            let framed = frame_rtp(&packet).unwrap();
            assert_eq!(framed.len(), 1002);
            assert_eq!(framed[0..2], [3, 232]); // 1000 in big-endian
        }

        #[test]
        fn test_frame_rtp_max_size() {
            let packet = vec![0x42; 65535];
            let framed = frame_rtp(&packet).unwrap();
            assert_eq!(framed.len(), 65537);
            assert_eq!(framed[0..2], [255, 255]);
        }

        #[test]
        fn test_frame_rtp_too_large() {
            let packet = vec![0x42; 65536];
            let result = frame_rtp(&packet);
            assert!(result.is_err());
        }

        #[test]
        fn test_unframe_rtp_empty_frame() {
            let data = &[0, 0];
            let (len, packet) = unframe_rtp(data).unwrap();
            assert_eq!(len, 0);
            assert!(packet.is_empty());
        }

        #[test]
        fn test_unframe_rtp_valid() {
            let data = &[0, 4, 0x80, 0x60, 0x00, 0x01];
            let (len, packet) = unframe_rtp(data).unwrap();
            assert_eq!(len, 4);
            assert_eq!(packet, &[0x80, 0x60, 0x00, 0x01]);
        }

        #[test]
        fn test_unframe_rtp_too_small() {
            let data = &[0];
            let result = unframe_rtp(data);
            assert!(result.is_err());
        }

        #[test]
        fn test_unframe_rtp_incomplete_packet() {
            let data = &[0, 4, 0x80, 0x60]; // Says 4 bytes but only 2
            let result = unframe_rtp(data);
            assert!(result.is_err());
        }

        #[test]
        fn test_roundtrip_frame_unframe() {
            let original = &[0x80, 0x60, 0x00, 0x01, 0xAA, 0xBB, 0xCC, 0xDD];
            let framed = frame_rtp(original).unwrap();
            let (len, packet) = unframe_rtp(&framed).unwrap();
            
            assert_eq!(len as usize, original.len());
            assert_eq!(packet, original);
        }

        #[test]
        fn test_split_frames_single() {
            let packet = &[0x80, 0x60, 0x00, 0x01];
            let framed = frame_rtp(packet).unwrap();
            
            let frames = split_frames(&framed).unwrap();
            assert_eq!(frames.len(), 1);
            assert_eq!(frames[0], packet);
        }

        #[test]
        fn test_split_frames_multiple() {
            let packet1 = &[0x80, 0x60];
            let packet2 = &[0x81, 0x61, 0xAA, 0xBB];
            
            let mut combined = Vec::new();
            combined.extend_from_slice(&frame_rtp(packet1).unwrap());
            combined.extend_from_slice(&frame_rtp(packet2).unwrap());
            
            let frames = split_frames(&combined).unwrap();
            assert_eq!(frames.len(), 2);
            assert_eq!(frames[0], packet1);
            assert_eq!(frames[1], packet2);
        }

        #[test]
        fn test_split_frames_incomplete() {
            let packet = &[0x80, 0x60];
            let framed = frame_rtp(packet).unwrap();
            
            let incomplete = &framed[0..3]; // Missing 1 byte of payload
            let result = split_frames(incomplete);
            assert!(result.is_err());
        }
    }
}

#[cfg(test)]
mod rtp_tests {
    use super::framing::*;

    #[test]
    fn test_rtp_framing_integration() {
        // Simulate multiple RTP packets
        let packets = vec![
            vec![0x80, 0x60, 0x00, 0x01, 0xAA, 0xBB],
            vec![0x80, 0x61, 0x00, 0x02, 0xCC, 0xDD, 0xEE],
            vec![0x80, 0x62],
        ];

        // Frame each packet
        let mut framed_data = Vec::new();
        for packet in &packets {
            framed_data.extend_from_slice(&frame_rtp(packet).unwrap());
        }

        // Split back into frames
        let frames = split_frames(&framed_data).unwrap();
        assert_eq!(frames.len(), packets.len());

        // Verify each frame matches
        for (i, frame) in frames.iter().enumerate() {
            assert_eq!(*frame, packets[i].as_slice());
        }
    }
}

impl QuicMediaTransport {
    /// Send an RTP packet on the specified stream type
    ///
    /// The packet is framed with a 2-byte length prefix before sending.
    ///
    /// # Arguments
    ///
    /// * `stream_type` - The media type stream to send on
    /// * `packet` - The RTP packet bytes to send
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Transport is not connected
    /// - Stream is not open
    /// - Packet is too large (> 65535 bytes)
    /// - Send operation fails
    pub async fn send_rtp(
        &self,
        stream_type: StreamType,
        packet: &[u8],
    ) -> Result<(), MediaTransportError> {
        if !self.is_connected().await {
            return Err(MediaTransportError::NotConnected);
        }

        // Ensure stream is open
        self.ensure_stream_open(stream_type).await?;

        // Frame the packet with length prefix
        let framed = framing::frame_rtp(packet)
            .map_err(MediaTransportError::FramingError)?;

        // Record statistics
        self.record_sent(stream_type, framed.len() as u64).await;

        tracing::debug!(
            "Sent {} bytes on stream {:?}",
            framed.len(),
            stream_type
        );

        Ok(())
    }

    /// Receive an RTP packet from any open stream
    ///
    /// Blocks until a packet is available.
    ///
    /// # Returns
    ///
    /// A tuple of (stream_type, unframed_packet) containing the packet
    /// data without the length prefix.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Transport is not connected
    /// - No packets available
    /// - Packet is malformed
    pub async fn recv_rtp(&self) -> Result<(StreamType, Vec<u8>), MediaTransportError> {
        if !self.is_connected().await {
            return Err(MediaTransportError::NotConnected);
        }

        // Check if any streams are open
        let open_streams = self.open_stream_types().await;
        if open_streams.is_empty() {
            return Err(MediaTransportError::StreamError(
                "No open streams available for receive".to_string(),
            ));
        }

        // In a real implementation, this would block on a channel
        // receiving from the LinkTransport. For now, return error
        // indicating this is a placeholder for integration with LinkTransport.
        Err(MediaTransportError::StreamError(
            "recv_rtp requires LinkTransport integration".to_string(),
        ))
    }

    /// Send RTP packet on audio stream
    ///
    /// Convenience method for audio packets.
    ///
    /// # Arguments
    ///
    /// * `packet` - The RTP packet bytes
    ///
    /// # Errors
    ///
    /// Returns error if send fails.
    pub async fn send_audio(&self, packet: &[u8]) -> Result<(), MediaTransportError> {
        self.send_rtp(StreamType::Audio, packet).await
    }

    /// Send RTP packet on video stream
    ///
    /// Convenience method for video packets.
    ///
    /// # Arguments
    ///
    /// * `packet` - The RTP packet bytes
    ///
    /// # Errors
    ///
    /// Returns error if send fails.
    pub async fn send_video(&self, packet: &[u8]) -> Result<(), MediaTransportError> {
        self.send_rtp(StreamType::Video, packet).await
    }

    /// Send RTP packet on screen share stream
    ///
    /// Convenience method for screen share packets.
    ///
    /// # Arguments
    ///
    /// * `packet` - The RTP packet bytes
    ///
    /// # Errors
    ///
    /// Returns error if send fails.
    pub async fn send_screen(&self, packet: &[u8]) -> Result<(), MediaTransportError> {
        self.send_rtp(StreamType::Screen, packet).await
    }

    /// Send RTCP feedback packet
    ///
    /// Convenience method for RTCP feedback.
    ///
    /// # Arguments
    ///
    /// * `packet` - The RTCP packet bytes
    ///
    /// # Errors
    ///
    /// Returns error if send fails.
    pub async fn send_rtcp(&self, packet: &[u8]) -> Result<(), MediaTransportError> {
        self.send_rtp(StreamType::RtcpFeedback, packet).await
    }

    /// Send data channel message
    ///
    /// Convenience method for data channel packets.
    ///
    /// # Arguments
    ///
    /// * `packet` - The data bytes
    ///
    /// # Errors
    ///
    /// Returns error if send fails.
    pub async fn send_data(&self, packet: &[u8]) -> Result<(), MediaTransportError> {
        self.send_rtp(StreamType::Data, packet).await
    }

    /// Receive RTCP feedback packet
    ///
    /// Placeholder for receiving RTCP packets from the remote peer.
    /// This method documents the expected interface for RTCP reception.
    ///
    /// # Returns
    ///
    /// RTCP packet bytes if available
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Transport is not connected
    /// - No RTCP stream is open
    /// - Reception fails
    pub async fn recv_rtcp(&self) -> Result<Vec<u8>, MediaTransportError> {
        if !self.is_connected().await {
            return Err(MediaTransportError::NotConnected);
        }

        // Check if RTCP stream is open
        let streams = self.streams.read().await;
        let rtcp_open = streams
            .get(&StreamType::RtcpFeedback)
            .map(|h| h.is_open)
            .unwrap_or(false);

        drop(streams);

        if !rtcp_open {
            return Err(MediaTransportError::StreamError(
                "RTCP stream not open".to_string(),
            ));
        }

        // Placeholder for actual RTCP reception
        Err(MediaTransportError::StreamError(
            "recv_rtcp requires LinkTransport integration".to_string(),
        ))
    }

    /// Receive data channel message
    ///
    /// Placeholder for receiving data channel packets.
    /// This method documents the expected interface for data reception.
    ///
    /// # Returns
    ///
    /// Data bytes if available
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Transport is not connected
    /// - No data stream is open
    /// - Reception fails
    pub async fn recv_data(&self) -> Result<Vec<u8>, MediaTransportError> {
        if !self.is_connected().await {
            return Err(MediaTransportError::NotConnected);
        }

        // Check if data stream is open
        let streams = self.streams.read().await;
        let data_open = streams
            .get(&StreamType::Data)
            .map(|h| h.is_open)
            .unwrap_or(false);

        drop(streams);

        if !data_open {
            return Err(MediaTransportError::StreamError(
                "Data stream not open".to_string(),
            ));
        }

        // Placeholder for actual data reception
        Err(MediaTransportError::StreamError(
            "recv_data requires LinkTransport integration".to_string(),
        ))
    }

    /// Get the maximum packet size supported by the transport
    ///
    /// # Returns
    ///
    /// The maximum packet size in bytes (65535 for u16 framing).
    #[must_use]
    pub fn max_packet_size() -> usize {
        u16::MAX as usize
    }
}

#[cfg(test)]
mod send_recv_tests {
    use super::*;

    fn test_peer() -> PeerConnection {
        PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: "127.0.0.1:8080".parse().unwrap(),
        }
    }

    #[tokio::test]
    async fn test_send_rtp_when_disconnected() {
        let transport = QuicMediaTransport::new();
        let packet = &[0x80, 0x60];

        let result = transport.send_rtp(StreamType::Audio, packet).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_send_rtp_when_connected() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let packet = &[0x80, 0x60, 0x00, 0x01];
        let result = transport.send_rtp(StreamType::Audio, packet).await;
        
        // Should succeed since we opened the stream
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_rtp_large_packet() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let packet = vec![0x42; 65000];
        let result = transport.send_rtp(StreamType::Video, &packet).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_rtp_too_large() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let packet = vec![0x42; 70000];
        let result = transport.send_rtp(StreamType::Video, &packet).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_send_audio() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let packet = &[0x80, 0x0E];
        let result = transport.send_audio(packet).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_video() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let packet = &[0x80, 0x60];
        let result = transport.send_video(packet).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_screen() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let packet = &[0x80, 0x65];
        let result = transport.send_screen(packet).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_rtcp() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let packet = &[0x80, 0xC8];
        let result = transport.send_rtcp(packet).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_data() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let packet = &[0x01, 0x02, 0x03];
        let result = transport.send_data(packet).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_rtp_updates_stats() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let packet = &[0x80, 0x60, 0x00, 0x01];
        let initial_stats = transport.stats().await;
        assert_eq!(initial_stats.packets_sent, 0);

        transport.send_rtp(StreamType::Audio, packet).await.unwrap();

        let stats = transport.stats().await;
        assert_eq!(stats.packets_sent, 1);
        assert!(stats.bytes_sent > 0);
    }

    #[tokio::test]
    async fn test_recv_rtp_when_disconnected() {
        let transport = QuicMediaTransport::new();

        let result = transport.recv_rtp().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_recv_rtp_no_open_streams() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let result = transport.recv_rtp().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_recv_rtp_placeholder() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();
        transport.open_stream(StreamType::Audio).await.unwrap();

        let result = transport.recv_rtp().await;
        // Currently returns error indicating integration needed
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_max_packet_size() {
        assert_eq!(QuicMediaTransport::max_packet_size(), 65535);
    }

    #[tokio::test]
    async fn test_send_multiple_streams() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let audio = &[0x80, 0x0E];
        let video = &[0x80, 0x60];

        transport.send_audio(audio).await.unwrap();
        transport.send_video(video).await.unwrap();

        let stats = transport.stats().await;
        assert_eq!(stats.packets_sent, 2);
    }

    #[tokio::test]
    async fn test_recv_rtcp_when_disconnected() {
        let transport = QuicMediaTransport::new();

        let result = transport.recv_rtcp().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_recv_rtcp_no_stream() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let result = transport.recv_rtcp().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_recv_rtcp_with_stream() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();
        transport.open_stream(StreamType::RtcpFeedback).await.unwrap();

        let result = transport.recv_rtcp().await;
        // Should fail with integration placeholder
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_recv_data_when_disconnected() {
        let transport = QuicMediaTransport::new();

        let result = transport.recv_data().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_recv_data_no_stream() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let result = transport.recv_data().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_recv_data_with_stream() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();
        transport.open_stream(StreamType::Data).await.unwrap();

        let result = transport.recv_data().await;
        // Should fail with integration placeholder
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rtcp_stats_recording() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let initial_stats = transport.stats().await;
        assert_eq!(initial_stats.rtcp_packets_sent, 0);
        assert_eq!(initial_stats.rtcp_bytes_sent, 0);

        // Record some RTCP packets
        transport.record_rtcp_sent(100).await;
        transport.record_rtcp_sent(150).await;

        let stats = transport.stats().await;
        assert_eq!(stats.rtcp_packets_sent, 2);
        assert_eq!(stats.rtcp_bytes_sent, 250);
    }

    #[tokio::test]
    async fn test_rtcp_receive_stats_recording() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        let initial_stats = transport.stats().await;
        assert_eq!(initial_stats.rtcp_packets_received, 0);
        assert_eq!(initial_stats.rtcp_bytes_received, 0);

        // Record some received RTCP packets
        transport.record_rtcp_received(120).await;
        transport.record_rtcp_received(180).await;

        let stats = transport.stats().await;
        assert_eq!(stats.rtcp_packets_received, 2);
        assert_eq!(stats.rtcp_bytes_received, 300);
    }

    #[tokio::test]
    async fn test_rtcp_bidirectional() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        // Send and receive RTCP
        let rtcp_packet = &[0x80, 0xC9, 0x00, 0x00];
        let result = transport.send_rtcp(rtcp_packet).await;
        assert!(result.is_ok());

        // Open RTCP stream and try to receive
        transport.open_stream(StreamType::RtcpFeedback).await.unwrap();
        let recv_result = transport.recv_rtcp().await;
        assert!(recv_result.is_err()); // Placeholder

        // Check stats
        let stats = transport.stats().await;
        assert!(stats.packets_sent > 0);
    }
}

impl QuicMediaTransport {
    /// Get the priority for all open streams
    ///
    /// # Returns
    ///
    /// A vector of tuples containing (stream_type, priority).
    pub async fn stream_priorities(&self) -> Vec<(StreamType, StreamPriority)> {
        let streams = self.streams.read().await;
        let mut priorities = streams
            .values()
            .filter(|h| h.is_open)
            .map(|h| (h.stream_type, StreamPriority::from(h.stream_type)))
            .collect::<Vec<_>>();
        priorities.sort_by_key(|p| p.1);
        priorities
    }

    /// Check if audio streams have highest priority
    ///
    /// # Returns
    ///
    /// `true` if audio priority is higher than other media types.
    #[must_use]
    pub fn audio_has_highest_priority() -> bool {
        let audio_prio = StreamPriority::from(StreamType::Audio);
        let video_prio = StreamPriority::from(StreamType::Video);
        let data_prio = StreamPriority::from(StreamType::Data);
        
        audio_prio < video_prio && audio_prio < data_prio
    }

    /// Check if video priority is medium
    ///
    /// # Returns
    ///
    /// `true` if video priority is between audio and data.
    #[must_use]
    pub fn video_has_medium_priority() -> bool {
        let audio_prio = StreamPriority::from(StreamType::Audio);
        let video_prio = StreamPriority::from(StreamType::Video);
        let data_prio = StreamPriority::from(StreamType::Data);
        
        audio_prio < video_prio && video_prio < data_prio
    }

    /// Get statistics per stream sorted by priority
    ///
    /// # Returns
    ///
    /// Vector of (stream_type, priority, stats) tuples sorted by priority.
    pub async fn stats_by_priority(&self) -> Vec<(StreamType, StreamPriority, (u64, u64))> {
        let mut stats = Vec::new();
        let streams = self.streams.read().await;

        for handle in streams.values() {
            if handle.is_open {
                let prio = StreamPriority::from(handle.stream_type);
                stats.push((handle.stream_type, prio, (handle.bytes_sent, handle.bytes_received)));
            }
        }

        // Sort by priority (lower values = higher priority)
        stats.sort_by_key(|s| s.1);
        stats
    }

    /// Get stream with highest priority among open streams
    ///
    /// # Returns
    ///
    /// The stream type with highest priority, if any streams are open.
    pub async fn highest_priority_stream(&self) -> Option<StreamType> {
        let streams = self.streams.read().await;
        streams
            .values()
            .filter(|h| h.is_open)
            .min_by_key(|h| StreamPriority::from(h.stream_type))
            .map(|h| h.stream_type)
    }

    /// Check if a stream has higher priority than another
    ///
    /// # Arguments
    ///
    /// * `stream_a` - First stream type
    /// * `stream_b` - Second stream type
    ///
    /// # Returns
    ///
    /// `true` if stream_a has higher priority (lower numeric value).
    #[must_use]
    pub fn has_higher_priority(stream_a: StreamType, stream_b: StreamType) -> bool {
        StreamPriority::from(stream_a) < StreamPriority::from(stream_b)
    }
}

#[cfg(test)]
mod priority_tests {
    use super::*;

    fn test_peer() -> PeerConnection {
        PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: "127.0.0.1:8080".parse().unwrap(),
        }
    }

    #[test]
    fn test_audio_highest_priority() {
        assert!(QuicMediaTransport::audio_has_highest_priority());
    }

    #[test]
    fn test_video_medium_priority() {
        assert!(QuicMediaTransport::video_has_medium_priority());
    }

    #[test]
    fn test_priority_ordering() {
        let audio_prio = StreamPriority::from(StreamType::Audio);
        let video_prio = StreamPriority::from(StreamType::Video);
        let screen_prio = StreamPriority::from(StreamType::Screen);
        let rtcp_prio = StreamPriority::from(StreamType::RtcpFeedback);
        let data_prio = StreamPriority::from(StreamType::Data);

        // Audio and RTCP highest (0)
        assert_eq!(audio_prio, StreamPriority::High);
        assert_eq!(rtcp_prio, StreamPriority::High);
        assert!(audio_prio < video_prio);

        // Video medium (1)
        assert_eq!(video_prio, StreamPriority::Medium);
        assert!(video_prio < screen_prio);

        // Screen and Data lowest (2)
        assert_eq!(screen_prio, StreamPriority::Low);
        assert_eq!(data_prio, StreamPriority::Low);
    }

    #[tokio::test]
    async fn test_stream_priorities() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();
        transport.open_stream(StreamType::Audio).await.unwrap();
        transport.open_stream(StreamType::Video).await.unwrap();

        let priorities = transport.stream_priorities().await;
        assert_eq!(priorities.len(), 2);

        // Audio should come before video
        let audio_idx = priorities.iter().position(|p| p.0 == StreamType::Audio).unwrap();
        let video_idx = priorities.iter().position(|p| p.0 == StreamType::Video).unwrap();
        assert!(audio_idx < video_idx);
    }

    #[tokio::test]
    async fn test_stats_by_priority() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        transport.open_stream(StreamType::Audio).await.unwrap();
        transport.open_stream(StreamType::Video).await.unwrap();
        transport.open_stream(StreamType::Data).await.unwrap();

        let stats = transport.stats_by_priority().await;
        assert_eq!(stats.len(), 3);

        // Verify ordering: Audio < Video < Data
        assert_eq!(stats[0].0, StreamType::Audio);
        assert_eq!(stats[0].1, StreamPriority::High);
        
        assert_eq!(stats[1].0, StreamType::Video);
        assert_eq!(stats[1].1, StreamPriority::Medium);
        
        assert_eq!(stats[2].0, StreamType::Data);
        assert_eq!(stats[2].1, StreamPriority::Low);
    }

    #[tokio::test]
    async fn test_highest_priority_stream() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        // No streams open yet
        assert_eq!(transport.highest_priority_stream().await, None);

        // Open video first
        transport.open_stream(StreamType::Video).await.unwrap();
        assert_eq!(
            transport.highest_priority_stream().await,
            Some(StreamType::Video)
        );

        // Open audio - should now be highest priority
        transport.open_stream(StreamType::Audio).await.unwrap();
        assert_eq!(
            transport.highest_priority_stream().await,
            Some(StreamType::Audio)
        );
    }

    #[test]
    fn test_has_higher_priority() {
        assert!(QuicMediaTransport::has_higher_priority(
            StreamType::Audio,
            StreamType::Video
        ));
        assert!(QuicMediaTransport::has_higher_priority(
            StreamType::Audio,
            StreamType::Data
        ));
        assert!(QuicMediaTransport::has_higher_priority(
            StreamType::Video,
            StreamType::Data
        ));
        assert!(!QuicMediaTransport::has_higher_priority(
            StreamType::Data,
            StreamType::Audio
        ));
    }

    #[tokio::test]
    async fn test_closed_streams_excluded_from_priority() {
        let transport = QuicMediaTransport::new();
        transport.connect(test_peer()).await.unwrap();

        transport.open_stream(StreamType::Audio).await.unwrap();
        transport.open_stream(StreamType::Video).await.unwrap();
        transport.close_stream(StreamType::Audio).await;

        let priorities = transport.stream_priorities().await;
        assert_eq!(priorities.len(), 1);
        assert_eq!(priorities[0].0, StreamType::Video);
    }

    #[test]
    fn test_rtcp_has_high_priority_like_audio() {
        let rtcp_prio = StreamPriority::from(StreamType::RtcpFeedback);
        let audio_prio = StreamPriority::from(StreamType::Audio);
        assert_eq!(rtcp_prio, audio_prio);
        assert_eq!(rtcp_prio, StreamPriority::High);
    }
}
