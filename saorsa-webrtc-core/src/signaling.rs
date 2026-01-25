//! WebRTC signaling protocol
//!
//! Handles SDP exchange and ICE candidate gathering for WebRTC connections.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;
use tokio::time::{sleep, Instant};

/// Signaling errors
#[derive(Error, Debug)]
pub enum SignalingError {
    /// Invalid SDP
    #[error("Invalid SDP: {0}")]
    InvalidSdp(String),

    /// Session not found
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    /// Transport error
    #[error("Transport error: {0}")]
    TransportError(String),
}

/// Signaling transport trait
///
/// Implement this for your specific transport (DHT, gossip, etc.)
///
/// # Connection Sharing
///
/// For QUIC-based transports (like AntQuicTransport), the underlying connection
/// can be shared with media transport handlers. This enables:
///
/// - Multiplexing signaling and media over a single QUIC connection
/// - Stream-based media routing (different stream types for audio/video/data)
/// - Reduced connection overhead and improved NAT traversal efficiency
///
/// To access the underlying connection for sharing, use `get_connection()` method
/// or transport-specific methods (e.g., `AntQuicTransport::get_node()` for ant-quic).
#[async_trait]
pub trait SignalingTransport: Send + Sync {
    /// Peer identifier type
    type PeerId: Clone + Send + Sync + fmt::Debug + fmt::Display + FromStr;

    /// Transport error type
    type Error: std::error::Error + Send + Sync + 'static;

    /// Send a signaling message
    async fn send_message(
        &self,
        peer: &Self::PeerId,
        message: SignalingMessage,
    ) -> Result<(), Self::Error>;

    /// Receive a signaling message
    async fn receive_message(&self) -> Result<(Self::PeerId, SignalingMessage), Self::Error>;

    /// Discover peer endpoint
    async fn discover_peer_endpoint(
        &self,
        peer: &Self::PeerId,
    ) -> Result<Option<SocketAddr>, Self::Error>;

    /// Get the underlying QUIC connection handle for connection sharing
    ///
    /// This method allows media transport handlers to share the signaling connection,
    /// avoiding the need for separate ICE negotiation.
    ///
    /// # Returns
    ///
    /// Returns `None` if the transport doesn't support connection sharing or the
    /// connection is not yet established. Implementations should return `Some` only
    /// when the underlying connection is ready to be shared.
    ///
    /// # Examples
    ///
    /// For AntQuicTransport, this returns the underlying ant-quic Node handle
    /// which can be used to create QUIC streams for media transport.
    fn get_connection_handle(&self) -> Option<Box<dyn std::any::Any>> {
        None
    }
}

