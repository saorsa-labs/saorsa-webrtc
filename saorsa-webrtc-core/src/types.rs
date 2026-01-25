//! WebRTC types and data structures

use crate::identity::PeerIdentity;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for a call
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CallId(pub Uuid);

impl CallId {
    /// Create a new random call ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for CallId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Media constraints for a call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaConstraints {
    /// Enable audio
    pub audio: bool,
    /// Enable video
    pub video: bool,
    /// Enable screen sharing
    pub screen_share: bool,
}

impl MediaConstraints {
    /// Audio-only call
    pub fn audio_only() -> Self {
        Self {
            audio: true,
            video: false,
            screen_share: false,
        }
    }

    /// Video call with audio
    pub fn video_call() -> Self {
        Self {
            audio: true,
            video: true,
            screen_share: false,
        }
    }

    /// Screen share with audio
    pub fn screen_share() -> Self {
        Self {
            audio: true,
            video: false,
            screen_share: true,
        }
    }

    /// Check if audio is enabled
    pub fn has_audio(&self) -> bool {
        self.audio
    }

    /// Check if video is enabled
    pub fn has_video(&self) -> bool {
        self.video
    }

    /// Check if screen share is enabled
    pub fn has_screen_share(&self) -> bool {
        self.screen_share
    }

    /// Convert to media types
    pub fn to_media_types(&self) -> Vec<MediaType> {
        let mut types = Vec::new();
        if self.audio {
            types.push(MediaType::Audio);
        }
        if self.video {
            types.push(MediaType::Video);
        }
        if self.screen_share {
            types.push(MediaType::ScreenShare);
        }
        types
    }
}

/// Types of media in a call
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaType {
    /// Audio stream
    Audio,
    /// Video stream
    Video,
    /// Screen share stream
    ScreenShare,
    /// Data channel
    DataChannel,
}

/// Call offer message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "I: PeerIdentity")]
pub struct CallOffer<I: PeerIdentity> {
    /// Unique call identifier
    pub call_id: CallId,
    /// Identity of the caller
    pub caller: I,
    /// Identity of the callee
    pub callee: I,
    /// SDP offer string
    pub sdp: String,
    /// Media types in this call
    pub media_types: Vec<MediaType>,
    /// Timestamp when offer was created
    pub timestamp: DateTime<Utc>,
}

/// Call answer message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallAnswer {
    /// Call identifier
    pub call_id: CallId,
    /// SDP answer string
    pub sdp: String,
    /// Whether the call was accepted
    pub accepted: bool,
    /// Timestamp when answer was created
    pub timestamp: DateTime<Utc>,
}

/// ICE candidate for WebRTC connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceCandidate {
    /// Call identifier
    pub call_id: CallId,
    /// ICE candidate string
    pub candidate: String,
    /// SDP media ID
    pub sdp_mid: Option<String>,
    /// SDP media line index
    pub sdp_mline_index: Option<u32>,
}

/// Call state enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallState {
    /// No active call
    Idle,
    /// Initiating call
    Calling,
    /// Establishing connection
    Connecting,
    /// Call is active
    Connected,
    /// Call is ending
    Ending,
    /// Call failed
    Failed,
}

/// Call quality metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallQualityMetrics {
    /// Round-trip time in milliseconds
    pub rtt_ms: u32,
    /// Packet loss percentage
    pub packet_loss_percent: f32,
    /// Jitter in milliseconds
    pub jitter_ms: u32,
    /// Bandwidth in kilobits per second
    pub bandwidth_kbps: u32,
    /// Timestamp when metrics were collected
    pub timestamp: DateTime<Utc>,
}

impl CallQualityMetrics {
    /// Check if quality is good
    pub fn is_good_quality(&self) -> bool {
        self.rtt_ms < 100
            && self.packet_loss_percent < 1.0
            && self.jitter_ms < 20
            && self.bandwidth_kbps > 500
    }

    /// Check if network adaptation is needed
    pub fn needs_adaptation(&self) -> bool {
        self.rtt_ms > 200
            || self.packet_loss_percent > 3.0
            || self.jitter_ms > 40
            || self.bandwidth_kbps < 300
    }
}

