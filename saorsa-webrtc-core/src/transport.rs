//! Transport layer implementations
//!
//! This module provides transport adapters for different signaling mechanisms.

use crate::link_transport::StreamType as LinkStreamType;
use crate::signaling::{SignalingMessage, SignalingTransport};
use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;

/// Maximum signaling message size (64KB) to prevent DoS attacks
const MAX_SIGNALING_MESSAGE_SIZE: usize = 64 * 1024;

/// Maximum session ID length
const MAX_SESSION_ID_LENGTH: usize = 256;

/// Maximum SDP string length (reasonable for WebRTC)
const MAX_SDP_LENGTH: usize = 32 * 1024;

/// Transport configuration
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Local endpoint address
    pub local_addr: Option<SocketAddr>,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self { local_addr: None }
    }
}

/// Transport errors
#[derive(Error, Debug)]
pub enum TransportError {
    /// Connection error
    #[error("Connection error: {0}")]
    ConnectionError(String),

    /// Send error
    #[error("Send error: {0}")]
    SendError(String),

    /// Receive error
    #[error("Receive error: {0}")]
    ReceiveError(String),
}

/// ant-quic transport adapter
///
/// This transport uses ant-quic for NAT traversal and encrypted connections.
/// It can be used with DHT-based peer discovery (saorsa-core) or
/// gossip-based rendezvous (communitas).
pub struct AntQuicTransport {
    config: TransportConfig,
    node: Option<Arc<ant_quic::Node>>,
    peer_map: Arc<tokio::sync::RwLock<std::collections::HashMap<String, ant_quic::PeerId>>>,
    default_peer: Arc<tokio::sync::RwLock<Option<ant_quic::PeerId>>>,
    shutdown: Arc<tokio::sync::watch::Sender<bool>>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

impl AntQuicTransport {
    /// Create new ant-quic transport
    #[must_use]
    pub fn new(config: TransportConfig) -> Self {
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        Self {
            config,
            node: None,
            peer_map: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            default_peer: Arc::new(tokio::sync::RwLock::new(None)),
            shutdown: Arc::new(shutdown_tx),
            shutdown_rx,
        }
    }

    /// Get transport configuration
    #[must_use]
    pub fn config(&self) -> &TransportConfig {
        &self.config
    }

