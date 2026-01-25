//! WebRTC to QUIC bridge
//!
//! Bridges WebRTC media with QUIC transport for data channels.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Bridge errors
#[derive(Error, Debug)]
pub enum BridgeError {
    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Stream error
    #[error("Stream error: {0}")]
    StreamError(String),
}

/// Stream type classification for prioritization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamType {
    /// Audio stream
    Audio,
    /// Video stream
    Video,
    /// Data channel
    Data,
    /// Screen sharing stream
    ScreenShare,
    /// RTCP feedback stream for QoS
    RtcpFeedback,
}

/// Stream type tag constants for QUIC streams
pub mod stream_tags {
    /// Audio stream type tag
    pub const AUDIO: u8 = 0x21;
    /// Video stream type tag
    pub const VIDEO: u8 = 0x22;
    /// Screen share stream type tag
    pub const SCREEN_SHARE: u8 = 0x23;
    /// RTCP feedback stream type tag
    pub const RTCP_FEEDBACK: u8 = 0x24;
    /// Data channel stream type tag
    pub const DATA: u8 = 0x25;
}

impl StreamType {
    /// Get priority value (lower = higher priority)
    #[must_use]
    pub const fn priority(&self) -> u8 {
        match self {
            Self::Audio => 1,        // Highest priority
            Self::RtcpFeedback => 1, // RTCP is critical for QoS
            Self::Video => 2,
            Self::ScreenShare => 3,
            Self::Data => 4, // Lowest priority
        }
    }

    /// Check if stream is real-time (audio/video)
    #[must_use]
    pub const fn is_realtime(&self) -> bool {
        matches!(
            self,
            Self::Audio | Self::Video | Self::ScreenShare | Self::RtcpFeedback
        )
    }

    /// Convert to stream type tag byte for QUIC framing
    #[must_use]
    pub const fn to_tag(&self) -> u8 {
        match self {
            Self::Audio => stream_tags::AUDIO,
            Self::Video => stream_tags::VIDEO,
            Self::ScreenShare => stream_tags::SCREEN_SHARE,
            Self::RtcpFeedback => stream_tags::RTCP_FEEDBACK,
            Self::Data => stream_tags::DATA,
        }
    }

    /// Create from stream type tag byte
    ///
    /// Returns None if tag is not a valid stream type
    #[must_use]
    pub const fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            stream_tags::AUDIO => Some(Self::Audio),
            stream_tags::VIDEO => Some(Self::Video),
            stream_tags::SCREEN_SHARE => Some(Self::ScreenShare),
            stream_tags::RTCP_FEEDBACK => Some(Self::RtcpFeedback),
            stream_tags::DATA => Some(Self::Data),
            _ => None,
        }
    }
}

/// RTP packet structure for media transmission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtpPacket {
    /// RTP header version (always 2)
    pub version: u8,
    /// Padding bit
    pub padding: bool,
    /// Extension bit
    pub extension: bool,
    /// CSRC count
    pub csrc_count: u8,
    /// Marker bit
    pub marker: bool,
    /// Payload type
    pub payload_type: u8,
    /// Sequence number
    pub sequence_number: u16,
    /// Timestamp
    pub timestamp: u32,
    /// SSRC identifier
    pub ssrc: u32,
    /// Payload data
    pub payload: Vec<u8>,
    /// Stream type classification
    pub stream_type: StreamType,
}

impl RtpPacket {
    /// Create new RTP packet
    ///
    /// # Errors
    ///
    /// Returns error if payload exceeds maximum packet size
    pub fn new(
        payload_type: u8,
        sequence_number: u16,
        timestamp: u32,
        ssrc: u32,
        payload: Vec<u8>,
        stream_type: StreamType,
    ) -> Result<Self> {
        const MAX_PAYLOAD_SIZE: usize = 1188; // 1200 - 12 byte RTP header

        if payload.len() > MAX_PAYLOAD_SIZE {
            return Err(anyhow::anyhow!(
                "Payload size {} exceeds maximum {}",
                payload.len(),
                MAX_PAYLOAD_SIZE
            ));
        }

        Ok(Self {
            version: 2,
            padding: false,
            extension: false,
            csrc_count: 0,
            marker: false,
            payload_type,
            sequence_number,
            timestamp,
            ssrc,
            payload,
            stream_type,
        })
    }

