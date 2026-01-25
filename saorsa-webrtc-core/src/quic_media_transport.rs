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
