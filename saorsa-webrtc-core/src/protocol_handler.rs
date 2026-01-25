//! WebRTC protocol handler for SharedTransport integration
//!
//! Implements the `ProtocolHandler` trait from saorsa-transport to handle
//! WebRTC-specific stream types over the shared transport layer.

use ant_quic::{
    LinkError as TransportError, LinkResult as TransportResult, PeerId, ProtocolHandler, StreamType,
};
use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use thiserror::Error;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, trace, warn};

use crate::quic_bridge::RtpPacket;
use crate::signaling::SignalingMessage;

/// Errors specific to WebRTC protocol handling.
#[derive(Debug, Error)]
pub enum WebRtcHandlerError {
    /// Failed to deserialize signaling message.
    #[error("failed to deserialize signaling message: {0}")]
    SignalingDeserialize(String),

    /// Failed to deserialize media packet.
    #[error("failed to deserialize media packet: {0}")]
    MediaDeserialize(String),

    /// Failed to serialize response.
    #[error("failed to serialize response: {0}")]
    Serialize(String),

    /// Channel send error.
    #[error("failed to send to channel: {0}")]
    ChannelSend(String),
}

/// Incoming WebRTC message types.
#[derive(Debug, Clone)]
pub enum WebRtcIncoming {
    /// Signaling message (SDP offers, answers, ICE candidates).
    Signal {
        /// Remote peer ID.
        peer: PeerId,
        /// The signaling message.
        message: SignalingMessage,
    },
    /// Media packet (RTP audio/video).
    Media {
        /// Remote peer ID.
        peer: PeerId,
        /// The RTP packet.
        packet: RtpPacket,
    },
    /// Data channel message.
    Data {
        /// Remote peer ID.
        peer: PeerId,
        /// Channel ID.
        channel_id: u32,
        /// The data payload.
        data: Bytes,
    },
}

/// Configuration for the WebRTC protocol handler.
#[derive(Debug, Clone)]
pub struct WebRtcHandlerConfig {
    /// Buffer size for incoming signal messages.
    pub signal_buffer_size: usize,
    /// Buffer size for incoming media packets.
    pub media_buffer_size: usize,
    /// Buffer size for incoming data channel messages.
    pub data_buffer_size: usize,
}

impl Default for WebRtcHandlerConfig {
    fn default() -> Self {
        Self {
            signal_buffer_size: 256,
            media_buffer_size: 1024,
            data_buffer_size: 512,
        }
    }
}

/// WebRTC protocol handler for SharedTransport.
///
/// Routes incoming streams to the appropriate WebRTC subsystem based on
/// stream type:
/// - `WebRtcSignal` (0x20): SDP offers/answers, ICE candidates
/// - `WebRtcMedia` (0x21): RTP packets for audio/video
/// - `WebRtcData` (0x22): Data channel messages
pub struct WebRtcProtocolHandler {
    /// Channel for incoming signaling messages.
    signal_tx: mpsc::Sender<WebRtcIncoming>,
    /// Channel for incoming media packets.
    media_tx: mpsc::Sender<WebRtcIncoming>,
    /// Channel for incoming data channel messages.
    data_tx: mpsc::Sender<WebRtcIncoming>,

    /// Per-peer session state.
    sessions: RwLock<HashMap<PeerId, PeerSession>>,

    /// Shutdown flag.
    shutdown: RwLock<bool>,
}

/// State for a peer's WebRTC session.
#[derive(Debug, Default)]
struct PeerSession {
    /// Active data channel IDs.
    data_channels: Vec<u32>,
    /// Messages received count.
    messages_received: u64,
    /// Last activity timestamp.
    last_activity: Option<std::time::Instant>,
}

