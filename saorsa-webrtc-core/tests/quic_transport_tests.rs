//! TDD tests for QUIC transport integration
//!
//! This module contains tests for the signaling transport layer:
//! - Real transport tests use AntQuicTransport (for basic creation/start)
//! - Mock transport tests verify send/receive logic without network dependencies

use saorsa_webrtc_core::signaling::{SignalingMessage, SignalingTransport};
use saorsa_webrtc_core::transport::{AntQuicTransport, TransportConfig};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};

// ============================================================================
// Mock Transport for Unit Testing (no network required)
// ============================================================================

/// Type alias for message channel sender
type MessageSender = mpsc::Sender<(String, SignalingMessage)>;
/// Type alias for message channel receiver
type MessageReceiver = mpsc::Receiver<(String, SignalingMessage)>;
/// Type alias for peer connection map
type PeerMap = HashMap<String, MessageSender>;

/// A mock transport that uses in-memory channels for testing
struct MockTransport {
    /// Our peer ID
    local_id: String,
    /// Sender to deliver messages to us
    tx: MessageSender,
    /// Receiver for incoming messages
    rx: Mutex<MessageReceiver>,
    /// Map of peer_id -> their tx channel (so we can send to them)
    peers: Arc<Mutex<PeerMap>>,
    /// Connection state
    connected: Mutex<bool>,
}

impl MockTransport {
    fn new(local_id: &str) -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            local_id: local_id.to_string(),
            tx,
            rx: Mutex::new(rx),
            peers: Arc::new(Mutex::new(HashMap::new())),
            connected: Mutex::new(false),
        }
    }

    /// Connect this transport to another mock transport
    async fn connect_to(&self, peer_id: &str, peer_tx: MessageSender) {
        self.peers.lock().await.insert(peer_id.to_string(), peer_tx);
        *self.connected.lock().await = true;
    }

    /// Get the sender for this transport (so others can connect to us)
    fn get_tx(&self) -> MessageSender {
        self.tx.clone()
    }

    async fn is_connected(&self) -> bool {
        *self.connected.lock().await
    }
}

#[async_trait::async_trait]
impl SignalingTransport for MockTransport {
    type PeerId = String;
    type Error = std::io::Error;

    async fn send_message(
        &self,
        peer: &String,
        message: SignalingMessage,
    ) -> Result<(), Self::Error> {
        let peers = self.peers.lock().await;
        if let Some(tx) = peers.get(peer) {
            tx.send((self.local_id.clone(), message))
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e.to_string()))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                format!("No connection to peer: {}", peer),
            ))
        }
    }

    async fn receive_message(&self) -> Result<(String, SignalingMessage), Self::Error> {
        let mut rx = self.rx.lock().await;
        rx.recv().await.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::ConnectionReset, "Channel closed")
        })
    }

    async fn discover_peer_endpoint(
        &self,
        _peer: &String,
    ) -> Result<Option<std::net::SocketAddr>, Self::Error> {
        Ok(Some("127.0.0.1:8080".parse().unwrap()))
    }
}

/// Helper to connect two mock transports bidirectionally
async fn connect_peers(t1: &MockTransport, t1_id: &str, t2: &MockTransport, t2_id: &str) {
    // t1 can send to t2, t2 can send to t1
    t1.connect_to(t2_id, t2.get_tx()).await;
    t2.connect_to(t1_id, t1.get_tx()).await;
}

#[tokio::test]
async fn test_transport_creation() {
    let config = TransportConfig::default();
    let transport = AntQuicTransport::new(config);
    assert!(!transport.is_connected().await);
}

#[tokio::test]
async fn test_transport_connect() {
    let config = TransportConfig::default();
    let mut transport = AntQuicTransport::new(config);

    // Start the transport
    transport.start().await.expect("Failed to start transport");

    // Should be able to get local address
    let addr = transport
        .local_addr()
        .await
        .expect("Should have local address");
    assert!(addr.port() > 0);
}

// ============================================================================
// Mock Transport Tests (fast, no network)
// ============================================================================

#[tokio::test]
async fn test_mock_transport_send_receive() {
    // Create two mock transports
    let transport1 = MockTransport::new("peer1");
    let transport2 = MockTransport::new("peer2");

    // Connect them bidirectionally
    connect_peers(&transport1, "peer1", &transport2, "peer2").await;

    assert!(transport1.is_connected().await);
    assert!(transport2.is_connected().await);

    // Send a message from transport1 to transport2
    let message = SignalingMessage::Offer {
        session_id: "test-session".to_string(),
        sdp: "v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\n".to_string(),
        quic_endpoint: None,
    };

    transport1
        .send_message(&"peer2".to_string(), message.clone())
        .await
        .expect("Failed to send message");

    // Receive the message on transport2
    let (received_peer, received_msg) =
        tokio::time::timeout(Duration::from_secs(1), transport2.receive_message())
            .await
            .expect("Timeout waiting for message")
            .expect("Failed to receive message");

    // Verify message content
    assert_eq!(received_peer, "peer1");
    assert_eq!(received_msg.session_id(), message.session_id());
}

#[tokio::test]
async fn test_mock_transport_multiple_peers() {
    // Central hub connects to multiple peers
    let central = MockTransport::new("central");
    let peer1 = MockTransport::new("peer1");
    let peer2 = MockTransport::new("peer2");

    // Connect peers to central
    connect_peers(&peer1, "peer1", &central, "central").await;
    connect_peers(&peer2, "peer2", &central, "central").await;

    assert!(central.is_connected().await);
    assert!(peer1.is_connected().await);
    assert!(peer2.is_connected().await);

    // Send from peer1 to central
    let msg1 = SignalingMessage::Offer {
        session_id: "session-1".to_string(),
        sdp: "sdp-1".to_string(),
        quic_endpoint: None,
    };
    peer1
        .send_message(&"central".to_string(), msg1.clone())
        .await
        .expect("Failed to send");

    // Send from peer2 to central
    let msg2 = SignalingMessage::Answer {
        session_id: "session-2".to_string(),
        sdp: "sdp-2".to_string(),
        quic_endpoint: None,
    };
    peer2
        .send_message(&"central".to_string(), msg2.clone())
        .await
        .expect("Failed to send");

    // Central should receive both messages
    let mut received_sessions = Vec::new();
    for _ in 0..2 {
        let (_peer, msg) = tokio::time::timeout(Duration::from_secs(1), central.receive_message())
            .await
            .expect("Timeout")
            .expect("Failed to receive");
        received_sessions.push(msg.session_id().to_string());
    }

    // Both sessions should be received
    assert!(received_sessions.contains(&"session-1".to_string()));
    assert!(received_sessions.contains(&"session-2".to_string()));
}

#[tokio::test]
async fn test_transport_disconnect() {
    let mut transport1 = AntQuicTransport::new(TransportConfig::default());
    let mut transport2 = AntQuicTransport::new(TransportConfig::default());

    transport1
        .start()
        .await
        .expect("Failed to start transport1");
    transport2
        .start()
        .await
        .expect("Failed to start transport2");

    let addr2 = transport2.local_addr().await.expect("Should have addr2");
    let peer_id = transport1
        .connect_to_peer(addr2)
        .await
        .expect("Failed to connect");

    // Disconnect
    transport1
        .disconnect_peer(&peer_id)
        .await
        .expect("Failed to disconnect");

    // Sending should fail
    let message = SignalingMessage::Offer {
        session_id: "test".to_string(),
        sdp: "sdp".to_string(),
        quic_endpoint: None,
    };

    let result = transport1.send_message(&peer_id, message).await;
    assert!(result.is_err());
}