/// Signaling message types
///
/// Supports both legacy WebRTC (SDP/ICE) and QUIC-native (capability exchange) signaling.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SignalingMessage {
    // === Legacy WebRTC Messages (deprecated for new calls) ===
    /// SDP offer (legacy WebRTC)
    #[serde(rename = "offer")]
    Offer {
        /// Session ID
        session_id: String,
        /// SDP content
        sdp: String,
        /// Optional QUIC endpoint
        quic_endpoint: Option<SocketAddr>,
    },

    /// SDP answer (legacy WebRTC)
    #[serde(rename = "answer")]
    Answer {
        /// Session ID
        session_id: String,
        /// SDP content
        sdp: String,
        /// Optional QUIC endpoint
        quic_endpoint: Option<SocketAddr>,
    },

    /// ICE candidate (legacy WebRTC)
    #[serde(rename = "ice_candidate")]
    IceCandidate {
        /// Session ID
        session_id: String,
        /// Candidate string
        candidate: String,
        /// SDP mid
        sdp_mid: Option<String>,
        /// SDP mline index
        sdp_mline_index: Option<u16>,
    },

    /// ICE gathering complete (legacy WebRTC)
    #[serde(rename = "ice_complete")]
    IceComplete {
        /// Session ID
        session_id: String,
    },

    // === QUIC-Native Messages ===
    /// Capability exchange (QUIC-native)
    ///
    /// Sent instead of SDP offer. Contains local media capabilities.
    #[serde(rename = "capability_exchange")]
    CapabilityExchange {
        /// Session/call ID
        session_id: String,
        /// Local media capabilities
        audio: bool,
        /// Video capability
        video: bool,
        /// Data channel capability
        data_channel: bool,
        /// Maximum bandwidth in kbps
        max_bandwidth_kbps: u32,
        /// QUIC endpoint for direct connection
        quic_endpoint: Option<SocketAddr>,
    },

    /// Connection confirmation (QUIC-native)
    ///
    /// Sent in response to CapabilityExchange to confirm connection is ready.
    #[serde(rename = "connection_confirm")]
    ConnectionConfirm {
        /// Session/call ID
        session_id: String,
        /// Peer's media capabilities (for mutual agreement)
        audio: bool,
        /// Video capability
        video: bool,
        /// Data channel capability
        data_channel: bool,
        /// Maximum bandwidth in kbps
        max_bandwidth_kbps: u32,
        /// QUIC endpoint for direct connection
        quic_endpoint: Option<SocketAddr>,
    },

    /// Connection ready notification (QUIC-native)
    ///
    /// Sent when QUIC connection is established and media can flow.
    #[serde(rename = "connection_ready")]
    ConnectionReady {
        /// Session/call ID
        session_id: String,
    },

    // === Common Messages ===
    /// Close session
    #[serde(rename = "bye")]
    Bye {
        /// Session ID
        session_id: String,
        /// Optional reason
        reason: Option<String>,
    },
}

impl SignalingMessage {
    /// Get the session ID
    #[must_use]
    pub fn session_id(&self) -> &str {
        match self {
            // Legacy WebRTC
            Self::Offer { session_id, .. }
            | Self::Answer { session_id, .. }
            | Self::IceCandidate { session_id, .. }
            | Self::IceComplete { session_id }
            // QUIC-native
            | Self::CapabilityExchange { session_id, .. }
            | Self::ConnectionConfirm { session_id, .. }
            | Self::ConnectionReady { session_id }
            // Common
            | Self::Bye { session_id, .. } => session_id,
        }
    }

    /// Check if this is a QUIC-native message
    #[must_use]
    pub fn is_quic_native(&self) -> bool {
        matches!(
            self,
            Self::CapabilityExchange { .. }
                | Self::ConnectionConfirm { .. }
                | Self::ConnectionReady { .. }
        )
    }

    /// Check if this is a legacy WebRTC message
    #[must_use]
    pub fn is_legacy_webrtc(&self) -> bool {
        matches!(
            self,
            Self::Offer { .. }
                | Self::Answer { .. }
                | Self::IceCandidate { .. }
                | Self::IceComplete { .. }
        )
    }
}

/// Minimum time between messages (10ms for 100 msg/sec rate limit)
const MIN_MESSAGE_INTERVAL: Duration = Duration::from_millis(10);

/// Signaling handler with rate limiting
pub struct SignalingHandler<T: SignalingTransport> {
    transport: std::sync::Arc<T>,
    last_receive_time: std::sync::Arc<tokio::sync::Mutex<Instant>>,
    error_count: std::sync::Arc<tokio::sync::Mutex<u32>>,
}

impl<T: SignalingTransport> SignalingHandler<T> {
    /// Create new signaling handler
    #[must_use]
    pub fn new(transport: std::sync::Arc<T>) -> Self {
        Self {
            transport,
            last_receive_time: std::sync::Arc::new(tokio::sync::Mutex::new(Instant::now())),
            error_count: std::sync::Arc::new(tokio::sync::Mutex::new(0)),
        }
    }