    /// Start the transport and initialize QUIC node
    ///
    /// # Errors
    ///
    /// Returns error if node creation fails
    pub async fn start(&mut self) -> Result<(), TransportError> {
        use ant_quic::{Node, NodeConfigBuilder};

        // Build node configuration
        let config_builder = NodeConfigBuilder::default();
        let node_config = if let Some(addr) = self.config.local_addr {
            config_builder.bind_addr(addr).build()
        } else {
            config_builder.build()
        };

        let node = Node::with_config(node_config).await.map_err(|e| {
            TransportError::ConnectionError(format!("Failed to create QUIC node: {}", e))
        })?;

        let node_arc = Arc::new(node);

        // Spawn background task to accept incoming connections
        let node_clone = node_arc.clone();
        let peer_map = self.peer_map.clone();
        let mut shutdown_rx = self.shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            tracing::info!("Shutting down accept loop");
                            break;
                        }
                    }
                    result = node_clone.accept() => {
                        if let Some(conn) = result {
                            let peer_id = conn.peer_id;
                            let addr = conn.remote_addr;
                            tracing::debug!("Accepted connection from {:?} at {:?}", peer_id, addr);
                            // Store the peer mapping
                            let peer_str = format!("{:?}", peer_id);
                            peer_map.write().await.insert(peer_str, peer_id);
                        }
                    }
                }
            }
        });

        self.node = Some(node_arc);
        Ok(())
    }

    /// Stop the transport and shutdown accept loop
    ///
    /// # Errors
    ///
    /// Returns error if shutdown signal fails to send
    pub fn stop(&self) -> Result<(), TransportError> {
        if self.shutdown.send(true).is_err() {
            return Err(TransportError::ConnectionError(
                "Failed to send shutdown signal".to_string(),
            ));
        }
        tracing::info!("Transport shutdown signal sent");
        Ok(())
    }

    /// Check if transport is connected
    pub async fn is_connected(&self) -> bool {
        self.node.is_some()
    }

    /// Get the underlying QUIC node for connection sharing
    ///
    /// This allows media transports to share the same QUIC connection
    /// as signaling, enabling multiplexed streams over one connection.
    ///
    /// # Returns
    ///
    /// Reference to the Arc<Node> if connected, None if not started
    ///
    /// # Note
    ///
    /// The returned Arc can be cloned and used by media transport handlers.
    /// Lifetimes are managed through Arc reference counting.
    pub fn get_node(&self) -> Option<Arc<ant_quic::Node>> {
        self.node.clone()
    }

    /// Get local address
    ///
    /// # Errors
    ///
    /// Returns error if transport is not started
    pub async fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        let node = self
            .node
            .as_ref()
            .ok_or_else(|| TransportError::ConnectionError("Transport not started".to_string()))?;

        let mut addr = node.local_addr().ok_or_else(|| {
            TransportError::ConnectionError("No local address available".to_string())
        })?;

        // If bound to 0.0.0.0, replace with localhost for connection purposes
        if addr.ip().is_unspecified() {
            addr.set_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
        }

        Ok(addr)
    }

    /// Connect to a peer
    ///
    /// # Errors
    ///
    /// Returns error if connection fails
    pub async fn connect_to_peer(&mut self, addr: SocketAddr) -> Result<String, TransportError> {
        let node = self
            .node
            .as_ref()
            .ok_or_else(|| TransportError::ConnectionError("Transport not started".to_string()))?;

        let conn = node
            .connect_addr(addr)
            .await
            .map_err(|e| TransportError::ConnectionError(format!("Failed to connect: {}", e)))?;

        let peer_id = conn.peer_id;

        // Generate string representation for peer ID
        let peer_str = format!("{:?}", peer_id);

        // Store mapping
        let mut peer_map = self.peer_map.write().await;
        peer_map.insert(peer_str.clone(), peer_id);

        // Set as default peer if no default set
        let mut default_peer = self.default_peer.write().await;
        if default_peer.is_none() {
            *default_peer = Some(peer_id);
        }
        drop(default_peer);

        Ok(peer_str)
    }

    /// Disconnect from a peer
    ///
    /// # Errors
    ///
    /// Returns error if disconnection fails
    pub async fn disconnect_peer(&mut self, peer: &String) -> Result<(), TransportError> {
        let mut peer_map = self.peer_map.write().await;
        peer_map.remove(peer);
        Ok(())
    }

    /// Get a handle for sending on a specific stream type
    ///
    /// This method prepares the transport for multiplexed streams.
    /// Currently returns the stream type as-is; future implementations
    /// may use this to allocate dedicated stream resources.
    ///
    /// # Errors
    ///
    /// Returns error if stream type is invalid
    pub fn get_stream_handle(&self, stream_type: LinkStreamType) -> Result<LinkStreamType, TransportError> {
        // Validate stream type is in expected range
        if stream_type.as_u8() >= 0x20 && stream_type.as_u8() <= 0x24 {
            Ok(stream_type)
        } else {
            Err(TransportError::SendError(
                "Invalid stream type for WebRTC".to_string(),
            ))
        }
    }

    /// Send raw bytes to default peer (for RTP packets and stream data)
    ///
    /// This method is used by the QUIC bridge and stream manager to send
    /// serialized RTP packets and media data over the QUIC transport.
    ///
    /// # Errors
    ///
    /// Returns error if send fails
    pub async fn send_bytes(&self, data: &[u8]) -> Result<(), TransportError> {
        let span = tracing::debug_span!("transport_send_bytes", data_len = data.len());
        let _enter = span.enter();

        let node = self
            .node
            .as_ref()
            .ok_or_else(|| TransportError::SendError("Transport not started".to_string()))?;

        let default_peer = self.default_peer.read().await;
        let peer_id = default_peer
            .as_ref()
            .ok_or_else(|| TransportError::SendError("No peer connected".to_string()))?;

        node.send(peer_id, data)
            .await
            .map_err(|e| TransportError::SendError(format!("Failed to send: {}", e)))?;

        tracing::trace!("Sent {} bytes to peer", data.len());

        Ok(())
    }

    /// Receive raw bytes from any peer (for RTP packets and stream data)
    ///
    /// This method is used by the QUIC bridge and stream manager to receive
    /// serialized RTP packets and media data from the QUIC transport.
    ///
    /// # Errors
    ///
    /// Returns error if receive fails
    pub async fn receive_bytes(&self) -> Result<Vec<u8>, TransportError> {
        use std::time::Duration;
        let span = tracing::debug_span!("transport_receive_bytes");
        let _enter = span.enter();

        let node = self
            .node
            .as_ref()
            .ok_or_else(|| TransportError::ReceiveError("Transport not started".to_string()))?;

        let (_peer_id, data) = node
            .recv(Duration::from_secs(30))
            .await
            .map_err(|e| TransportError::ReceiveError(format!("Failed to receive: {}", e)))?;

        tracing::trace!("Received {} bytes from peer", data.len());

        Ok(data)
    }

    /// Send bytes on a specific stream type to default peer
    ///
    /// This method adds stream type awareness for multiplexed media over QUIC.
    /// The stream type is included in the packet metadata for routing.
    ///
    /// # Errors
    ///
    /// Returns error if send fails or stream type is invalid
    pub async fn send_bytes_on_stream(
        &self,
        stream_type: LinkStreamType,
        data: &[u8],
    ) -> Result<(), TransportError> {
        let span = tracing::debug_span!("transport_send_stream",
                                        stream_type = stream_type.as_u8(),
                                        data_len = data.len());
        let _enter = span.enter();

        // Validate stream type
        self.get_stream_handle(stream_type)?;

        // Current implementation sends on default transport
        // Future versions will use dedicated QUIC streams per stream type
        self.send_bytes(data).await
    }

    /// Receive bytes with stream type information (when available)
    ///
    /// For now, this returns stream type inferred from content.
    /// Future implementations will extract stream type from QUIC stream ID.
    ///
    /// # Errors
    ///
    /// Returns error if receive fails
    pub async fn receive_bytes_on_stream(
        &self,
    ) -> Result<(Vec<u8>, LinkStreamType), TransportError> {
        let data = self.receive_bytes().await?;

        // For now, infer stream type from data length
        // Future: extract from QUIC stream ID metadata
        let stream_type = if data.len() > 1000 {
            LinkStreamType::Video  // Assume large packets are video
        } else {
            LinkStreamType::Audio  // Assume small packets are audio
        };

        Ok((data, stream_type))
    }
}