/// Multi-party call information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "I: PeerIdentity")]
pub struct MultiPartyCall<I: PeerIdentity> {
    /// Call identifier
    pub call_id: CallId,
    /// Participating peers
    pub participants: Vec<I>,
    /// Call architecture
    pub architecture: CallArchitecture,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
}

/// Call architecture for multi-party calls
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallArchitecture {
    /// Mesh - Direct P2P between all participants (2-4 people)
    Mesh,
    /// SFU - Selective Forwarding Unit (5+ people)
    SFU,
}

/// Recording consent management
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "I: PeerIdentity")]
pub struct RecordingConsent<I: PeerIdentity> {
    /// Call being recorded
    pub call_id: CallId,
    /// Who is requesting to record
    pub requester: I,
    /// Participants who must consent
    pub participants: Vec<I>,
}

/// Consent status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsentStatus {
    /// Awaiting response
    Pending,
    /// Consent granted
    Granted,
    /// Consent denied
    Denied,
    /// Consent revoked
    Revoked,
}

/// Network adaptation settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptationSettings {
    /// Video bitrate in kilobits per second
    pub video_bitrate_kbps: u32,
    /// Video resolution
    pub video_resolution: VideoResolution,
    /// Video frames per second
    pub video_fps: u32,
    /// Audio bitrate in kilobits per second
    pub audio_bitrate_kbps: u32,
    /// Enable discontinuous transmission
    pub enable_dtx: bool,
}

/// Video resolution options
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoResolution {
    /// 320x240
    QVGA240,
    /// 640x480
    SD480,
    /// 1280x720
    HD720,
    /// 1920x1080
    HD1080,
}

impl VideoResolution {
    /// Get width in pixels
    pub fn width(&self) -> u32 {
        match self {
            Self::QVGA240 => 320,
            Self::SD480 => 640,
            Self::HD720 => 1280,
            Self::HD1080 => 1920,
        }
    }

    /// Get height in pixels
    pub fn height(&self) -> u32 {
        match self {
            Self::QVGA240 => 240,
            Self::SD480 => 480,
            Self::HD720 => 720,
            Self::HD1080 => 1080,
        }
    }
}

/// Native QUIC connectivity configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeQuicConfiguration {
    /// DHT-based peer discovery is enabled by default
    pub dht_discovery: bool,
    /// Coordinator-based hole punching configuration
    pub hole_punching: bool,
}

impl Default for NativeQuicConfiguration {
    fn default() -> Self {
        Self {
            dht_discovery: true,
            hole_punching: true,
        }
    }
}

/// WebRTC signaling message wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "I: PeerIdentity")]
pub enum SignalingMessage<I: PeerIdentity> {
    /// Call offer
    Offer(CallOffer<I>),
    /// Call answer
    Answer(CallAnswer),
    /// End call
    CallEnd {
        /// Call to end
        call_id: CallId,
    },
    /// Reject call
    CallReject {
        /// Call to reject
        call_id: CallId,
    },
}

/// Call event for notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "I: PeerIdentity")]
pub enum CallEvent<I: PeerIdentity> {
    /// Incoming call received
    IncomingCall {
        /// The call offer
        offer: CallOffer<I>,
    },
    /// Call initiated
    CallInitiated {
        /// Call identifier
        call_id: CallId,
        /// Who is being called
        callee: I,
        /// Media constraints
        constraints: MediaConstraints,
    },
    /// Call accepted
    CallAccepted {
        /// Call identifier
        call_id: CallId,
        /// The answer
        answer: CallAnswer,
    },
    /// Call rejected
    CallRejected {
        /// Call identifier
        call_id: CallId,
    },
    /// Call ended
    CallEnded {
        /// Call identifier
        call_id: CallId,
    },
    /// Connection established
    ConnectionEstablished {
        /// Call identifier
        call_id: CallId,
    },
    /// Connection failed
    ConnectionFailed {
        /// Call identifier
        call_id: CallId,
        /// Error description
        error: String,
    },
    /// Quality changed
    QualityChanged {
        /// Call identifier
        call_id: CallId,
        /// Current metrics
        metrics: CallQualityMetrics,
    },
}

