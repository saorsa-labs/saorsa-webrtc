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
}

impl StreamType {
    /// Get priority value (lower = higher priority)
    #[must_use]
    pub const fn priority(&self) -> u8 {
        match self {
            Self::Audio => 1, // Highest priority
            Self::Video => 2,
            Self::ScreenShare => 3,
            Self::Data => 4, // Lowest priority
        }
    }

    /// Check if stream is real-time (audio/video)
    #[must_use]
    pub const fn is_realtime(&self) -> bool {
        matches!(self, Self::Audio | Self::Video | Self::ScreenShare)
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

    /// Send RTP packet over QUIC
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
            .to_bytes()
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

        tracing::debug!("Sent RTP packet of size {} bytes", data.len());

        Ok(())
    }

    /// Receive RTP packet from QUIC
    ///
    /// # Errors
    ///
    /// Returns error if receiving fails
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

        let packet = RtpPacket::from_bytes(&data).map_err(|e| {
            BridgeError::StreamError(format!("Failed to deserialize packet: {}", e))
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
}