    /// Send a signaling message to a peer
    ///
    /// # Errors
    ///
    /// Returns error if sending fails
    #[tracing::instrument(skip(self, message), fields(peer = %peer, message_type = ?message_type(&message)))]
    pub async fn send_message(
        &self,
        peer: &T::PeerId,
        message: SignalingMessage,
    ) -> Result<(), T::Error> {
        tracing::debug!("Sending signaling message");
        self.transport.send_message(peer, message).await
    }

    /// Receive a signaling message with rate limiting and backpressure
    ///
    /// # Errors
    ///
    /// Returns error if receiving fails
    #[tracing::instrument(skip(self))]
    pub async fn receive_message(&self) -> Result<(T::PeerId, SignalingMessage), T::Error> {
        let mut last_time = self.last_receive_time.lock().await;
        let now = Instant::now();
        let elapsed = now.duration_since(*last_time);

        if elapsed < MIN_MESSAGE_INTERVAL {
            let sleep_duration = MIN_MESSAGE_INTERVAL - elapsed;
            tracing::trace!(
                sleep_ms = sleep_duration.as_millis(),
                "Rate limiting applied"
            );
            drop(last_time);
            sleep(sleep_duration).await;
            last_time = self.last_receive_time.lock().await;
        }

        *last_time = Instant::now();
        drop(last_time);

        tracing::debug!("Waiting for signaling message");

        match self.transport.receive_message().await {
            Ok(result) => {
                let mut error_count = self.error_count.lock().await;
                *error_count = 0;
                drop(error_count);

                tracing::debug!(peer = %result.0, message_type = ?message_type(&result.1), "Received signaling message");
                Ok(result)
            }
            Err(e) => {
                let mut error_count = self.error_count.lock().await;
                *error_count += 1;
                let count = *error_count;
                drop(error_count);

                let backoff_duration = Duration::from_millis(100 * u64::from(count.min(10)));
                tracing::warn!(
                    error_count = count,
                    backoff_ms = backoff_duration.as_millis(),
                    "Error receiving message, applying exponential backoff"
                );
                sleep(backoff_duration).await;

                Err(e)
            }
        }
    }

    /// Discover endpoint for a peer
    ///
    /// # Errors
    ///
    /// Returns error if discovery fails
    #[tracing::instrument(skip(self), fields(peer = %peer))]
    pub async fn discover_peer_endpoint(
        &self,
        peer: &T::PeerId,
    ) -> Result<Option<std::net::SocketAddr>, T::Error> {
        tracing::info!("Discovering peer endpoint");
        let endpoint = self.transport.discover_peer_endpoint(peer).await?;
        if let Some(addr) = &endpoint {
            tracing::info!(endpoint = %addr, "Peer endpoint discovered");
        } else {
            tracing::debug!("No endpoint found for peer");
        }
        Ok(endpoint)
    }

    /// Get connection handle for sharing with media transport
    ///
    /// This allows media transport to use the same underlying connection
    /// as signaling, avoiding separate connection establishment.
    ///
    /// # Returns
    ///
    /// Returns a generic connection handle if available, `None` if the transport
    /// doesn't support connection sharing or the connection is not ready.
    #[must_use]
    pub fn get_connection_handle(&self) -> Option<Box<dyn std::any::Any>> {
        self.transport.get_connection_handle()
    }

    /// Get access to the underlying transport
    ///
    /// Useful for accessing transport-specific methods and state.
    #[must_use]
    pub fn transport(&self) -> &std::sync::Arc<T> {
        &self.transport
    }
}