impl WebRtcProtocolHandler {
    /// Create a new WebRTC protocol handler.
    ///
    /// Returns the handler and receivers for each message type.
    pub fn new(
        config: WebRtcHandlerConfig,
    ) -> (
        Self,
        mpsc::Receiver<WebRtcIncoming>,
        mpsc::Receiver<WebRtcIncoming>,
        mpsc::Receiver<WebRtcIncoming>,
    ) {
        let (signal_tx, signal_rx) = mpsc::channel(config.signal_buffer_size);
        let (media_tx, media_rx) = mpsc::channel(config.media_buffer_size);
        let (data_tx, data_rx) = mpsc::channel(config.data_buffer_size);

        let handler = Self {
            signal_tx,
            media_tx,
            data_tx,
            sessions: RwLock::new(HashMap::new()),
            shutdown: RwLock::new(false),
        };

        (handler, signal_rx, media_rx, data_rx)
    }

    /// Create with default configuration.
    pub fn with_defaults() -> (
        Self,
        mpsc::Receiver<WebRtcIncoming>,
        mpsc::Receiver<WebRtcIncoming>,
        mpsc::Receiver<WebRtcIncoming>,
    ) {
        Self::new(WebRtcHandlerConfig::default())
    }

    /// Handle incoming signaling message.
    async fn handle_signal(&self, peer: PeerId, data: Bytes) -> TransportResult<Option<Bytes>> {
        trace!(peer = ?peer, size = data.len(), "Processing WebRTC signal");

        // Deserialize the signaling message
        let message: SignalingMessage = serde_json::from_slice(&data).map_err(|e| {
            TransportError::Internal(format!("Failed to deserialize signaling message: {}", e))
        })?;

        debug!(
            peer = ?peer,
            session_id = %message.session_id(),
            "Received signaling message"
        );

        // Update session state
        {
            let mut sessions = self.sessions.write().await;
            let session = sessions.entry(peer).or_default();
            session.messages_received += 1;
            session.last_activity = Some(std::time::Instant::now());
        }

        // Send to signal channel
        self.signal_tx
            .send(WebRtcIncoming::Signal { peer, message })
            .await
            .map_err(|e| {
                TransportError::Internal(format!("Failed to send to signal channel: {}", e))
            })?;

        // Signaling typically expects a response, but we handle that asynchronously
        Ok(None)
    }

    /// Handle incoming media packet.
    async fn handle_media(&self, peer: PeerId, data: Bytes) -> TransportResult<Option<Bytes>> {
        trace!(peer = ?peer, size = data.len(), "Processing WebRTC media");

        // Deserialize the RTP packet
        let packet = RtpPacket::from_bytes(&data).map_err(|e| {
            TransportError::Internal(format!("Failed to deserialize RTP packet: {}", e))
        })?;

        trace!(
            peer = ?peer,
            stream_type = ?packet.stream_type,
            seq = packet.sequence_number,
            "Received media packet"
        );

        // Update session state
        {
            let mut sessions = self.sessions.write().await;
            let session = sessions.entry(peer).or_default();
            session.messages_received += 1;
            session.last_activity = Some(std::time::Instant::now());
        }

        // Send to media channel - use try_send for non-blocking media
        match self
            .media_tx
            .try_send(WebRtcIncoming::Media { peer, packet })
        {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                warn!(peer = ?peer, "Media channel full, dropping packet");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(TransportError::Shutdown);
            }
        }

