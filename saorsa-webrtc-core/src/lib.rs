//! Saorsa WebRTC - WebRTC implementation over ant-quic transport
//!
//! This library provides a WebRTC implementation that uses ant-quic as the underlying
//! transport layer instead of traditional ICE/STUN/TURN. It features:
//!
//! - **Native QUIC Transport**: Uses ant-quic for reliable, encrypted connections
//! - **DHT-based Signaling**: Distributed signaling without centralized servers
//! - **Post-Quantum Cryptography**: Built-in PQC support via ant-quic
//! - **NAT Traversal**: Automatic hole punching and relay fallback
//! - **High Performance**: Low-latency media streaming with QoS
//!
//! # Examples
//!
//! ```rust,no_run
//! use saorsa_webrtc_core::{WebRtcService, MediaConstraints, SignalingHandler, AntQuicTransport, TransportConfig, PeerIdentityString};
//! use std::sync::Arc;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Create signaling transport
//! let transport = Arc::new(AntQuicTransport::new(TransportConfig::default()));
//! let signaling = Arc::new(SignalingHandler::new(transport));
//!
//! // Create WebRTC service
//! let service = WebRtcService::<PeerIdentityString, AntQuicTransport>::new(
//!     signaling,
//!     Default::default()
//! ).await?;
//!
//! // Start service
//! service.start().await?;
//!
//! // Initiate a video call
//! let call_id = service.initiate_call(
//!     PeerIdentityString::new("eve-frank-grace-henry"),
//!     MediaConstraints::video_call()
//! ).await?;
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(clippy::panic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![warn(clippy::all)]
// Allow pedantic warnings for stub implementations
#![allow(clippy::pedantic)]
#![allow(clippy::nursery)]
#![allow(clippy::unused_async)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::derivable_impls)]

/// Core WebRTC types and data structures
pub mod types;

/// WebRTC service and configuration (requires legacy-webrtc feature)
#[cfg(feature = "legacy-webrtc")]
pub mod service;

/// Media stream management (requires legacy-webrtc feature)
#[cfg(feature = "legacy-webrtc")]
pub mod media;

/// Call management and state (requires legacy-webrtc feature)
#[cfg(feature = "legacy-webrtc")]
pub mod call;

/// Signaling protocol and handlers
pub mod signaling;

/// ant-quic transport integration
pub mod transport;

/// QUIC media stream management with QoS
pub mod quic_streams;

/// Bridge between WebRTC and QUIC
pub mod quic_bridge;

/// Protocol handler for SharedTransport integration
pub mod protocol_handler;

/// Peer identity abstraction
pub mod identity;

/// Link transport abstraction layer
pub mod link_transport;

/// QUIC-based media transport for RTP/RTCP over QUIC streams
pub mod quic_media_transport;

// Re-export main types at crate root
#[cfg(feature = "legacy-webrtc")]
pub use call::{CallManager, CallManagerConfig};
pub use identity::{PeerIdentity, PeerIdentityString};
pub use link_transport::{LinkTransport, LinkTransportError, PeerConnection, StreamType as LinkStreamType};
pub use quic_media_transport::{
    MediaTransportError, MediaTransportState, QuicMediaTransport, StreamHandle, StreamPriority,
    TransportStats,
};
#[cfg(feature = "legacy-webrtc")]
pub use media::{
    AudioDevice, AudioTrack, MediaEvent, MediaStream, MediaStreamManager, VideoDevice, VideoTrack,
};
pub use protocol_handler::{
    WebRtcHandlerConfig, WebRtcHandlerError, WebRtcIncoming, WebRtcProtocolHandler,
    WebRtcProtocolHandlerBuilder,
};
pub use quic_bridge::{RtpPacket, StreamConfig, StreamType, WebRtcQuicBridge};
pub use service::{WebRtcConfig, WebRtcEvent, WebRtcService, WebRtcServiceBuilder};
pub use signaling::{
    SignalingHandler, SignalingMessage as SignalingMessageType, SignalingTransport,
};
pub use transport::{AntQuicTransport, TransportConfig};
pub use types::*;

/// Prelude module for convenient imports
pub mod prelude {
    #[cfg(feature = "legacy-webrtc")]
    pub use crate::call::{CallManager, CallManagerConfig};
    pub use crate::identity::{PeerIdentity, PeerIdentityString};
    #[cfg(feature = "legacy-webrtc")]
    pub use crate::media::{MediaEvent, MediaStreamManager};
    pub use crate::protocol_handler::{WebRtcHandlerConfig, WebRtcIncoming, WebRtcProtocolHandler};
    #[cfg(feature = "legacy-webrtc")]
    pub use crate::service::{WebRtcConfig, WebRtcEvent, WebRtcService, WebRtcServiceBuilder};
    pub use crate::signaling::{SignalingHandler, SignalingMessage, SignalingTransport};
    pub use crate::transport::{AntQuicTransport, TransportConfig};
    pub use crate::types::{
        CallEvent, CallId, CallState, MediaConstraints, MediaType, NativeQuicConfiguration,
    };
}