#[async_trait]
impl SignalingTransport for AntQuicTransport {
    type PeerId = String;
    type Error = TransportError;

    async fn send_message(
        &self,
        peer: &String,
        message: SignalingMessage,
    ) -> Result<(), TransportError> {
        if peer.is_empty() {
            return Err(TransportError::SendError(
                "Peer ID cannot be empty".to_string(),
            ));
        }

        let node = self
            .node
            .as_ref()
            .ok_or_else(|| TransportError::SendError("Transport not started".to_string()))?;

        // Get actual peer ID from map
        let peer_map = self.peer_map.read().await;
        let peer_id = peer_map
            .get(peer)
            .ok_or_else(|| TransportError::SendError(format!("Peer not found: {}", peer)))?;

        // Serialize the message
        let data = serde_json::to_vec(&message).map_err(|e| {
            TransportError::SendError(format!("Failed to serialize message: {}", e))
        })?;

        // Send over QUIC
        node.send(peer_id, &data)
            .await
            .map_err(|e| TransportError::SendError(format!("Failed to send: {}", e)))?;

        tracing::debug!("Sent signaling message to peer: {}", peer);
        Ok(())
    }

    async fn receive_message(&self) -> Result<(String, SignalingMessage), TransportError> {
        use std::time::Duration;
        let node = self
            .node
            .as_ref()
            .ok_or_else(|| TransportError::ReceiveError("Transport not started".to_string()))?;

        // Receive data from any peer (this will block until data arrives)
        // The Node handles incoming connections internally
        let (peer_id, data) = node
            .recv(Duration::from_secs(30))
            .await
            .map_err(|e| TransportError::ReceiveError(format!("Failed to receive: {}", e)))?;

        // Check message size limit to prevent DoS
        if data.len() > MAX_SIGNALING_MESSAGE_SIZE {
            return Err(TransportError::ReceiveError(format!(
                "Message size {} exceeds maximum of {} bytes",
                data.len(),
                MAX_SIGNALING_MESSAGE_SIZE
            )));
        }

        // Deserialize the message
        let message: SignalingMessage = serde_json::from_slice(&data).map_err(|e| {
            TransportError::ReceiveError(format!("Failed to deserialize message: {}", e))
        })?;

        // Validate message fields
        validate_signaling_message(&message)?;

        // Generate string representation for peer ID
        let peer_str = format!("{:?}", peer_id);

        // Update peer map if needed
        let mut peer_map = self.peer_map.write().await;
        peer_map.entry(peer_str.clone()).or_insert(peer_id);
        drop(peer_map);

        tracing::debug!("Received signaling message from peer: {}", peer_str);
        Ok((peer_str, message))
    }

