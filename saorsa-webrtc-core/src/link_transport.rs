//! Link transport abstraction layer
//!
//! Provides abstraction over ant-quic API to enable cleaner separation of concerns
//! and easier migration to future transport implementations.

use async_trait::async_trait;
use std::net::SocketAddr;
use thiserror::Error;

/// Stream type identifiers for WebRTC media multiplexing
///
/// Allocates stream IDs in the 0x20-0x2F range to enable multiple
/// concurrent media streams over a single QUIC connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum StreamType {
    /// Audio RTP stream (0x20)
    Audio = 0x20,
    /// Video RTP stream (0x21)
    Video = 0x21,
    /// Screen share RTP stream (0x22)
    Screen = 0x22,
    /// RTCP feedback stream (0x23)
    RtcpFeedback = 0x23,
    /// Data channel (0x24)
    Data = 0x24,
}

impl StreamType {
    /// Get stream type as byte value
    #[must_use]
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Try to convert byte value to StreamType
    ///
    /// # Errors
    ///
    /// Returns None if byte is not a valid StreamType
    pub fn try_from_u8(val: u8) -> Option<Self> {
        match val {
            0x20 => Some(StreamType::Audio),
            0x21 => Some(StreamType::Video),
            0x22 => Some(StreamType::Screen),
            0x23 => Some(StreamType::RtcpFeedback),
            0x24 => Some(StreamType::Data),
            _ => None,
        }
    }
}

/// Link transport errors
#[derive(Error, Debug, Clone)]
pub enum LinkTransportError {
    /// Connection not established
    #[error("Connection not established")]
    NotConnected,

    /// Peer not found
    #[error("Peer not found: {0}")]
    PeerNotFound(String),

    /// Send operation failed
    #[error("Send failed: {0}")]
    SendError(String),

    /// Receive operation failed
    #[error("Receive failed: {0}")]
    ReceiveError(String),

    /// Invalid stream type
    #[error("Invalid stream type: {0}")]
    InvalidStreamType(u8),

    /// IO error
    #[error("IO error: {0}")]
    IoError(String),
}

/// Represents a peer connection handle
///
/// This abstraction allows transport implementations to use their own
/// internal peer ID formats without exposing them to higher layers.
#[derive(Debug, Clone)]
pub struct PeerConnection {
    /// Peer identifier (transport-specific)
    pub peer_id: String,
    /// Remote socket address
    pub remote_addr: SocketAddr,
}

/// Link transport trait
///
/// Provides abstraction over QUIC transport operations, enabling:
/// - Cleaner separation between transport and application concerns
/// - Easier migration to alternative transport implementations
/// - Stable API that doesn't change with underlying transport upgrades
///
/// This trait supports both connection-oriented operations (connect/accept)
/// and connectionless operations (send/receive on default peer).
#[async_trait]
pub trait LinkTransport: Send + Sync {
    /// Start the transport
    ///
    /// # Errors
    ///
    /// Returns error if transport initialization fails
    async fn start(&mut self) -> Result<(), LinkTransportError>;

    /// Stop the transport
    ///
    /// # Errors
    ///
    /// Returns error if transport shutdown fails
    async fn stop(&mut self) -> Result<(), LinkTransportError>;

    /// Check if transport is running
    #[must_use]
    async fn is_running(&self) -> bool;

    /// Get local address
    ///
    /// # Errors
    ///
    /// Returns error if transport not running or address unavailable
    async fn local_addr(&self) -> Result<SocketAddr, LinkTransportError>;

    /// Connect to a peer at the given address
    ///
    /// # Errors
    ///
    /// Returns error if connection fails
    async fn connect(&mut self, addr: SocketAddr) -> Result<PeerConnection, LinkTransportError>;

    /// Accept an incoming connection
    ///
    /// Blocks until a peer connects or returns None if no connections available.
    /// Implementation may timeout after a period to allow for shutdown.
    ///
    /// # Errors
    ///
    /// Returns error on accept failure
    async fn accept(&mut self) -> Result<Option<PeerConnection>, LinkTransportError>;

    /// Send data to a specific peer on the specified stream
    ///
    /// # Errors
    ///
    /// Returns error if send fails
    async fn send(
        &self,
        peer: &PeerConnection,
        stream_type: StreamType,
        data: &[u8],
    ) -> Result<(), LinkTransportError>;

    /// Receive data from any peer
    ///
    /// Returns (peer_connection, stream_type, data) tuple.
    ///
    /// # Errors
    ///
    /// Returns error if receive fails
    async fn receive(&self) -> Result<(PeerConnection, StreamType, Vec<u8>), LinkTransportError>;

    /// Send data to default peer (convenience method)
    ///
    /// # Errors
    ///
    /// Returns error if no default peer or send fails
    async fn send_default(
        &self,
        stream_type: StreamType,
        data: &[u8],
    ) -> Result<(), LinkTransportError> {
        self.send(&self.default_peer()?, stream_type, data).await
    }

    /// Get the current default peer
    ///
    /// # Errors
    ///
    /// Returns error if no default peer has been set
    fn default_peer(&self) -> Result<PeerConnection, LinkTransportError> {
        Err(LinkTransportError::NotConnected)
    }

    /// Set the default peer for send operations
    ///
    /// # Errors
    ///
    /// Returns error if unable to set default peer
    fn set_default_peer(&mut self, peer: PeerConnection) -> Result<(), LinkTransportError> {
        let _ = peer;
        Err(LinkTransportError::NotConnected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_type_conversions() {
        assert_eq!(StreamType::Audio.as_u8(), 0x20);
        assert_eq!(StreamType::Video.as_u8(), 0x21);
        assert_eq!(StreamType::Screen.as_u8(), 0x22);
        assert_eq!(StreamType::RtcpFeedback.as_u8(), 0x23);
        assert_eq!(StreamType::Data.as_u8(), 0x24);
    }

    #[test]
    fn test_stream_type_from_u8() {
        assert_eq!(StreamType::try_from_u8(0x20), Some(StreamType::Audio));
        assert_eq!(StreamType::try_from_u8(0x21), Some(StreamType::Video));
        assert_eq!(StreamType::try_from_u8(0x22), Some(StreamType::Screen));
        assert_eq!(
            StreamType::try_from_u8(0x23),
            Some(StreamType::RtcpFeedback)
        );
        assert_eq!(StreamType::try_from_u8(0x24), Some(StreamType::Data));
        assert_eq!(StreamType::try_from_u8(0x25), None);
        assert_eq!(StreamType::try_from_u8(0xFF), None);
    }

    #[test]
    fn test_stream_type_roundtrip() {
        let types = vec![
            StreamType::Audio,
            StreamType::Video,
            StreamType::Screen,
            StreamType::RtcpFeedback,
            StreamType::Data,
        ];

        for original in types {
            let byte = original.as_u8();
            let recovered = StreamType::try_from_u8(byte);
            assert_eq!(recovered, Some(original));
        }
    }
}