/// Call session information
#[derive(Debug, Clone)]
pub struct CallSession<I: PeerIdentity> {
    /// Call identifier
    pub call_id: CallId,
    /// Participating peers
    pub participants: Vec<I>,
    /// Current state
    pub state: CallState,
    /// Media constraints
    pub media_constraints: MediaConstraints,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Start time (when connected)
    pub start_time: Option<DateTime<Utc>>,
    /// End time
    pub end_time: Option<DateTime<Utc>>,
    /// Quality metrics history
    pub quality_metrics: Vec<CallQualityMetrics>,
}

impl<I: PeerIdentity> CallSession<I> {
    /// Create a new call session
    pub fn new(call_id: CallId, constraints: MediaConstraints) -> Self {
        Self {
            call_id,
            participants: Vec::new(),
            state: CallState::Idle,
            media_constraints: constraints,
            created_at: Utc::now(),
            start_time: None,
            end_time: None,
            quality_metrics: Vec::new(),
        }
    }

    /// Get call duration
    pub fn duration(&self) -> Option<chrono::Duration> {
        if let (Some(start), Some(end)) = (self.start_time, self.end_time) {
            Some(end - start)
        } else {
            self.start_time.map(|start| Utc::now() - start)
        }
    }

    /// Add a participant
    pub fn add_participant(&mut self, participant: I) {
        if !self
            .participants
            .iter()
            .any(|p| p.to_string_repr() == participant.to_string_repr())
        {
            self.participants.push(participant);
        }
    }

    /// Remove a participant
    pub fn remove_participant(&mut self, participant: &I) {
        self.participants
            .retain(|p| p.to_string_repr() != participant.to_string_repr());
    }

    /// Add quality metric
    pub fn add_quality_metric(&mut self, metric: CallQualityMetrics) {
        self.quality_metrics.push(metric);

        // Keep only last 100 metrics
        if self.quality_metrics.len() > 100 {
            self.quality_metrics.remove(0);
        }
    }

    /// Get latest quality metric
    pub fn latest_quality(&self) -> Option<&CallQualityMetrics> {
        self.quality_metrics.last()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::identity::PeerIdentityString;

    #[test]
    fn test_call_id() {
        let id1 = CallId::new();
        let id2 = CallId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_media_constraints() {
        let audio = MediaConstraints::audio_only();
        assert!(audio.has_audio());
        assert!(!audio.has_video());
        assert!(!audio.has_screen_share());

        let video = MediaConstraints::video_call();
        assert!(video.has_audio());
        assert!(video.has_video());
        assert!(!video.has_screen_share());

        let screen = MediaConstraints::screen_share();
        assert!(screen.has_audio());
        assert!(!screen.has_video());
        assert!(screen.has_screen_share());
    }

    #[test]
    fn test_call_session() {
        let call_id = CallId::new();
        let constraints = MediaConstraints::video_call();
        let mut session: CallSession<PeerIdentityString> = CallSession::new(call_id, constraints);

        assert_eq!(session.call_id, call_id);
        assert_eq!(session.state, CallState::Idle);
        assert_eq!(session.participants.len(), 0);

        let peer = PeerIdentityString::new("alice");
        session.add_participant(peer.clone());
        assert_eq!(session.participants.len(), 1);

        session.add_participant(peer.clone()); // Should not add duplicate
        assert_eq!(session.participants.len(), 1);
    }

    #[test]
    fn test_quality_metrics() {
        let good = CallQualityMetrics {
            rtt_ms: 50,
            packet_loss_percent: 0.5,
            jitter_ms: 10,
            bandwidth_kbps: 1000,
            timestamp: Utc::now(),
        };
        assert!(good.is_good_quality());
        assert!(!good.needs_adaptation());

        let bad = CallQualityMetrics {
            rtt_ms: 300,
            packet_loss_percent: 5.0,
            jitter_ms: 50,
            bandwidth_kbps: 200,
            timestamp: Utc::now(),
        };
        assert!(!bad.is_good_quality());
        assert!(bad.needs_adaptation());
    }

    #[test]
    fn test_video_resolution() {
        let hd720 = VideoResolution::HD720;
        assert_eq!(hd720.width(), 1280);
        assert_eq!(hd720.height(), 720);

        let hd1080 = VideoResolution::HD1080;
        assert_eq!(hd1080.width(), 1920);
        assert_eq!(hd1080.height(), 1080);
    }
}