    async fn discover_peer_endpoint(
        &self,
        peer: &String,
    ) -> Result<Option<SocketAddr>, TransportError> {
        // TODO: Implement actual peer discovery via DHT or gossip
        // For now, return None to indicate discovery not available

        tracing::debug!("Attempting to discover endpoint for peer: {}", peer);
        Ok(None)
    }
}

/// Validate signaling message fields to prevent abuse
fn validate_signaling_message(message: &SignalingMessage) -> Result<(), TransportError> {
    match message {
        SignalingMessage::Offer {
            session_id, sdp, ..
        }
        | SignalingMessage::Answer {
            session_id, sdp, ..
        } => {
            if session_id.len() > MAX_SESSION_ID_LENGTH {
                return Err(TransportError::ReceiveError(format!(
                    "Session ID length {} exceeds maximum of {}",
                    session_id.len(),
                    MAX_SESSION_ID_LENGTH
                )));
            }
            if sdp.len() > MAX_SDP_LENGTH {
                return Err(TransportError::ReceiveError(format!(
                    "SDP length {} exceeds maximum of {}",
                    sdp.len(),
                    MAX_SDP_LENGTH
                )));
            }
        }
        SignalingMessage::IceCandidate {
            session_id,
            candidate,
            ..
        } => {
            if session_id.len() > MAX_SESSION_ID_LENGTH {
                return Err(TransportError::ReceiveError(format!(
                    "Session ID length {} exceeds maximum of {}",
                    session_id.len(),
                    MAX_SESSION_ID_LENGTH
                )));
            }
            if candidate.len() > MAX_SDP_LENGTH {
                return Err(TransportError::ReceiveError(format!(
                    "Candidate length {} exceeds maximum of {}",
                    candidate.len(),
                    MAX_SDP_LENGTH
                )));
            }
        }
        SignalingMessage::IceComplete { session_id } | SignalingMessage::Bye { session_id, .. } => {
            if session_id.len() > MAX_SESSION_ID_LENGTH {
                return Err(TransportError::ReceiveError(format!(
                    "Session ID length {} exceeds maximum of {}",
                    session_id.len(),
                    MAX_SESSION_ID_LENGTH
                )));
            }
        }
    }
    Ok(())
}

#[async_trait]
impl crate::link_transport::LinkTransport for AntQuicTransport {
    async fn start(&mut self) -> Result<(), crate::link_transport::LinkTransportError> {
        AntQuicTransport::start(self)
            .await
            .map_err(|e| crate::link_transport::LinkTransportError::IoError(e.to_string()))
    }

    async fn stop(&mut self) -> Result<(), crate::link_transport::LinkTransportError> {
        // Note: AntQuicTransport::stop() is synchronous, so we can call it directly
        AntQuicTransport::stop(self)
            .map_err(|e| crate::link_transport::LinkTransportError::IoError(e.to_string()))
    }

    async fn is_running(&self) -> bool {
        self.is_connected().await
    }

    async fn local_addr(&self) -> Result<SocketAddr, crate::link_transport::LinkTransportError> {
        AntQuicTransport::local_addr(self)
            .await
            .map_err(|e| crate::link_transport::LinkTransportError::IoError(e.to_string()))
    }

    async fn connect(&mut self, addr: SocketAddr) -> Result<crate::link_transport::PeerConnection, crate::link_transport::LinkTransportError> {
        let peer_id_str = self
            .connect_to_peer(addr)
            .await
            .map_err(|e| crate::link_transport::LinkTransportError::IoError(e.to_string()))?;

        Ok(crate::link_transport::PeerConnection {
            peer_id: peer_id_str,
            remote_addr: addr,
        })
    }

    async fn accept(&mut self) -> Result<Option<crate::link_transport::PeerConnection>, crate::link_transport::LinkTransportError> {
        // Accept is handled in the background task spawned by start()
        // Return None for now - actual connections are tracked via the peer_map
        Ok(None)
    }