    /// Serialize packet to bytes for QUIC transmission
    ///
    /// # Errors
    ///
    /// Returns error if serialization fails
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize RTP packet: {}", e))
    }

    /// Deserialize packet from bytes received via QUIC
    ///
    /// # Errors
    ///
    /// Returns error if deserialization fails or data exceeds size limits
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        const MAX_PACKET_SIZE: usize = 1200;

        // Validate input size before deserialization to prevent DoS
        if data.is_empty() {
            return Err(anyhow::anyhow!("Cannot deserialize empty data"));
        }

        if data.len() > MAX_PACKET_SIZE {
            return Err(anyhow::anyhow!(
                "Data size {} exceeds maximum packet size {}",
                data.len(),
                MAX_PACKET_SIZE
            ));
        }

        // Deserialize with pre-validated size limit
        bincode::deserialize(data)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize RTP packet: {}", e))
    }

    /// Get packet size in bytes
    #[must_use]
    pub fn size(&self) -> usize {
        12 + self.payload.len() // Basic RTP header is 12 bytes
    }

    /// Serialize packet with stream type tag prefix
    ///
    /// Format: [1-byte stream type tag][serialized packet]
    ///
    /// # Errors
    ///
    /// Returns error if serialization fails
    pub fn to_tagged_bytes(&self) -> Result<Vec<u8>> {
        let tag = self.stream_type.to_tag();
        let data = self.to_bytes()?;
        let mut tagged = Vec::with_capacity(1 + data.len());
        tagged.push(tag);
        tagged.extend(data);
        Ok(tagged)
    }

    /// Deserialize packet from tagged bytes
    ///
    /// Expects: [1-byte stream type tag][serialized packet]
    ///
    /// # Errors
    ///
    /// Returns error if deserialization fails or tag is invalid
    pub fn from_tagged_bytes(data: &[u8]) -> Result<Self> {
        if data.is_empty() {
            return Err(anyhow::anyhow!("Cannot deserialize empty data"));
        }

        let tag = data[0];
        let stream_type = StreamType::from_tag(tag)
            .ok_or_else(|| anyhow::anyhow!("Invalid stream type tag: 0x{:02X}", tag))?;

        let mut packet = Self::from_bytes(&data[1..])?;
        packet.stream_type = stream_type;
        Ok(packet)
    }
}

/// Stream configuration for QUIC media streams
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Stream type
    pub stream_type: StreamType,
    /// Target bitrate in bits per second
    pub target_bitrate_bps: u32,
    /// Maximum bitrate in bits per second
    pub max_bitrate_bps: u32,
    /// Maximum latency in milliseconds
    pub max_latency_ms: u32,
}

impl StreamConfig {
    /// Create audio stream configuration
    #[must_use]
    pub fn audio() -> Self {
        Self {
            stream_type: StreamType::Audio,
            target_bitrate_bps: 64_000,
            max_bitrate_bps: 128_000,
            max_latency_ms: 50,
        }
    }

    /// Create video stream configuration
    #[must_use]
    pub fn video() -> Self {
        Self {
            stream_type: StreamType::Video,
            target_bitrate_bps: 1_000_000,
            max_bitrate_bps: 2_000_000,
            max_latency_ms: 150,
        }
    }

    /// Create screen share configuration
    #[must_use]
    pub fn screen_share() -> Self {
        Self {
            stream_type: StreamType::ScreenShare,
            target_bitrate_bps: 500_000,
            max_bitrate_bps: 1_500_000,
            max_latency_ms: 200,
        }
    }
}

/// WebRTC to QUIC bridge configuration
#[derive(Debug, Clone)]
pub struct QuicBridgeConfig {
    /// Maximum packet size
    pub max_packet_size: usize,
}

impl Default for QuicBridgeConfig {
    fn default() -> Self {
        Self {
            max_packet_size: 1200,
        }
    }
}

/// WebRTC QUIC bridge
///
/// Handles translation between WebRTC RTP packets and QUIC streams
pub struct WebRtcQuicBridge {
    config: QuicBridgeConfig,
    transport: Option<crate::transport::AntQuicTransport>,
}

impl WebRtcQuicBridge {
    /// Create new bridge
    #[must_use]
    pub fn new(config: QuicBridgeConfig) -> Self {
        Self {
            config,
            transport: None,
        }
    }

    /// Create bridge with transport
    #[must_use]
    pub fn with_transport(
        config: QuicBridgeConfig,
        transport: crate::transport::AntQuicTransport,
    ) -> Self {
        Self {
            config,
            transport: Some(transport),
        }
    }