/// Helper function to extract message type for tracing
fn message_type(msg: &SignalingMessage) -> &'static str {
    match msg {
        // Legacy WebRTC
        SignalingMessage::Offer { .. } => "Offer",
        SignalingMessage::Answer { .. } => "Answer",
        SignalingMessage::IceCandidate { .. } => "IceCandidate",
        SignalingMessage::IceComplete { .. } => "IceComplete",
        // QUIC-native
        SignalingMessage::CapabilityExchange { .. } => "CapabilityExchange",
        SignalingMessage::ConnectionConfirm { .. } => "ConnectionConfirm",
        SignalingMessage::ConnectionReady { .. } => "ConnectionReady",
        // Common
        SignalingMessage::Bye { .. } => "Bye",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    // Mock transport for testing
    struct MockTransport {
        messages: Mutex<VecDeque<(String, SignalingMessage)>>,
    }

    #[derive(Debug)]
    struct MockError;

    impl std::fmt::Display for MockError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Mock error")
        }
    }

    impl std::error::Error for MockError {}

    impl MockTransport {
        fn new() -> Self {
            Self {
                messages: Mutex::new(VecDeque::new()),
            }
        }

        fn add_message(&self, peer: String, message: SignalingMessage) {
            self.messages.lock().unwrap().push_back((peer, message));
        }
    }

    #[async_trait]
    impl SignalingTransport for MockTransport {
        type PeerId = String;
        type Error = MockError;

        async fn send_message(
            &self,
            peer: &String,
            message: SignalingMessage,
        ) -> Result<(), MockError> {
            self.messages
                .lock()
                .unwrap()
                .push_back((peer.clone(), message));
            Ok(())
        }

        async fn receive_message(&self) -> Result<(String, SignalingMessage), MockError> {
            if let Some((peer, message)) = self.messages.lock().unwrap().pop_front() {
                Ok((peer, message))
            } else {
                Err(MockError)
            }
        }

        async fn discover_peer_endpoint(
            &self,
            _peer: &String,
        ) -> Result<Option<std::net::SocketAddr>, MockError> {
            Ok(Some("127.0.0.1:8080".parse().unwrap()))
        }
    }

    #[tokio::test]
    async fn test_signaling_handler_send_message() {
        let transport = Arc::new(MockTransport::new());
        let handler = SignalingHandler::new(transport.clone());

        let message = SignalingMessage::Offer {
            session_id: "test-session".to_string(),
            sdp: "test-sdp".to_string(),
            quic_endpoint: None,
        };

        let result = handler
            .send_message(&"peer1".to_string(), message.clone())
            .await;
        assert!(result.is_ok());

        // Check that message was queued
        let received = transport.messages.lock().unwrap().pop_front();
        assert_eq!(received, Some(("peer1".to_string(), message)));
    }

    #[tokio::test]
    async fn test_signaling_handler_receive_message() {
        let transport = Arc::new(MockTransport::new());
        let handler = SignalingHandler::new(transport.clone());

        let message = SignalingMessage::Answer {
            session_id: "test-session".to_string(),
            sdp: "test-sdp".to_string(),
            quic_endpoint: None,
        };

        transport.add_message("peer1".to_string(), message.clone());

        let result = handler.receive_message().await;
        assert!(result.is_ok());
        let (peer, received_message) = result.unwrap();
        assert_eq!(peer, "peer1");
        assert_eq!(received_message, message);
    }

    #[tokio::test]
    async fn test_signaling_handler_discover_endpoint() {
        let transport = Arc::new(MockTransport::new());
        let handler = SignalingHandler::new(transport);

        let result = handler.discover_peer_endpoint(&"peer1".to_string()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("127.0.0.1:8080".parse().unwrap()));
    }

    #[tokio::test]
    async fn test_signaling_handler_get_connection_handle() {
        let transport = Arc::new(MockTransport::new());
        let handler = SignalingHandler::new(transport);

        // MockTransport doesn't provide a connection handle (uses default None)
        let handle = handler.get_connection_handle();
        assert!(handle.is_none());
    }

    #[tokio::test]
    async fn test_signaling_handler_access_transport() {
        let transport = Arc::new(MockTransport::new());
        let handler = SignalingHandler::new(transport.clone());

        // Should be able to access the underlying transport
        let handler_transport = handler.transport();
        assert!(std::ptr::eq(
            handler_transport.as_ref() as *const _,
            transport.as_ref() as *const _
        ));
    }

    #[tokio::test]
    async fn test_signaling_message_with_quic_endpoint() {
        let offer = SignalingMessage::Offer {
            session_id: "sess-123".to_string(),
            sdp: "v=0\r\n".to_string(),
            quic_endpoint: Some("192.168.1.100:4433".parse().unwrap()),
        };

        assert_eq!(offer.session_id(), "sess-123");

        let answer = SignalingMessage::Answer {
            session_id: "sess-123".to_string(),
            sdp: "v=0\r\n".to_string(),
            quic_endpoint: Some("192.168.1.101:4433".parse().unwrap()),
        };

        assert_eq!(answer.session_id(), "sess-123");
    }

    #[test]
    fn test_capability_exchange_message() {
        let cap_exchange = SignalingMessage::CapabilityExchange {
            session_id: "quic-sess-1".to_string(),
            audio: true,
            video: true,
            data_channel: false,
            max_bandwidth_kbps: 2500,
            quic_endpoint: Some("192.168.1.100:4433".parse().unwrap()),
        };

        assert_eq!(cap_exchange.session_id(), "quic-sess-1");
        assert!(cap_exchange.is_quic_native());
        assert!(!cap_exchange.is_legacy_webrtc());
    }

    #[test]
    fn test_connection_confirm_message() {
        let confirm = SignalingMessage::ConnectionConfirm {
            session_id: "quic-sess-1".to_string(),
            audio: true,
            video: true,
            data_channel: false,
            max_bandwidth_kbps: 2500,
            quic_endpoint: Some("192.168.1.101:4433".parse().unwrap()),
        };

        assert_eq!(confirm.session_id(), "quic-sess-1");
        assert!(confirm.is_quic_native());
        assert!(!confirm.is_legacy_webrtc());
    }

    #[test]
    fn test_connection_ready_message() {
        let ready = SignalingMessage::ConnectionReady {
            session_id: "quic-sess-1".to_string(),
        };

        assert_eq!(ready.session_id(), "quic-sess-1");
        assert!(ready.is_quic_native());
        assert!(!ready.is_legacy_webrtc());
    }

    #[test]
    fn test_legacy_message_detection() {
        let offer = SignalingMessage::Offer {
            session_id: "legacy-1".to_string(),
            sdp: "v=0".to_string(),
            quic_endpoint: None,
        };

        assert!(offer.is_legacy_webrtc());
        assert!(!offer.is_quic_native());

        let ice = SignalingMessage::IceCandidate {
            session_id: "legacy-1".to_string(),
            candidate: "candidate:1".to_string(),
            sdp_mid: None,
            sdp_mline_index: None,
        };

        assert!(ice.is_legacy_webrtc());
        assert!(!ice.is_quic_native());
    }

    #[test]
    fn test_bye_message_classification() {
        let bye = SignalingMessage::Bye {
            session_id: "any-1".to_string(),
            reason: Some("user ended call".to_string()),
        };

        assert_eq!(bye.session_id(), "any-1");
        // Bye is neither legacy nor QUIC-native specific
        assert!(!bye.is_legacy_webrtc());
        assert!(!bye.is_quic_native());
    }

    #[test]
    fn test_capability_exchange_serialization() {
        let msg = SignalingMessage::CapabilityExchange {
            session_id: "test".to_string(),
            audio: true,
            video: false,
            data_channel: true,
            max_bandwidth_kbps: 1000,
            quic_endpoint: None,
        };

        let serialized = serde_json::to_string(&msg).unwrap();
        assert!(serialized.contains("\"type\":\"capability_exchange\""));
        assert!(serialized.contains("\"audio\":true"));
        assert!(serialized.contains("\"video\":false"));
        assert!(serialized.contains("\"data_channel\":true"));

        let deserialized: SignalingMessage = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, msg);
    }
}