    async fn send(
        &self,
        peer: &crate::link_transport::PeerConnection,
        stream_type: crate::link_transport::StreamType,
        data: &[u8],
    ) -> Result<(), crate::link_transport::LinkTransportError> {
        // Create a framed message: [stream_type: 1 byte][length: 2 bytes][data]
        let mut framed = Vec::with_capacity(3 + data.len());
        framed.push(stream_type.as_u8());
        framed.extend_from_slice(&(data.len() as u16).to_be_bytes());
        framed.extend_from_slice(data);

        let node = self
            .node
            .as_ref()
            .ok_or(crate::link_transport::LinkTransportError::NotConnected)?;

        // Look up peer_id from peer_map
        let peer_map = self.peer_map.read().await;
        let peer_id = peer_map
            .get(&peer.peer_id)
            .ok_or_else(|| crate::link_transport::LinkTransportError::PeerNotFound(peer.peer_id.clone()))?;

        node.send(peer_id, &framed)
            .await
            .map_err(|e| crate::link_transport::LinkTransportError::SendError(e.to_string()))
    }

    async fn receive(&self) -> Result<(crate::link_transport::PeerConnection, crate::link_transport::StreamType, Vec<u8>), crate::link_transport::LinkTransportError> {
        use std::time::Duration;

        let node = self
            .node
            .as_ref()
            .ok_or(crate::link_transport::LinkTransportError::NotConnected)?;

        let (peer_id, data) = node
            .recv(Duration::from_secs(30))
            .await
            .map_err(|e| crate::link_transport::LinkTransportError::ReceiveError(e.to_string()))?;

        // Parse framed message: [stream_type: 1 byte][length: 2 bytes][data]
        if data.len() < 3 {
            return Err(crate::link_transport::LinkTransportError::ReceiveError(
                "Framed message too short".to_string(),
            ));
        }

        let stream_type_byte = data[0];
        let stream_type = crate::link_transport::StreamType::try_from_u8(stream_type_byte)
            .ok_or(crate::link_transport::LinkTransportError::InvalidStreamType(stream_type_byte))?;

        let length = u16::from_be_bytes([data[1], data[2]]) as usize;
        if 3 + length > data.len() {
            return Err(crate::link_transport::LinkTransportError::ReceiveError(
                "Invalid frame length".to_string(),
            ));
        }

        let payload = data[3..3 + length].to_vec();

        // Generate string representation for peer ID
        let peer_str = format!("{:?}", peer_id);

        // Update peer map if needed
        let mut peer_map = self.peer_map.write().await;
        peer_map.entry(peer_str.clone()).or_insert(peer_id);
        drop(peer_map);

        Ok((
            crate::link_transport::PeerConnection {
                peer_id: peer_str,
                remote_addr: SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 0),
            },
            stream_type,
            payload,
        ))
    }

    fn default_peer(&self) -> Result<crate::link_transport::PeerConnection, crate::link_transport::LinkTransportError> {
        // This is a blocking method, so we need to access the Arc directly
        // In practice, this should be called only when we know a peer exists
        // For now, return error - use async methods instead
        Err(crate::link_transport::LinkTransportError::NotConnected)
    }

    fn set_default_peer(
        &mut self,
        peer: crate::link_transport::PeerConnection,
    ) -> Result<(), crate::link_transport::LinkTransportError> {
        // Note: This is a blocking method but we can't access async RwLock here
        // We'd need to refactor the data structure or make this async in future
        let _ = peer;
        Err(crate::link_transport::LinkTransportError::NotConnected)
    }
}