    /// Send RTP packet over QUIC with stream type tagging
    ///
    /// Encodes the packet with a stream type tag prefix for proper routing.
    ///
    /// # Errors
    ///
    /// Returns error if sending fails
    pub async fn send_rtp_packet(&self, packet: &RtpPacket) -> Result<(), BridgeError> {
        let span = tracing::debug_span!(
            "send_rtp_packet",
            stream_type = ?packet.stream_type,
            priority = packet.stream_type.priority(),
            seq_num = packet.sequence_number
        );
        let _enter = span.enter();

        let transport = self
            .transport
            .as_ref()
            .ok_or_else(|| BridgeError::ConfigError("No transport configured".to_string()))?;

        let data = packet
            .to_tagged_bytes()
            .map_err(|e| BridgeError::StreamError(format!("Failed to serialize packet: {}", e)))?;

        if data.len() > self.config.max_packet_size {
            return Err(BridgeError::StreamError(format!(
                "Packet size {} exceeds maximum {}",
                data.len(),
                self.config.max_packet_size
            )));
        }

        transport
            .send_bytes(&data)
            .await
            .map_err(|e| BridgeError::StreamError(format!("Failed to send packet: {}", e)))?;

        tracing::debug!(
            "Sent RTP packet of size {} bytes with type tag 0x{:02X}",
            data.len(),
            packet.stream_type.to_tag()
        );

        Ok(())
    }

    /// Receive RTP packet from QUIC with stream type tagging
    ///
    /// Parses stream type from the tag prefix for proper routing.
    ///
    /// # Errors
    ///
    /// Returns error if receiving fails or tag is invalid
    pub async fn receive_rtp_packet(&self) -> Result<RtpPacket, BridgeError> {
        let span = tracing::debug_span!("receive_rtp_packet");
        let _enter = span.enter();

        let transport = self
            .transport
            .as_ref()
            .ok_or_else(|| BridgeError::ConfigError("No transport configured".to_string()))?;

        let data = transport
            .receive_bytes()
            .await
            .map_err(|e| BridgeError::StreamError(format!("Failed to receive: {}", e)))?;

        let packet = RtpPacket::from_tagged_bytes(&data).map_err(|e| {
            BridgeError::StreamError(format!("Failed to deserialize packet with tag: {}", e))
        })?;

        tracing::debug!(
            "Received RTP packet of size {} bytes, stream_type={:?}, seq={}",
            data.len(),
            packet.stream_type,
            packet.sequence_number
        );

        Ok(packet)
    }

    /// Bridge WebRTC track to QUIC stream
    ///
    /// # Errors
    ///
    /// Returns error if bridging fails
    pub async fn bridge_track(&self, _track_id: &str) -> Result<(), BridgeError> {
        // TODO: Implement track bridging
        Ok(())
    }
}