        // Media packets do not require responses
        Ok(None)
    }

    /// Handle incoming data channel message.
    async fn handle_data(&self, peer: PeerId, data: Bytes) -> TransportResult<Option<Bytes>> {
        trace!(peer = ?peer, size = data.len(), "Processing WebRTC data");

        // Data channel format: 4-byte channel ID + payload
        if data.len() < 4 {
            return Err(TransportError::Internal(
                "Data channel message too short".into(),
            ));
        }

        let channel_id = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let payload = data.slice(4..);

        debug!(
            peer = ?peer,
            channel_id = channel_id,
            payload_size = payload.len(),
            "Received data channel message"
        );

        // Update session state
        {
            let mut sessions = self.sessions.write().await;
            let session = sessions.entry(peer).or_default();
            session.messages_received += 1;
            session.last_activity = Some(std::time::Instant::now());
            if !session.data_channels.contains(&channel_id) {
                session.data_channels.push(channel_id);
            }
        }

        // Send to data channel
        self.data_tx
            .send(WebRtcIncoming::Data {
                peer,
                channel_id,
                data: payload,
            })
            .await
            .map_err(|e| {
                TransportError::Internal(format!("Failed to send to data channel: {}", e))
            })?;

        // Data channel messages may or may not require responses
        Ok(None)
    }

    /// Get number of active sessions.
    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Get session info for a peer.
    pub async fn get_session_stats(&self, peer: &PeerId) -> Option<(u64, Vec<u32>)> {
        let sessions = self.sessions.read().await;
        sessions
            .get(peer)
            .map(|s| (s.messages_received, s.data_channels.clone()))
    }

    /// Remove a peer session.
    pub async fn remove_session(&self, peer: &PeerId) {
        let mut sessions = self.sessions.write().await;
        if sessions.remove(peer).is_some() {
            debug!(peer = ?peer, "Removed WebRTC session");
        }
    }
}

#[async_trait]
impl ProtocolHandler for WebRtcProtocolHandler {
    fn stream_types(&self) -> &[StreamType] {
        StreamType::webrtc_types()
    }

    async fn handle_stream(
        &self,
        peer: PeerId,
        stream_type: StreamType,
        data: Bytes,
    ) -> TransportResult<Option<Bytes>> {
        // Check shutdown flag
        if *self.shutdown.read().await {
            return Err(TransportError::Shutdown);
        }

        match stream_type {
            StreamType::WebRtcSignal => self.handle_signal(peer, data).await,
            StreamType::WebRtcMedia => self.handle_media(peer, data).await,
            StreamType::WebRtcData => self.handle_data(peer, data).await,
            _ => {
                error!(stream_type = %stream_type, "Unexpected stream type in WebRTC handler");
                Err(TransportError::Internal(format!(
                    "Unknown stream type: {}",
                    stream_type
                )))
            }
        }
    }

    async fn handle_datagram(
        &self,
        peer: PeerId,
        stream_type: StreamType,
        data: Bytes,
    ) -> TransportResult<()> {
        // Datagrams are used for unreliable media (e.g., low-priority video frames)
        if stream_type == StreamType::WebRtcMedia {
            trace!(peer = ?peer, size = data.len(), "Received media datagram");

            // Try to deserialize and forward, but do not fail on errors for datagrams
            if let Ok(packet) = RtpPacket::from_bytes(&data) {
                let _ = self
                    .media_tx
                    .try_send(WebRtcIncoming::Media { peer, packet });
            }
        }
        Ok(())
    }

    async fn shutdown(&self) -> TransportResult<()> {
        debug!("Shutting down WebRTC protocol handler");

        let mut shutdown = self.shutdown.write().await;
        *shutdown = true;

        // Clear sessions
        self.sessions.write().await.clear();

        Ok(())
    }

    fn name(&self) -> &str {
        "WebRtcProtocolHandler"
    }
}

/// Builder for creating WebRtcProtocolHandler with custom configuration.
pub struct WebRtcProtocolHandlerBuilder {
    config: WebRtcHandlerConfig,
}

impl WebRtcProtocolHandlerBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: WebRtcHandlerConfig::default(),
        }
    }

    /// Set signal buffer size.
    pub fn signal_buffer_size(mut self, size: usize) -> Self {
        self.config.signal_buffer_size = size;
        self
    }

    /// Set media buffer size.
    pub fn media_buffer_size(mut self, size: usize) -> Self {
        self.config.media_buffer_size = size;
        self
    }

    /// Set data buffer size.
    pub fn data_buffer_size(mut self, size: usize) -> Self {
        self.config.data_buffer_size = size;
        self
    }

    /// Build the handler and return receivers.
    pub fn build(
        self,
    ) -> (
        WebRtcProtocolHandler,
        mpsc::Receiver<WebRtcIncoming>,
        mpsc::Receiver<WebRtcIncoming>,
        mpsc::Receiver<WebRtcIncoming>,
    ) {
        WebRtcProtocolHandler::new(self.config)
    }
}

impl Default for WebRtcProtocolHandlerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handler_stream_types() {
        let (handler, _, _, _) = WebRtcProtocolHandler::with_defaults();

        let types = handler.stream_types();
        assert!(types.contains(&StreamType::WebRtcSignal));
        assert!(types.contains(&StreamType::WebRtcMedia));
        assert!(types.contains(&StreamType::WebRtcData));
        assert_eq!(types.len(), 3);
    }

    #[tokio::test]
    async fn test_handler_name() {
        let (handler, _, _, _) = WebRtcProtocolHandler::with_defaults();
        assert_eq!(handler.name(), "WebRtcProtocolHandler");
    }

    #[tokio::test]
    async fn test_handle_signal_message() {
        let (handler, mut signal_rx, _, _) = WebRtcProtocolHandler::with_defaults();

        let peer = PeerId::from([1u8; 32]);
        let message = SignalingMessage::Offer {
            session_id: "test-session".to_string(),
            sdp: "v=0\r\no=- 123 1 IN IP4 127.0.0.1\r\n".to_string(),
            quic_endpoint: None,
        };

        let data = Bytes::from(serde_json::to_vec(&message).unwrap());

        let result = handler
            .handle_stream(peer, StreamType::WebRtcSignal, data)
            .await;
        assert!(result.is_ok());

        // Check message was forwarded
        let received = signal_rx.try_recv();
        assert!(received.is_ok());

        if let WebRtcIncoming::Signal {
            peer: p,
            message: m,
        } = received.unwrap()
        {
            assert_eq!(p, peer);
            assert_eq!(m.session_id(), "test-session");
        } else {
            panic!("Expected Signal message");
        }
    }

    #[tokio::test]
    async fn test_handle_media_packet() {
        let (handler, _, mut media_rx, _) = WebRtcProtocolHandler::with_defaults();

        let peer = PeerId::from([2u8; 32]);
        let packet = RtpPacket::new(
            96,         // payload type
            1000,       // sequence number
            12345,      // timestamp
            0xDEADBEEF, // SSRC
            vec![1, 2, 3, 4],
            crate::quic_bridge::StreamType::Audio,
        )
        .unwrap();

        let data = Bytes::from(packet.to_bytes().unwrap());

        let result = handler
            .handle_stream(peer, StreamType::WebRtcMedia, data)
            .await;
        assert!(result.is_ok());

        // Check packet was forwarded
        let received = media_rx.try_recv();
        assert!(received.is_ok());

        if let WebRtcIncoming::Media {
            peer: p,
            packet: pkt,
        } = received.unwrap()
        {
            assert_eq!(p, peer);
            assert_eq!(pkt.sequence_number, 1000);
        } else {
            panic!("Expected Media message");
        }
    }

    #[tokio::test]
    async fn test_handle_data_channel() {
        let (handler, _, _, mut data_rx) = WebRtcProtocolHandler::with_defaults();

        let peer = PeerId::from([3u8; 32]);

        // Build data channel message: 4-byte channel ID + payload
        let channel_id: u32 = 42;
        let payload = b"hello world";
        let mut data = channel_id.to_be_bytes().to_vec();
        data.extend_from_slice(payload);

        let result = handler
            .handle_stream(peer, StreamType::WebRtcData, Bytes::from(data))
            .await;
        assert!(result.is_ok());

        // Check message was forwarded
        let received = data_rx.try_recv();
        assert!(received.is_ok());

        if let WebRtcIncoming::Data {
            peer: p,
            channel_id: ch,
            data: d,
        } = received.unwrap()
        {
            assert_eq!(p, peer);
            assert_eq!(ch, 42);
            assert_eq!(&d[..], payload);
        } else {
            panic!("Expected Data message");
        }
    }

    #[tokio::test]
    async fn test_data_channel_too_short() {
        let (handler, _, _, _) = WebRtcProtocolHandler::with_defaults();

        let peer = PeerId::from([4u8; 32]);
        let data = Bytes::from_static(&[1, 2, 3]); // Only 3 bytes, need 4 for channel ID

        let result = handler
            .handle_stream(peer, StreamType::WebRtcData, data)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_session_tracking() {
        let (handler, _, _, _) = WebRtcProtocolHandler::with_defaults();

        let peer = PeerId::from([5u8; 32]);

        // Initially no sessions
        assert_eq!(handler.session_count().await, 0);

        // Send a data channel message
        let mut data = 1u32.to_be_bytes().to_vec();
        data.extend_from_slice(b"test");

        let _ = handler
            .handle_stream(peer, StreamType::WebRtcData, Bytes::from(data))
            .await;

        // Now we have a session
        assert_eq!(handler.session_count().await, 1);

        let stats = handler.get_session_stats(&peer).await;
        assert!(stats.is_some());
        let (msgs, channels) = stats.unwrap();
        assert_eq!(msgs, 1);
        assert!(channels.contains(&1));

        // Remove session
        handler.remove_session(&peer).await;
        assert_eq!(handler.session_count().await, 0);
    }

    #[tokio::test]
    async fn test_shutdown() {
        let (handler, _, _, _) = WebRtcProtocolHandler::with_defaults();

        // Shutdown
        let result = handler.shutdown().await;
        assert!(result.is_ok());

        // After shutdown, handle_stream should fail
        let peer = PeerId::from([6u8; 32]);
        let result = handler
            .handle_stream(peer, StreamType::WebRtcSignal, Bytes::new())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_builder() {
        let (handler, _, _, _) = WebRtcProtocolHandlerBuilder::new()
            .signal_buffer_size(128)
            .media_buffer_size(512)
            .data_buffer_size(256)
            .build();

        assert_eq!(handler.name(), "WebRtcProtocolHandler");
    }

    #[tokio::test]
    async fn test_invalid_signal_message() {
        let (handler, _, _, _) = WebRtcProtocolHandler::with_defaults();

        let peer = PeerId::from([7u8; 32]);
        let data = Bytes::from_static(b"not valid json");

        let result = handler
            .handle_stream(peer, StreamType::WebRtcSignal, data)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unexpected_stream_type() {
        let (handler, _, _, _) = WebRtcProtocolHandler::with_defaults();

        let peer = PeerId::from([8u8; 32]);

        // Try with a non-WebRTC stream type
        let result = handler
            .handle_stream(peer, StreamType::Membership, Bytes::new())
            .await;
        assert!(result.is_err());
    }
}

/// Stream routing for WebRTC media types
///
/// Provides mapping between packet types and QUIC stream types
pub mod stream_routing {
    use crate::link_transport::StreamType;

    /// RTP payload types for audio codecs (96-127)
    pub const AUDIO_PAYLOAD_TYPE_RANGE: (u8, u8) = (96, 127);

    /// RTP payload types for video codecs (96-127)
    pub const VIDEO_PAYLOAD_TYPE_RANGE: (u8, u8) = (96, 127);

    /// RTCP payload types (200-211)
    pub const RTCP_PAYLOAD_TYPE_RANGE: (u8, u8) = (200, 211);

    /// Detect if a packet is RTP based on payload type
    ///
    /// # Arguments
    ///
    /// * `payload_type` - The RTP payload type (first 2 bits of first byte)
    ///
    /// # Returns
    ///
    /// `true` if this appears to be an RTP packet
    pub fn is_rtp(payload_type: u8) -> bool {
        payload_type < 128 || (96..=127).contains(&payload_type)
    }

    /// Detect if a packet is RTCP based on payload type
    ///
    /// # Arguments
    ///
    /// * `payload_type` - The RTCP payload type
    ///
    /// # Returns
    ///
    /// `true` if this appears to be an RTCP packet
    pub fn is_rtcp(payload_type: u8) -> bool {
        (200..=211).contains(&payload_type)
    }

    /// Detect if packet is audio based on codec hint
    ///
    /// # Arguments
    ///
    /// * `payload_type` - The RTP payload type
    ///
    /// # Returns
    ///
    /// `true` if this is likely an audio codec
    pub fn is_audio_codec(payload_type: u8) -> bool {
        // Dynamic payload types 96-127 need external SDP mapping
        // But common audio PTs: 0-23 are static
        matches!(
            payload_type,
            0 | 1
                | 3
                | 4
                | 5
                | 6
                | 7
                | 8
                | 9
                | 10
                | 11
                | 12
                | 13
                | 14
                | 15
                | 16
                | 17
                | 18
                | 19
                | 25
                | 97
        )
    }

    /// Detect if packet is video based on codec hint
    ///
    /// # Arguments
    ///
    /// * `payload_type` - The RTP payload type
    ///
    /// # Returns
    ///
    /// `true` if this is likely a video codec
    pub fn is_video_codec(payload_type: u8) -> bool {
        // Common video PTs: 26, 32-34, 96-127 (dynamic), etc.
        matches!(
            payload_type,
            26 | 32 | 33 | 34 | 96 | 97 | 98 | 99 | 100 | 101 | 102 | 103 | 104 | 105
        )
    }

    /// Route media to appropriate stream based on type
    ///
    /// # Arguments
    ///
    /// * `payload_type` - The RTP/RTCP payload type
    ///
    /// # Returns
    ///
    /// The target `StreamType` for this packet
    pub fn route_to_stream(payload_type: u8) -> StreamType {
        if is_rtcp(payload_type) {
            StreamType::RtcpFeedback
        } else if is_audio_codec(payload_type) {
            StreamType::Audio
        } else if is_video_codec(payload_type) {
            StreamType::Video
        } else {
            // Default to video for unknown dynamic types
            StreamType::Video
        }
    }

    /// Get all RTCP packet types
    ///
    /// # Returns
    ///
    /// A vector of all valid RTCP payload types
    pub fn rtcp_feedback_types() -> Vec<u8> {
        (200..=211).collect()
    }

    #[cfg(test)]
    mod routing_tests {
        use super::*;

        #[test]
        fn test_is_rtp() {
            assert!(is_rtp(0));
            assert!(is_rtp(96));
            assert!(is_rtp(127));
            assert!(!is_rtp(200));
        }

        #[test]
        fn test_is_rtcp() {
            assert!(is_rtcp(200));
            assert!(is_rtcp(205));
            assert!(is_rtcp(211));
            assert!(!is_rtcp(199));
            assert!(!is_rtcp(212));
        }

        #[test]
        fn test_is_audio_codec() {
            assert!(is_audio_codec(0)); // PCMU
            assert!(is_audio_codec(8)); // PCMA
            assert!(is_audio_codec(97)); // iLBC
            assert!(!is_audio_codec(26)); // Video
        }

        #[test]
        fn test_is_video_codec() {
            assert!(is_video_codec(26)); // Motion JPEG
            assert!(is_video_codec(32)); // MPV
            assert!(is_video_codec(96)); // Dynamic
            assert!(!is_video_codec(0)); // Audio
        }

        #[test]
        fn test_route_to_stream_audio() {
            let stream = route_to_stream(0); // PCMU
            assert_eq!(stream, StreamType::Audio);
        }

        #[test]
        fn test_route_to_stream_video() {
            let stream = route_to_stream(26); // Motion JPEG
            assert_eq!(stream, StreamType::Video);
        }

        #[test]
        fn test_route_to_stream_rtcp() {
            let stream = route_to_stream(200); // SR
            assert_eq!(stream, StreamType::RtcpFeedback);
        }

        #[test]
        fn test_rtcp_feedback_types() {
            let types = rtcp_feedback_types();
            assert_eq!(types.len(), 12);
            assert_eq!(types[0], 200);
            assert_eq!(types[11], 211);
        }
    }
}

impl WebRtcProtocolHandler {
    /// Route an incoming packet to the correct stream type
    ///
    /// # Arguments
    ///
    /// * `payload` - The packet payload
    ///
    /// # Returns
    ///
    /// The target `StreamType` for this packet
    pub fn route_packet_to_stream(payload: &[u8]) -> crate::link_transport::StreamType {
        if payload.is_empty() {
            return crate::link_transport::StreamType::Data;
        }

        // Extract payload type from RTP/RTCP header
        // First check if this is RTCP (byte[1] >= 200)
        if payload[1] >= 200 {
            return crate::link_transport::StreamType::RtcpFeedback;
        }

        // For RTP, extract payload type from bits 1-7 of second byte
        let pt = payload[1] & 0x7F;
        stream_routing::route_to_stream(pt)
    }

    /// Get the media type for a stream
    ///
    /// # Arguments
    ///
    /// * `stream_type` - The stream type
    ///
    /// # Returns
    ///
    /// A description of the media type
    pub fn stream_media_type(stream_type: crate::link_transport::StreamType) -> &'static str {
        match stream_type {
            crate::link_transport::StreamType::Audio => "Audio RTP",
            crate::link_transport::StreamType::Video => "Video RTP",
            crate::link_transport::StreamType::Screen => "Screen Share RTP",
            crate::link_transport::StreamType::RtcpFeedback => "RTCP Feedback",
            crate::link_transport::StreamType::Data => "Data Channel",
        }
    }
}

#[cfg(test)]
mod routing_integration_tests {
    use super::*;
    use crate::link_transport::StreamType;

    #[test]
    fn test_route_packet_audio_rtp() {
        // RTP packet with PCMU (PT=0)
        let payload = vec![0x80, 0x00, 0x00, 0x01];
        let stream = WebRtcProtocolHandler::route_packet_to_stream(&payload);
        assert_eq!(stream, StreamType::Audio);
    }

    #[test]
    fn test_route_packet_video_rtp() {
        // RTP packet with Motion JPEG (PT=26)
        let payload = vec![0x80, 0x1A, 0x00, 0x01];
        let stream = WebRtcProtocolHandler::route_packet_to_stream(&payload);
        assert_eq!(stream, StreamType::Video);
    }

    #[test]
    fn test_route_packet_rtcp() {
        // RTCP packet with SR (PT=200)
        let payload = vec![0x80, 0xC8, 0x00, 0x01];
        let stream = WebRtcProtocolHandler::route_packet_to_stream(&payload);
        assert_eq!(stream, StreamType::RtcpFeedback);
    }

    #[test]
    fn test_route_packet_empty() {
        let payload: Vec<u8> = vec![];
        let stream = WebRtcProtocolHandler::route_packet_to_stream(&payload);
        assert_eq!(stream, StreamType::Data);
    }

    #[test]
    fn test_stream_media_type_descriptions() {
        assert_eq!(
            WebRtcProtocolHandler::stream_media_type(StreamType::Audio),
            "Audio RTP"
        );
        assert_eq!(
            WebRtcProtocolHandler::stream_media_type(StreamType::Video),
            "Video RTP"
        );
        assert_eq!(
            WebRtcProtocolHandler::stream_media_type(StreamType::Screen),
            "Screen Share RTP"
        );
        assert_eq!(
            WebRtcProtocolHandler::stream_media_type(StreamType::RtcpFeedback),
            "RTCP Feedback"
        );
        assert_eq!(
            WebRtcProtocolHandler::stream_media_type(StreamType::Data),
            "Data Channel"
        );
    }
}