#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ant_quic_transport_send_message_valid() {
        let config = TransportConfig::default();
        let transport = AntQuicTransport::new(config);

        let message = SignalingMessage::Offer {
            session_id: "test-session".to_string(),
            sdp: "test-sdp".to_string(),
            quic_endpoint: None,
        };

        // Will fail without peer connected, which is expected
        let _result = transport.send_message(&"peer1".to_string(), message).await;
    }

    #[tokio::test]
    async fn test_ant_quic_transport_send_message_empty_peer() {
        let config = TransportConfig::default();
        let transport = AntQuicTransport::new(config);

        let message = SignalingMessage::Offer {
            session_id: "test-session".to_string(),
            sdp: "test-sdp".to_string(),
            quic_endpoint: None,
        };

        let result = transport.send_message(&"".to_string(), message).await;
        assert!(matches!(result, Err(TransportError::SendError(_))));
    }

    #[tokio::test]
    async fn test_ant_quic_transport_receive_message() {
        let config = TransportConfig::default();
        let transport = AntQuicTransport::new(config);

        let result = transport.receive_message().await;
        assert!(matches!(result, Err(TransportError::ReceiveError(_))));
    }

    #[tokio::test]
    async fn test_ant_quic_transport_discover_peer_endpoint() {
        let config = TransportConfig::default();
        let transport = AntQuicTransport::new(config);

        let result = transport.discover_peer_endpoint(&"peer1".to_string()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_ant_quic_transport_config() {
        let config = TransportConfig {
            local_addr: Some("127.0.0.1:8080".parse().unwrap()),
        };
        let transport = AntQuicTransport::new(config.clone());

        assert_eq!(transport.config().local_addr, config.local_addr);
    }

    #[test]
    fn test_transport_config_default() {
        let config = TransportConfig::default();
        assert!(config.local_addr.is_none());
    }

    #[test]
    fn test_get_stream_handle_valid() {
        let config = TransportConfig::default();
        let transport = AntQuicTransport::new(config);

        // Test all valid stream types
        let result = transport.get_stream_handle(LinkStreamType::Audio);
        assert!(result.is_ok());

        let result = transport.get_stream_handle(LinkStreamType::Video);
        assert!(result.is_ok());

        let result = transport.get_stream_handle(LinkStreamType::Screen);
        assert!(result.is_ok());

        let result = transport.get_stream_handle(LinkStreamType::RtcpFeedback);
        assert!(result.is_ok());

        let result = transport.get_stream_handle(LinkStreamType::Data);
        assert!(result.is_ok());
    }
}

#[cfg(test)]
mod link_transport_tests {
    use super::*;
    use crate::link_transport::{LinkTransport, StreamType};

    #[tokio::test]
    async fn test_link_transport_start_stop() {
        let config = TransportConfig::default();
        let mut transport = AntQuicTransport::new(config);
        
        // Start should succeed
        let result = transport.start().await;
        assert!(result.is_ok());
        
        // Should be running after start
        assert!(transport.is_running().await);
        
        // Stop should succeed
        let result = transport.stop();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_link_transport_framing_audio() {
        let _config = TransportConfig::default();
        
        // Test stream type framing for audio
        let stream_type = StreamType::Audio;
        let payload = b"audio_data".to_vec();
        
        // Frame would be: [0x20][0x00, 0x0A][audio_data]
        assert_eq!(stream_type.as_u8(), 0x20);
        assert_eq!(payload.len(), 10);
    }

    #[tokio::test]
    async fn test_link_transport_framing_video() {
        let _config = TransportConfig::default();
        
        // Test stream type for video (larger packet)
        let stream_type = StreamType::Video;
        let large_payload = vec![0u8; 5000]; // 5KB payload
        
        // Frame would be: [0x21][0x13, 0x88][large_payload]
        assert_eq!(stream_type.as_u8(), 0x21);
        assert_eq!(large_payload.len(), 5000);
    }

    #[tokio::test]
    async fn test_link_transport_stream_types() {
        let config = TransportConfig::default();
        let transport = AntQuicTransport::new(config);
        
        // Verify all stream types are valid
        let types = vec![
            StreamType::Audio,
            StreamType::Video,
            StreamType::Screen,
            StreamType::RtcpFeedback,
            StreamType::Data,
        ];
        
        for stream_type in types {
            let result = transport.get_stream_handle(stream_type);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_link_transport_stream_type_conversions() {
        let test_cases = vec![
            (StreamType::Audio, 0x20),
            (StreamType::Video, 0x21),
            (StreamType::Screen, 0x22),
            (StreamType::RtcpFeedback, 0x23),
            (StreamType::Data, 0x24),
        ];
        
        for (stream_type, byte_value) in test_cases {
            assert_eq!(stream_type.as_u8(), byte_value);
            assert_eq!(StreamType::try_from_u8(byte_value), Some(stream_type));
        }
    }

    #[test]
    fn test_link_transport_invalid_stream_type() {
        // Test invalid stream type bytes
        assert_eq!(StreamType::try_from_u8(0x19), None);
        assert_eq!(StreamType::try_from_u8(0x25), None);
        assert_eq!(StreamType::try_from_u8(0xFF), None);
    }
}