impl Default for WebRtcQuicBridge {
    fn default() -> Self {
        Self::new(QuicBridgeConfig::default())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_quic_bridge_send_rtp_packet() {
        let bridge = WebRtcQuicBridge::default();
        let packet = RtpPacket::new(
            96,
            1000,
            12345,
            0xDEADBEEF,
            vec![1, 2, 3, 4],
            StreamType::Audio,
        )
        .expect("Failed to create packet");

        // Will fail without transport, but that's expected
        let _result = bridge.send_rtp_packet(&packet).await;
    }

    #[tokio::test]
    async fn test_quic_bridge_receive_rtp_packet() {
        let bridge = WebRtcQuicBridge::default();

        let result = bridge.receive_rtp_packet().await;
        // Should fail without transport configured
        assert!(result.is_err());
        assert!(matches!(result, Err(BridgeError::ConfigError(_))));
    }

    #[tokio::test]
    async fn test_quic_bridge_bridge_track() {
        let bridge = WebRtcQuicBridge::default();

        let result = bridge.bridge_track("audio-track").await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_stream_type_to_tag() {
        assert_eq!(StreamType::Audio.to_tag(), stream_tags::AUDIO);
        assert_eq!(StreamType::Video.to_tag(), stream_tags::VIDEO);
        assert_eq!(StreamType::ScreenShare.to_tag(), stream_tags::SCREEN_SHARE);
        assert_eq!(StreamType::Data.to_tag(), stream_tags::DATA);
    }

    #[test]
    fn test_stream_type_from_tag() {
        assert_eq!(
            StreamType::from_tag(stream_tags::AUDIO),
            Some(StreamType::Audio)
        );
        assert_eq!(
            StreamType::from_tag(stream_tags::VIDEO),
            Some(StreamType::Video)
        );
        assert_eq!(
            StreamType::from_tag(stream_tags::SCREEN_SHARE),
            Some(StreamType::ScreenShare)
        );
        assert_eq!(
            StreamType::from_tag(stream_tags::DATA),
            Some(StreamType::Data)
        );
        assert_eq!(StreamType::from_tag(0xFF), None); // Invalid tag
    }

    #[test]
    fn test_tagged_bytes_roundtrip() {
        let packet = RtpPacket::new(
            96,
            1234,
            56789,
            0xABCDEF01,
            vec![0x01, 0x02, 0x03, 0x04, 0x05],
            StreamType::Audio,
        )
        .expect("Failed to create packet");

        let tagged = packet.to_tagged_bytes().expect("Failed to serialize");

        // First byte should be stream type tag
        assert_eq!(tagged[0], stream_tags::AUDIO);

        // Deserialize should produce same packet
        let restored = RtpPacket::from_tagged_bytes(&tagged).expect("Failed to deserialize");
        assert_eq!(restored.payload_type, 96);
        assert_eq!(restored.sequence_number, 1234);
        assert_eq!(restored.timestamp, 56789);
        assert_eq!(restored.ssrc, 0xABCDEF01);
        assert_eq!(restored.stream_type, StreamType::Audio);
    }

    #[test]
    fn test_tagged_bytes_video() {
        let packet = RtpPacket::new(
            98,
            5000,
            100000,
            0x12345678,
            vec![0xAA, 0xBB],
            StreamType::Video,
        )
        .expect("Failed to create packet");

        let tagged = packet.to_tagged_bytes().expect("Failed to serialize");
        assert_eq!(tagged[0], stream_tags::VIDEO);

        let restored = RtpPacket::from_tagged_bytes(&tagged).expect("Failed to deserialize");
        assert_eq!(restored.stream_type, StreamType::Video);
    }

    #[test]
    fn test_tagged_bytes_invalid_tag() {
        // Create invalid tagged bytes (bad tag)
        let invalid = vec![0xFF, 0x00, 0x01]; // Invalid tag 0xFF

        let result = RtpPacket::from_tagged_bytes(&invalid);
        assert!(result.is_err());
    }

    #[test]
    fn test_tagged_bytes_empty() {
        let result = RtpPacket::from_tagged_bytes(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_stream_type_priority() {
        assert_eq!(StreamType::Audio.priority(), 1);
        assert_eq!(StreamType::Video.priority(), 2);
        assert_eq!(StreamType::ScreenShare.priority(), 3);
        assert_eq!(StreamType::Data.priority(), 4);

        // Audio should have higher priority (lower value) than video
        assert!(StreamType::Audio.priority() < StreamType::Video.priority());
        assert!(StreamType::Video.priority() < StreamType::Data.priority());
    }

    #[test]
    fn test_stream_type_is_realtime() {
        assert!(StreamType::Audio.is_realtime());
        assert!(StreamType::Video.is_realtime());
        assert!(StreamType::ScreenShare.is_realtime());
        assert!(!StreamType::Data.is_realtime());
    }

    #[test]
    fn test_tagged_bytes_all_stream_types() {
        for (stream_type, expected_tag) in &[
            (StreamType::Audio, stream_tags::AUDIO),
            (StreamType::Video, stream_tags::VIDEO),
            (StreamType::ScreenShare, stream_tags::SCREEN_SHARE),
            (StreamType::Data, stream_tags::DATA),
        ] {
            let packet =
                RtpPacket::new(96, 1000, 10000, 0x12345678, vec![0x01, 0x02], *stream_type)
                    .expect("Failed to create packet");

            let tagged = packet.to_tagged_bytes().expect("Failed to tag");
            assert_eq!(
                tagged[0], *expected_tag,
                "Tag mismatch for {:?}",
                stream_type
            );

            let restored = RtpPacket::from_tagged_bytes(&tagged).expect("Failed to restore");
            assert_eq!(
                restored.stream_type, *stream_type,
                "Type mismatch for {:?}",
                stream_type
            );
        }
    }

    #[test]
    fn test_tagged_bytes_preserves_payload() {
        let original_payload = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let packet = RtpPacket::new(
            96,
            1000,
            10000,
            0x12345678,
            original_payload.clone(),
            StreamType::Audio,
        )
        .expect("Failed to create packet");

        let tagged = packet.to_tagged_bytes().expect("Failed to tag");
        let restored = RtpPacket::from_tagged_bytes(&tagged).expect("Failed to restore");

        assert_eq!(restored.payload, original_payload);
        assert_eq!(restored.version, 2);
        assert_eq!(restored.payload_type, 96);
        assert_eq!(restored.sequence_number, 1000);
        assert_eq!(restored.timestamp, 10000);
        assert_eq!(restored.ssrc, 0x12345678);
    }
}
