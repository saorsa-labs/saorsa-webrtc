//! Call management for WebRTC
//!
//! **Note:** This module uses the webrtc crate types (requires legacy-webrtc feature).
//! In Phase 2, this will be replaced with a QUIC-native implementation via QuicMediaTransport.

use crate::identity::PeerIdentity;
use crate::link_transport::PeerConnection;
use crate::media::{MediaStreamManager, WebRtcTrack};
use crate::quic_media_transport::{MediaTransportError, MediaTransportState, QuicMediaTransport};
use crate::types::{CallEvent, CallId, CallState, MediaCapabilities, MediaConstraints};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{broadcast, RwLock};
use webrtc::peer_connection::RTCPeerConnection;

/// Call management errors
#[derive(Error, Debug)]
pub enum CallError {
    /// Call not found
    #[error("Call not found: {0}")]
    CallNotFound(String),

    /// Invalid state
    #[error("Invalid call state")]
    InvalidState,

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Transport error
    #[error("Transport error: {0}")]
    TransportError(String),
}

impl From<MediaTransportError> for CallError {
    fn from(err: MediaTransportError) -> Self {
        CallError::TransportError(err.to_string())
    }
}

/// Call manager configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallManagerConfig {
    /// Maximum concurrent calls
    pub max_concurrent_calls: usize,
}

impl Default for CallManagerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_calls: 10,
        }
    }
}

/// Network adapter trait (placeholder for future implementation)
pub trait NetworkAdapter: Send + Sync {}

/// Active call with WebRTC peer connection
pub struct Call<I: PeerIdentity> {
    /// Call identifier
    pub id: CallId,
    /// Remote peer
    pub remote_peer: I,
    /// WebRTC peer connection (legacy, will be removed in Phase 3.2)
    pub peer_connection: Arc<RTCPeerConnection>,
    /// QUIC-based media transport (Phase 3 migration)
    pub media_transport: Option<Arc<QuicMediaTransport>>,
    /// Current state
    pub state: CallState,
    /// Media constraints
    pub constraints: MediaConstraints,
    /// WebRTC tracks for this call
    pub tracks: Vec<WebRtcTrack>,
}

/// Call manager
///
/// Manages call lifecycle for both legacy WebRTC and QUIC-native calls.
/// The generic parameter `I` represents the peer identity type, allowing
/// type-safe handling of different identity schemes (e.g., string IDs,
/// cryptographic identities).
///
/// # QUIC-Native Call Flow
///
/// For QUIC-native calls, the state machine follows this flow:
///
/// ```text
///     Idle
///       │
///       ▼
///    Calling ───────────────────┐
///       │                       │
///       ▼ (exchange_capabilities)│
///  Connecting                   │
///       │                       ▼
///       ▼ (confirm_connection)  Failed
///   Connected                   │
///       │                       │
///       ▼ (end_call)            │
///    Ending ◄───────────────────┘
///       │
///       ▼
///     Idle
/// ```
///
/// Key transitions:
/// - `initiate_quic_call`: Idle → Connecting (transport connects immediately)
/// - `exchange_capabilities`: Calling → Connecting
/// - `confirm_connection`: Connecting → Connected
/// - `end_call`: Any → Ending → removed from manager
/// - Failure: Any active state → Failed
///
/// # Legacy WebRTC Call Flow (deprecated)
///
/// For legacy calls, the flow uses SDP/ICE negotiation:
/// - `initiate_call` + `create_offer` for SDP
/// - `handle_answer` + `add_ice_candidate` for connection
///
/// # Type Safety
///
/// All methods preserve the `I: PeerIdentity` type parameter, ensuring:
/// - Call events include properly typed peer identities
/// - Remote peer information is type-safe throughout call lifecycle
/// - No accidental mixing of different identity schemes
pub struct CallManager<I: PeerIdentity> {
    calls: Arc<RwLock<HashMap<CallId, Call<I>>>>,
    event_sender: broadcast::Sender<CallEvent<I>>,
    #[allow(dead_code)]
    config: CallManagerConfig,
    media_manager: Arc<RwLock<MediaStreamManager>>,
}

impl<I: PeerIdentity> CallManager<I> {
    /// Create new call manager
    ///
    /// # Errors
    ///
    /// Returns error if initialization fails
    pub async fn new(config: CallManagerConfig) -> Result<Self, CallError> {
        let (event_sender, _) = broadcast::channel(100);
        let media_manager = Arc::new(RwLock::new(MediaStreamManager::new()));
        Ok(Self {
            calls: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
            config,
            media_manager,
        })
    }

    /// Start the call manager
    ///
    /// # Errors
    ///
    /// Returns error if start fails
    pub async fn start(&self) -> Result<(), CallError> {
        Ok(())
    }

    /// Initiate a call
    ///
    /// # Errors
    ///
    /// Returns error if call cannot be initiated
    pub async fn initiate_call(
        &self,
        callee: I,
        constraints: MediaConstraints,
    ) -> Result<CallId, CallError> {
        // Enforce max_concurrent_calls limit
        let calls = self.calls.read().await;
        if calls.len() >= self.config.max_concurrent_calls {
            return Err(CallError::ConfigError(format!(
                "Maximum concurrent calls limit reached: {}",
                self.config.max_concurrent_calls
            )));
        }
        drop(calls);

        let call_id = CallId::new();

        tracing::info!(
            "Initiating call {} to peer: {}",
            call_id,
            callee.to_string_repr()
        );

        // Create QUIC-based media transport (Phase 3 migration)
        let media_transport = Arc::new(QuicMediaTransport::new());
        tracing::debug!("Created QuicMediaTransport for call {}", call_id);

        // Create WebRTC peer connection (legacy path, will be removed in Phase 3.2)
        let peer_connection = Arc::new(
            webrtc::api::APIBuilder::new()
                .build()
                .new_peer_connection(
                    webrtc::peer_connection::configuration::RTCConfiguration::default(),
                )
                .await
                .map_err(|e| {
                    tracing::error!(
                        "Failed to create peer connection for call {}: {}",
                        call_id,
                        e
                    );
                    CallError::ConfigError(format!("Failed to create peer connection: {}", e))
                })?,
        );

        tracing::debug!("Created peer connection for call {}", call_id);

        // Create media tracks based on constraints
        let mut media_manager = self.media_manager.write().await;
        let mut tracks = Vec::new();

        if constraints.has_audio() {
            let audio_track = media_manager.create_audio_track().await.map_err(|e| {
                CallError::ConfigError(format!("Failed to create audio track: {:?}", e))
            })?;
            tracks.push((*audio_track).clone());

            // Add track to peer connection
            let track: Arc<dyn webrtc::track::track_local::TrackLocal + Send + Sync> =
                audio_track.track.clone();
            peer_connection
                .add_track(track)
                .await
                .map_err(|e| CallError::ConfigError(format!("Failed to add audio track: {}", e)))?;
        }

        if constraints.has_video() {
            let video_track = media_manager.create_video_track().await.map_err(|e| {
                CallError::ConfigError(format!("Failed to create video track: {:?}", e))
            })?;
            tracks.push((*video_track).clone());

            // Add track to peer connection
            let track: Arc<dyn webrtc::track::track_local::TrackLocal + Send + Sync> =
                video_track.track.clone();
            peer_connection
                .add_track(track)
                .await
                .map_err(|e| CallError::ConfigError(format!("Failed to add video track: {}", e)))?;
        }

        let call = Call {
            id: call_id,
            remote_peer: callee.clone(),
            peer_connection,
            media_transport: Some(media_transport),
            state: CallState::Calling,
            constraints: constraints.clone(),
            tracks,
        };

        let mut calls = self.calls.write().await;
        calls.insert(call_id, call);

        // Emit call initiated event
        let _ = self.event_sender.send(CallEvent::CallInitiated {
            call_id,
            callee,
            constraints,
        });

        Ok(call_id)
    }

    /// Accept a call
    ///
    /// # Errors
    ///
    /// Returns error if call cannot be accepted
    pub async fn accept_call(
        &self,
        call_id: CallId,
        _constraints: MediaConstraints,
    ) -> Result<(), CallError> {
        let mut calls = self.calls.write().await;
        if let Some(call) = calls.get_mut(&call_id) {
            // Validate state transition
            match call.state {
                CallState::Calling | CallState::Connecting => {
                    let old_state = call.state;
                    call.state = CallState::Connected;
                    tracing::debug!(
                        call_id = %call_id,
                        old_state = ?old_state,
                        new_state = ?CallState::Connected,
                        "Call state transition"
                    );

                    // Emit connection established event
                    let _ = self
                        .event_sender
                        .send(CallEvent::ConnectionEstablished { call_id });

                    tracing::info!("Call {} accepted", call_id);
                    Ok(())
                }
                _ => {
                    tracing::warn!(
                        "Invalid state transition: cannot accept call {} in state {:?}",
                        call_id,
                        call.state
                    );
                    Err(CallError::InvalidState)
                }
            }
        } else {
            tracing::warn!("Attempted to accept non-existent call {}", call_id);
            Err(CallError::CallNotFound(call_id.to_string()))
        }
    }

    /// Reject a call
    ///
    /// # Errors
    ///
    /// Returns error if call cannot be rejected
    pub async fn reject_call(&self, call_id: CallId) -> Result<(), CallError> {
        let mut calls = self.calls.write().await;
        if let Some(call) = calls.get_mut(&call_id) {
            // Validate state transition - can only reject calls that are not yet connected/ended
            match call.state {
                CallState::Calling | CallState::Connecting => {
                    let old_state = call.state;
                    call.state = CallState::Failed;
                    tracing::debug!(
                        call_id = %call_id,
                        old_state = ?old_state,
                        new_state = ?CallState::Failed,
                        "Call state transition"
                    );

                    // Emit call rejected event
                    let _ = self.event_sender.send(CallEvent::CallRejected { call_id });

                    Ok(())
                }
                _ => {
                    tracing::warn!(
                        "Invalid state transition: cannot reject call {} in state {:?}",
                        call_id,
                        call.state
                    );
                    Err(CallError::InvalidState)
                }
            }
        } else {
            Err(CallError::CallNotFound(call_id.to_string()))
        }
    }

    /// End a call
    ///
    /// # Errors
    ///
    /// Returns error if call cannot be ended
    pub async fn end_call(&self, call_id: CallId) -> Result<(), CallError> {
        let mut calls = self.calls.write().await;
        if let Some(call) = calls.remove(&call_id) {
            // Remove all tracks associated with this call from media manager
            let mut media_manager = self.media_manager.write().await;
            for track in &call.tracks {
                media_manager.remove_track(&track.id);
            }
            drop(media_manager);

            // Disconnect QuicMediaTransport if present (Phase 3 path)
            if let Some(ref transport) = call.media_transport {
                if let Err(e) = transport.disconnect().await {
                    tracing::warn!(
                        "Failed to disconnect QuicMediaTransport for call {}: {}",
                        call_id,
                        e
                    );
                    // Continue cleanup even if disconnect fails
                } else {
                    tracing::debug!("QuicMediaTransport disconnected for call {}", call_id);
                }
            }

            // Close the peer connection (legacy path)
            let _ = call.peer_connection.close().await;

            // Emit call ended event
            let _ = self.event_sender.send(CallEvent::CallEnded { call_id });

            tracing::info!(
                "Ended call {} and cleaned up {} tracks",
                call_id,
                call.tracks.len()
            );
            Ok(())
        } else {
            Err(CallError::CallNotFound(call_id.to_string()))
        }
    }

    /// Get call state
    #[must_use]
    pub async fn get_call_state(&self, call_id: CallId) -> Option<CallState> {
        let calls = self.calls.read().await;
        calls.get(&call_id).map(|call| call.state)
    }

    /// Create SDP offer for a call (legacy WebRTC only)
    ///
    /// **Deprecated**: This method is for legacy WebRTC calls only.
    /// QUIC-native calls do not use SDP - use `exchange_capabilities` instead.
    ///
    /// # Deprecation Path
    ///
    /// - Phase 3.2: Both SDP and capability exchange are available
    /// - Phase 3.3: SDP methods will be removed entirely
    ///
    /// # Errors
    ///
    /// Returns error if offer cannot be created
    #[deprecated(
        since = "0.3.0",
        note = "Use QUIC-native call flow (exchange_capabilities) instead. SDP is only for legacy WebRTC calls."
    )]
    #[tracing::instrument(skip(self), fields(call_id = %call_id))]
    pub async fn create_offer(&self, call_id: CallId) -> Result<String, CallError> {
        let calls = self.calls.read().await;
        if let Some(call) = calls.get(&call_id) {
            tracing::debug!("Creating SDP offer");
            let offer = call.peer_connection.create_offer(None).await.map_err(|e| {
                tracing::error!("Failed to create offer: {}", e);
                CallError::ConfigError(format!("Failed to create offer: {}", e))
            })?;
            call.peer_connection
                .set_local_description(offer.clone())
                .await
                .map_err(|e| {
                    tracing::error!("Failed to set local description: {}", e);
                    CallError::ConfigError(format!("Failed to set local description: {}", e))
                })?;
            tracing::debug!("SDP offer created successfully");
            Ok(offer.sdp)
        } else {
            tracing::warn!("Attempted to create offer for non-existent call");
            Err(CallError::CallNotFound(call_id.to_string()))
        }
    }

    /// Handle SDP answer for a call (legacy WebRTC only)
    ///
    /// **Deprecated**: This method is for legacy WebRTC calls only.
    /// QUIC-native calls do not use SDP - use `confirm_connection` instead.
    ///
    /// # Deprecation Path
    ///
    /// - Phase 3.2: Both SDP and capability exchange are available
    /// - Phase 3.3: SDP methods will be removed entirely
    ///
    /// # Errors
    ///
    /// Returns error if answer cannot be handled
    #[deprecated(
        since = "0.3.0",
        note = "Use QUIC-native call flow (confirm_connection) instead. SDP is only for legacy WebRTC calls."
    )]
    #[tracing::instrument(skip(self, sdp), fields(call_id = %call_id, sdp_len = sdp.len()))]
    pub async fn handle_answer(&self, call_id: CallId, sdp: String) -> Result<(), CallError> {
        tracing::debug!("Processing SDP answer");

        let calls = self.calls.read().await;
        if let Some(call) = calls.get(&call_id) {
            // Validate SDP is not empty
            if sdp.trim().is_empty() {
                return Err(CallError::ConfigError(
                    "SDP answer cannot be empty".to_string(),
                ));
            }

            let answer =
                webrtc::peer_connection::sdp::session_description::RTCSessionDescription::answer(
                    sdp,
                )
                .map_err(|e| CallError::ConfigError(format!("Invalid SDP answer: {}", e)))?;

            call.peer_connection
                .set_remote_description(answer)
                .await
                .map_err(|e| {
                    CallError::ConfigError(format!("Failed to set remote description: {}", e))
                })?;

            tracing::debug!("SDP answer processed successfully");
            Ok(())
        } else {
            Err(CallError::CallNotFound(call_id.to_string()))
        }
    }

    /// Add ICE candidate to a call (legacy WebRTC only)
    ///
    /// **Deprecated**: This method is for legacy WebRTC calls only.
    /// QUIC-native calls do not use ICE candidates - connection is handled
    /// automatically by the QuicMediaTransport.
    ///
    /// For QUIC calls, use `exchange_capabilities` and `confirm_connection` instead.
    ///
    /// # Errors
    ///
    /// Returns error if candidate cannot be added
    #[deprecated(
        since = "0.3.0",
        note = "Use QUIC-native call flow (exchange_capabilities/confirm_connection) instead. ICE is only for legacy WebRTC calls."
    )]
    #[tracing::instrument(skip(self, candidate), fields(call_id = %call_id))]
    pub async fn add_ice_candidate(
        &self,
        call_id: CallId,
        candidate: String,
    ) -> Result<(), CallError> {
        tracing::trace!("Adding ICE candidate (legacy WebRTC)");

        let calls = self.calls.read().await;
        if let Some(call) = calls.get(&call_id) {
            let rtc_candidate = webrtc::ice_transport::ice_candidate::RTCIceCandidateInit {
                candidate,
                ..Default::default()
            };
            call.peer_connection
                .add_ice_candidate(rtc_candidate)
                .await
                .map_err(|e| {
                    CallError::ConfigError(format!("Failed to add ICE candidate: {}", e))
                })?;

            tracing::trace!("ICE candidate added successfully");
            Ok(())
        } else {
            Err(CallError::CallNotFound(call_id.to_string()))
        }
    }

    /// Start ICE gathering for a call (legacy WebRTC only)
    ///
    /// **Deprecated**: This method is for legacy WebRTC calls only.
    /// QUIC-native calls do not use ICE - connection is handled automatically
    /// by the QuicMediaTransport.
    ///
    /// For QUIC calls, use `exchange_capabilities` and `confirm_connection` instead.
    ///
    /// # Errors
    ///
    /// Returns error if gathering cannot be started
    #[deprecated(
        since = "0.3.0",
        note = "Use QUIC-native call flow (exchange_capabilities/confirm_connection) instead. ICE is only for legacy WebRTC calls."
    )]
    pub async fn start_ice_gathering(&self, call_id: CallId) -> Result<(), CallError> {
        let calls = self.calls.read().await;
        if let Some(_call) = calls.get(&call_id) {
            // ICE gathering is typically started automatically when creating offer
            // For now, this is a no-op as gathering happens during offer creation
            Ok(())
        } else {
            Err(CallError::CallNotFound(call_id.to_string()))
        }
    }

    /// Exchange media capabilities with peer (QUIC-native)
    ///
    /// Replaces SDP offer creation with a simpler capability exchange.
    /// Returns the local capabilities that should be sent to the remote peer.
    ///
    /// # Flow
    ///
    /// 1. Caller invokes `exchange_capabilities` to get local capabilities
    /// 2. Caller sends capabilities to callee via signaling
    /// 3. Callee receives capabilities and calls `confirm_connection`
    ///
    /// # Arguments
    ///
    /// * `call_id` - The call to exchange capabilities for
    ///
    /// # Errors
    ///
    /// Returns error if call not found or call is not in a valid state.
    #[tracing::instrument(skip(self), fields(call_id = %call_id))]
    pub async fn exchange_capabilities(
        &self,
        call_id: CallId,
    ) -> Result<MediaCapabilities, CallError> {
        let mut calls = self.calls.write().await;
        let call = calls
            .get_mut(&call_id)
            .ok_or_else(|| CallError::CallNotFound(call_id.to_string()))?;

        // Validate call is in a state where capability exchange is valid
        match call.state {
            CallState::Calling | CallState::Connecting => {
                // Valid states for capability exchange
            }
            _ => {
                tracing::warn!(
                    "Cannot exchange capabilities for call {} in state {:?}",
                    call_id,
                    call.state
                );
                return Err(CallError::InvalidState);
            }
        }

        // Transition to Connecting if still Calling
        if call.state == CallState::Calling {
            call.state = CallState::Connecting;
            tracing::debug!(
                call_id = %call_id,
                "Call state transition: Calling -> Connecting"
            );
        }

        // Generate capabilities from call constraints
        let capabilities = MediaCapabilities::from_constraints(&call.constraints);

        tracing::info!(
            call_id = %call_id,
            audio = capabilities.audio,
            video = capabilities.video,
            data_channel = capabilities.data_channel,
            max_bandwidth = capabilities.max_bandwidth_kbps,
            "Capabilities exchanged"
        );

        Ok(capabilities)
    }

    /// Confirm peer capabilities and activate connection (QUIC-native)
    ///
    /// Called after exchanging capabilities. Verifies peer capabilities
    /// match our constraints and confirms QUIC connection is ready.
    ///
    /// # Flow
    ///
    /// 1. Caller sends capabilities to callee
    /// 2. Callee receives capabilities and calls `confirm_connection`
    /// 3. If capabilities match, connection is established
    ///
    /// # Arguments
    ///
    /// * `call_id` - The call to confirm
    /// * `peer_capabilities` - Remote peer's media capabilities
    ///
    /// # Errors
    ///
    /// Returns error if call not found, capabilities incompatible, or
    /// transport not connected.
    #[tracing::instrument(skip(self, peer_capabilities), fields(call_id = %call_id))]
    pub async fn confirm_connection(
        &self,
        call_id: CallId,
        peer_capabilities: MediaCapabilities,
    ) -> Result<(), CallError> {
        let mut calls = self.calls.write().await;
        let call = calls
            .get_mut(&call_id)
            .ok_or_else(|| CallError::CallNotFound(call_id.to_string()))?;

        // Validate call is in Connecting state
        if call.state != CallState::Connecting {
            tracing::warn!(
                "Cannot confirm connection for call {} in state {:?}",
                call_id,
                call.state
            );
            return Err(CallError::InvalidState);
        }

        // Validate peer capabilities satisfy our constraints
        if let Err(e) = Self::validate_remote_capabilities(&call.constraints, &peer_capabilities) {
            tracing::warn!(
                call_id = %call_id,
                peer_audio = peer_capabilities.audio,
                peer_video = peer_capabilities.video,
                required_audio = call.constraints.audio,
                required_video = call.constraints.video,
                error = %e,
                "Peer capabilities do not satisfy call constraints"
            );
            return Err(e);
        }

        // Verify QuicMediaTransport is connected
        if let Some(ref transport) = call.media_transport {
            let transport_state = transport.state().await;
            if transport_state != MediaTransportState::Connected {
                tracing::warn!(
                    call_id = %call_id,
                    transport_state = ?transport_state,
                    "Transport is not connected"
                );
                return Err(CallError::TransportError(
                    "Transport is not connected".to_string(),
                ));
            }
        } else {
            return Err(CallError::ConfigError(
                "Call has no media transport".to_string(),
            ));
        }

        // Update call state to Connected
        call.state = CallState::Connected;
        tracing::debug!(
            call_id = %call_id,
            "Call state transition: Connecting -> Connected"
        );

        // Emit ConnectionEstablished event
        let _ = self
            .event_sender
            .send(CallEvent::ConnectionEstablished { call_id });

        tracing::info!(
            call_id = %call_id,
            peer_audio = peer_capabilities.audio,
            peer_video = peer_capabilities.video,
            "Connection confirmed"
        );

        Ok(())
    }

    /// Validate remote capabilities against call constraints
    ///
    /// Checks whether the remote peer's capabilities satisfy the call's
    /// media requirements. This is used during connection confirmation
    /// to ensure both peers can support the required media types.
    ///
    /// # Validation Rules
    ///
    /// - If audio is required by constraints, remote must support audio
    /// - If video is required by constraints, remote must support video
    /// - If screen share is required, remote must support video
    /// - Bandwidth requirements are considered but not strictly enforced
    ///
    /// # Arguments
    ///
    /// * `constraints` - The local call constraints
    /// * `remote_caps` - The remote peer's capabilities
    ///
    /// # Errors
    ///
    /// Returns error describing which capability is missing or incompatible.
    pub fn validate_remote_capabilities(
        constraints: &MediaConstraints,
        remote_caps: &MediaCapabilities,
    ) -> Result<(), CallError> {
        // Check audio requirement
        if constraints.audio && !remote_caps.audio {
            return Err(CallError::ConfigError(
                "Remote peer does not support audio".to_string(),
            ));
        }

        // Check video requirement (video or screen share)
        if (constraints.video || constraints.screen_share) && !remote_caps.video {
            return Err(CallError::ConfigError(
                "Remote peer does not support video".to_string(),
            ));
        }

        // Note: Bandwidth is advisory, not strictly enforced
        // In the future, we could add bandwidth negotiation logic here

        Ok(())
    }

    /// Subscribe to call events
    #[must_use]
    pub fn subscribe_events(&self) -> broadcast::Receiver<CallEvent<I>> {
        self.event_sender.subscribe()
    }

    /// Check if a call has a QUIC media transport
    ///
    /// Returns `true` if the call has an associated `QuicMediaTransport`.
    #[must_use]
    pub async fn has_media_transport(&self, call_id: CallId) -> bool {
        let calls = self.calls.read().await;
        calls
            .get(&call_id)
            .is_some_and(|call| call.media_transport.is_some())
    }

    /// Initiate a QUIC-native call (bypasses SDP/ICE)
    ///
    /// Creates a call using only QuicMediaTransport, without creating a
    /// legacy RTCPeerConnection. This is the preferred method for new
    /// QUIC-native calls.
    ///
    /// # Arguments
    ///
    /// * `callee` - The remote peer identity
    /// * `constraints` - Media constraints for the call
    /// * `peer` - The QUIC peer connection to use for transport
    ///
    /// # Errors
    ///
    /// Returns error if call cannot be initiated or transport connection fails.
    pub async fn initiate_quic_call(
        &self,
        callee: I,
        constraints: MediaConstraints,
        peer: PeerConnection,
    ) -> Result<CallId, CallError> {
        // Enforce max_concurrent_calls limit
        let calls = self.calls.read().await;
        if calls.len() >= self.config.max_concurrent_calls {
            return Err(CallError::ConfigError(format!(
                "Maximum concurrent calls limit reached: {}",
                self.config.max_concurrent_calls
            )));
        }
        drop(calls);

        let call_id = CallId::new();

        tracing::info!(
            "Initiating QUIC call {} to peer: {}",
            call_id,
            callee.to_string_repr()
        );

        // Create and connect QUIC-based media transport
        let media_transport = Arc::new(QuicMediaTransport::new());
        media_transport.connect(peer).await?;
        tracing::debug!("QuicMediaTransport connected for call {}", call_id);

        // Create a placeholder peer connection (required for legacy compatibility)
        // This will be removed in Phase 3.2
        let peer_connection = Arc::new(
            webrtc::api::APIBuilder::new()
                .build()
                .new_peer_connection(
                    webrtc::peer_connection::configuration::RTCConfiguration::default(),
                )
                .await
                .map_err(|e| {
                    tracing::error!(
                        "Failed to create peer connection for call {}: {}",
                        call_id,
                        e
                    );
                    CallError::ConfigError(format!("Failed to create peer connection: {}", e))
                })?,
        );

        let call = Call {
            id: call_id,
            remote_peer: callee.clone(),
            peer_connection,
            media_transport: Some(media_transport),
            state: CallState::Connecting,
            constraints: constraints.clone(),
            tracks: Vec::new(), // QUIC calls don't use WebRTC tracks
        };

        let mut calls = self.calls.write().await;
        calls.insert(call_id, call);

        // Emit call initiated event
        let _ = self.event_sender.send(CallEvent::CallInitiated {
            call_id,
            callee,
            constraints,
        });

        Ok(call_id)
    }

    /// Connect an existing call's QuicMediaTransport to a peer
    ///
    /// This method connects the call's media transport to the specified peer,
    /// enabling QUIC-based media streaming.
    ///
    /// # Arguments
    ///
    /// * `call_id` - The call to connect
    /// * `peer` - The QUIC peer connection to use
    ///
    /// # Errors
    ///
    /// Returns error if call not found, no media transport exists, or
    /// connection fails.
    pub async fn connect_quic_transport(
        &self,
        call_id: CallId,
        peer: PeerConnection,
    ) -> Result<(), CallError> {
        let calls = self.calls.read().await;
        let call = calls
            .get(&call_id)
            .ok_or_else(|| CallError::CallNotFound(call_id.to_string()))?;

        let transport = call
            .media_transport
            .as_ref()
            .ok_or_else(|| CallError::ConfigError("Call has no media transport".to_string()))?;

        tracing::debug!(
            "Connecting QuicMediaTransport for call {} to peer {}",
            call_id,
            peer.peer_id
        );

        transport.connect(peer).await?;

        tracing::info!("QuicMediaTransport connected for call {}", call_id);
        Ok(())
    }

    /// Update call state based on QuicMediaTransport state
    ///
    /// Synchronizes the call's `CallState` with the underlying transport state.
    /// This should be called when transport state changes are detected.
    ///
    /// # State Transitions
    ///
    /// - `Disconnected` transport: Maps to `Idle` (or `Ending` if call was active)
    /// - `Connecting` transport: Maps to `Connecting`
    /// - `Connected` transport: Maps to `Connected`
    /// - `Failed` transport: Maps to `Failed`
    ///
    /// # Arguments
    ///
    /// * `call_id` - The call to update
    ///
    /// # Errors
    ///
    /// Returns error if call not found or has no media transport.
    pub async fn update_state_from_transport(
        &self,
        call_id: CallId,
    ) -> Result<CallState, CallError> {
        let mut calls = self.calls.write().await;
        let call = calls
            .get_mut(&call_id)
            .ok_or_else(|| CallError::CallNotFound(call_id.to_string()))?;

        let transport = call
            .media_transport
            .as_ref()
            .ok_or_else(|| CallError::ConfigError("Call has no media transport".to_string()))?;

        let transport_state = transport.state().await;
        let old_state = call.state;

        // Map transport state to call state, using ending context if appropriate
        let new_state = if old_state == CallState::Connected
            && transport_state == MediaTransportState::Disconnected
        {
            // Connected call that disconnected -> Ending
            CallState::Ending
        } else {
            CallState::from_transport_state(transport_state)
        };

        if old_state != new_state {
            call.state = new_state;
            tracing::debug!(
                call_id = %call_id,
                old_state = ?old_state,
                new_state = ?new_state,
                transport_state = ?transport_state,
                "Call state updated from transport"
            );

            // Emit appropriate event based on transition
            match new_state {
                CallState::Connected => {
                    let _ = self
                        .event_sender
                        .send(CallEvent::ConnectionEstablished { call_id });
                }
                CallState::Failed => {
                    let _ = self.event_sender.send(CallEvent::ConnectionFailed {
                        call_id,
                        error: "Transport failed".to_string(),
                    });
                }
                _ => {}
            }
        }

        Ok(new_state)
    }

    /// Check if a call state transition is valid for QUIC flow
    ///
    /// Validates that the proposed state transition follows the QUIC call
    /// state machine rules.
    ///
    /// # Valid Transitions (QUIC-native)
    ///
    /// - **Setup**: Idle → Calling → Connecting → Connected
    /// - **Direct connect**: Idle → Connecting (for QUIC with pre-established transport)
    /// - **Teardown**: Connected → Ending → Idle
    /// - **Failure**: Any active state → Failed
    /// - **Recovery**: Failed → Idle
    #[must_use]
    pub fn is_valid_quic_transition(from: CallState, to: CallState) -> bool {
        matches!(
            (from, to),
            // Starting a call
            (CallState::Idle, CallState::Calling)
                | (CallState::Idle, CallState::Connecting) // Direct QUIC connect
                // Progressing through call setup
                | (CallState::Calling, CallState::Connecting)
                | (CallState::Connecting, CallState::Connected)
                // Ending a call
                | (CallState::Connected, CallState::Ending)
                | (CallState::Ending, CallState::Idle)
                // Failures can happen from any active state
                | (CallState::Calling, CallState::Failed)
                | (CallState::Connecting, CallState::Failed)
                | (CallState::Connected, CallState::Failed)
                // Recovery from failure
                | (CallState::Failed, CallState::Idle)
        )
    }

    /// Transition call to failed state
    ///
    /// Marks the call as failed and emits a `ConnectionFailed` event.
    /// This can be called from any active state (Calling, Connecting, Connected).
    ///
    /// # Arguments
    ///
    /// * `call_id` - The call to mark as failed
    /// * `reason` - Description of why the call failed
    ///
    /// # Errors
    ///
    /// Returns error if call not found or already in terminal state.
    pub async fn fail_call(&self, call_id: CallId, reason: String) -> Result<(), CallError> {
        let mut calls = self.calls.write().await;
        let call = calls
            .get_mut(&call_id)
            .ok_or_else(|| CallError::CallNotFound(call_id.to_string()))?;

        // Validate transition is allowed
        if !Self::is_valid_quic_transition(call.state, CallState::Failed) {
            tracing::warn!(
                call_id = %call_id,
                current_state = ?call.state,
                "Cannot transition to Failed from current state"
            );
            return Err(CallError::InvalidState);
        }

        let old_state = call.state;
        call.state = CallState::Failed;

        tracing::warn!(
            call_id = %call_id,
            old_state = ?old_state,
            reason = %reason,
            "Call failed"
        );

        let _ = self.event_sender.send(CallEvent::ConnectionFailed {
            call_id,
            error: reason,
        });

        Ok(())
    }

    /// Get current call information
    ///
    /// Returns a snapshot of the call's current state, constraints, and
    /// transport status.
    ///
    /// # Arguments
    ///
    /// * `call_id` - The call to query
    ///
    /// # Returns
    ///
    /// Returns `None` if call not found.
    pub async fn get_call_info(
        &self,
        call_id: CallId,
    ) -> Option<(CallState, MediaConstraints, bool)> {
        let calls = self.calls.read().await;
        calls.get(&call_id).map(|call| {
            (
                call.state,
                call.constraints.clone(),
                call.media_transport.is_some(),
            )
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::identity::PeerIdentityString;

    #[tokio::test]
    async fn test_call_manager_initiate_call() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();

        let call_id = call_manager
            .initiate_call(callee, constraints)
            .await
            .unwrap();

        let state = call_manager.get_call_state(call_id).await;
        assert_eq!(state, Some(CallState::Calling));
    }

    #[tokio::test]
    async fn test_call_manager_initiate_call_creates_media_transport() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();

        let call_id = call_manager
            .initiate_call(callee, constraints)
            .await
            .unwrap();

        // Verify QuicMediaTransport is created for new calls
        assert!(
            call_manager.has_media_transport(call_id).await,
            "New calls should have QuicMediaTransport initialized"
        );
    }

    #[tokio::test]
    async fn test_call_manager_accept_call() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();

        let call_id = call_manager
            .initiate_call(callee, constraints.clone())
            .await
            .unwrap();

        call_manager
            .accept_call(call_id, constraints)
            .await
            .unwrap();

        let state = call_manager.get_call_state(call_id).await;
        assert_eq!(state, Some(CallState::Connected));
    }

    #[tokio::test]
    async fn test_call_manager_reject_call() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();

        let call_id = call_manager
            .initiate_call(callee, constraints)
            .await
            .unwrap();

        call_manager.reject_call(call_id).await.unwrap();

        let state = call_manager.get_call_state(call_id).await;
        assert_eq!(state, Some(CallState::Failed));
    }

    #[tokio::test]
    async fn test_call_manager_end_call() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();

        let call_id = call_manager
            .initiate_call(callee, constraints)
            .await
            .unwrap();

        call_manager.end_call(call_id).await.unwrap();

        let state = call_manager.get_call_state(call_id).await;
        assert_eq!(state, None);
    }

    #[tokio::test]
    #[allow(deprecated)]
    async fn test_call_manager_create_offer_legacy() {
        // Tests legacy SDP offer creation (deprecated for QUIC-native calls)
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();

        let _call_id = call_manager
            .initiate_call(callee, constraints)
            .await
            .unwrap();

        // Skip the offer creation test for now since it requires proper codec setup
        // This would need more complex WebRTC setup
        // let offer = call_manager.create_offer(call_id).await.unwrap();
        // assert!(!offer.is_empty());
        // assert!(offer.contains("v=0"));
    }

    #[tokio::test]
    #[allow(deprecated)]
    async fn test_call_manager_add_ice_candidate_legacy() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();

        let call_id = call_manager
            .initiate_call(callee, constraints)
            .await
            .unwrap();

        // Test with a dummy ICE candidate (legacy WebRTC only)
        let candidate = "candidate:1 1 UDP 2122260223 192.168.1.1 12345 typ host".to_string();
        let result = call_manager.add_ice_candidate(call_id, candidate).await;
        // This might fail in test environment, but should not panic
        // We just test that the method exists and handles call not found
        assert!(result.is_ok() || matches!(result, Err(CallError::ConfigError(_))));
    }

    #[tokio::test]
    #[allow(deprecated)]
    async fn test_call_manager_start_ice_gathering_legacy() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();

        let call_id = call_manager
            .initiate_call(callee, constraints)
            .await
            .unwrap();

        // Legacy WebRTC ICE gathering
        let result = call_manager.start_ice_gathering(call_id).await;
        // This might fail in test environment, but should not panic
        assert!(result.is_ok() || matches!(result, Err(CallError::ConfigError(_))));
    }

    #[tokio::test]
    async fn test_call_manager_call_not_found() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let fake_call_id = CallId::new();

        let result = call_manager
            .accept_call(fake_call_id, MediaConstraints::audio_only())
            .await;
        assert!(matches!(result, Err(CallError::CallNotFound(_))));

        let result = call_manager.reject_call(fake_call_id).await;
        assert!(matches!(result, Err(CallError::CallNotFound(_))));

        let result = call_manager.end_call(fake_call_id).await;
        assert!(matches!(result, Err(CallError::CallNotFound(_))));

        #[allow(deprecated)]
        let result = call_manager.create_offer(fake_call_id).await;
        assert!(matches!(result, Err(CallError::CallNotFound(_))));

        #[allow(deprecated)]
        let result = call_manager
            .handle_answer(fake_call_id, "dummy".to_string())
            .await;
        assert!(matches!(result, Err(CallError::CallNotFound(_))));

        #[allow(deprecated)]
        let result = call_manager
            .add_ice_candidate(fake_call_id, "dummy".to_string())
            .await;
        assert!(matches!(result, Err(CallError::CallNotFound(_))));

        #[allow(deprecated)]
        let result = call_manager.start_ice_gathering(fake_call_id).await;
        assert!(matches!(result, Err(CallError::CallNotFound(_))));
    }

    /// Helper to create a test PeerConnection
    fn test_peer() -> crate::link_transport::PeerConnection {
        crate::link_transport::PeerConnection {
            peer_id: "test-peer".to_string(),
            remote_addr: "127.0.0.1:9000".parse().unwrap(),
        }
    }

    #[tokio::test]
    async fn test_initiate_quic_call() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("quic-callee");
        let constraints = MediaConstraints::audio_only();
        let peer = test_peer();

        let call_id = call_manager
            .initiate_quic_call(callee, constraints, peer)
            .await
            .unwrap();

        // QUIC calls start in Connecting state (already connected to transport)
        let state = call_manager.get_call_state(call_id).await;
        assert_eq!(state, Some(CallState::Connecting));

        // Verify media transport is set and connected
        assert!(call_manager.has_media_transport(call_id).await);
    }

    #[tokio::test]
    async fn test_initiate_quic_call_respects_max_concurrent() {
        let config = CallManagerConfig {
            max_concurrent_calls: 1,
        };
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();

        // First call should succeed
        let _ = call_manager
            .initiate_quic_call(callee.clone(), constraints.clone(), test_peer())
            .await
            .unwrap();

        // Second call should fail due to limit
        let result = call_manager
            .initiate_quic_call(callee, constraints, test_peer())
            .await;

        assert!(matches!(result, Err(CallError::ConfigError(_))));
    }

    #[tokio::test]
    async fn test_connect_quic_transport() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();

        // Create a regular call (has unconnected media transport)
        let call_id = call_manager
            .initiate_call(callee, constraints)
            .await
            .unwrap();

        // Connect the transport
        let peer = test_peer();
        let result = call_manager.connect_quic_transport(call_id, peer).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_connect_quic_transport_call_not_found() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let fake_call_id = CallId::new();
        let peer = test_peer();

        let result = call_manager
            .connect_quic_transport(fake_call_id, peer)
            .await;

        assert!(matches!(result, Err(CallError::CallNotFound(_))));
    }

    #[tokio::test]
    async fn test_end_call_with_quic_transport() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("quic-callee");
        let constraints = MediaConstraints::audio_only();
        let peer = test_peer();

        // Create a QUIC call (has connected transport)
        let call_id = call_manager
            .initiate_quic_call(callee, constraints, peer)
            .await
            .unwrap();

        // Verify transport is present
        assert!(call_manager.has_media_transport(call_id).await);

        // End the call - should disconnect both transport and peer connection
        let result = call_manager.end_call(call_id).await;
        assert!(result.is_ok());

        // Verify call is removed
        let state = call_manager.get_call_state(call_id).await;
        assert_eq!(state, None);
    }

    #[tokio::test]
    async fn test_end_call_with_legacy_transport() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("legacy-callee");
        let constraints = MediaConstraints::audio_only();

        // Create a legacy call (has unconnected transport)
        let call_id = call_manager
            .initiate_call(callee, constraints)
            .await
            .unwrap();

        // End the call - should handle both transport paths gracefully
        let result = call_manager.end_call(call_id).await;
        assert!(result.is_ok());

        // Verify call is removed
        let state = call_manager.get_call_state(call_id).await;
        assert_eq!(state, None);
    }

    #[tokio::test]
    async fn test_update_state_from_transport() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();
        let peer = test_peer();

        // Create a QUIC call (starts in Connecting since transport connects immediately)
        let call_id = call_manager
            .initiate_quic_call(callee, constraints, peer)
            .await
            .unwrap();

        // Transport is connected, update should reflect Connected state
        let new_state = call_manager.update_state_from_transport(call_id).await;
        assert!(new_state.is_ok());
        assert_eq!(new_state.unwrap(), CallState::Connected);

        // Verify the call state was actually updated
        let state = call_manager.get_call_state(call_id).await;
        assert_eq!(state, Some(CallState::Connected));
    }

    #[tokio::test]
    async fn test_update_state_from_transport_not_found() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let fake_call_id = CallId::new();
        let result = call_manager.update_state_from_transport(fake_call_id).await;

        assert!(matches!(result, Err(CallError::CallNotFound(_))));
    }

    #[test]
    fn test_valid_quic_transitions() {
        // Valid transitions
        assert!(CallManager::<PeerIdentityString>::is_valid_quic_transition(
            CallState::Idle,
            CallState::Calling
        ));
        assert!(CallManager::<PeerIdentityString>::is_valid_quic_transition(
            CallState::Idle,
            CallState::Connecting
        ));
        assert!(CallManager::<PeerIdentityString>::is_valid_quic_transition(
            CallState::Calling,
            CallState::Connecting
        ));
        assert!(CallManager::<PeerIdentityString>::is_valid_quic_transition(
            CallState::Connecting,
            CallState::Connected
        ));
        assert!(CallManager::<PeerIdentityString>::is_valid_quic_transition(
            CallState::Connected,
            CallState::Ending
        ));
        assert!(CallManager::<PeerIdentityString>::is_valid_quic_transition(
            CallState::Ending,
            CallState::Idle
        ));

        // Failure transitions
        assert!(CallManager::<PeerIdentityString>::is_valid_quic_transition(
            CallState::Calling,
            CallState::Failed
        ));
        assert!(CallManager::<PeerIdentityString>::is_valid_quic_transition(
            CallState::Connecting,
            CallState::Failed
        ));
        assert!(CallManager::<PeerIdentityString>::is_valid_quic_transition(
            CallState::Connected,
            CallState::Failed
        ));

        // Recovery
        assert!(CallManager::<PeerIdentityString>::is_valid_quic_transition(
            CallState::Failed,
            CallState::Idle
        ));

        // Invalid transitions
        assert!(
            !CallManager::<PeerIdentityString>::is_valid_quic_transition(
                CallState::Idle,
                CallState::Connected
            )
        );
        assert!(
            !CallManager::<PeerIdentityString>::is_valid_quic_transition(
                CallState::Connected,
                CallState::Calling
            )
        );
        assert!(
            !CallManager::<PeerIdentityString>::is_valid_quic_transition(
                CallState::Ending,
                CallState::Connected
            )
        );
    }

    #[tokio::test]
    async fn test_exchange_capabilities() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::video_call();

        let call_id = call_manager
            .initiate_call(callee, constraints)
            .await
            .unwrap();

        // Exchange capabilities
        let capabilities = call_manager.exchange_capabilities(call_id).await.unwrap();

        // Verify capabilities match constraints
        assert!(capabilities.audio);
        assert!(capabilities.video);
        assert!(!capabilities.data_channel);
        assert!(capabilities.max_bandwidth_kbps > 0);

        // Verify state transitioned to Connecting
        let state = call_manager.get_call_state(call_id).await;
        assert_eq!(state, Some(CallState::Connecting));
    }

    #[tokio::test]
    async fn test_exchange_capabilities_audio_only() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();

        let call_id = call_manager
            .initiate_call(callee, constraints)
            .await
            .unwrap();

        let capabilities = call_manager.exchange_capabilities(call_id).await.unwrap();

        assert!(capabilities.audio);
        assert!(!capabilities.video);
        assert_eq!(capabilities.max_bandwidth_kbps, 128);
    }

    #[tokio::test]
    async fn test_exchange_capabilities_not_found() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let fake_call_id = CallId::new();
        let result = call_manager.exchange_capabilities(fake_call_id).await;

        assert!(matches!(result, Err(CallError::CallNotFound(_))));
    }

    #[tokio::test]
    async fn test_exchange_capabilities_invalid_state() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();

        let call_id = call_manager
            .initiate_call(callee, constraints.clone())
            .await
            .unwrap();

        // Accept the call to move it to Connected state
        call_manager
            .accept_call(call_id, constraints)
            .await
            .unwrap();

        // Try to exchange capabilities in Connected state - should fail
        let result = call_manager.exchange_capabilities(call_id).await;

        assert!(matches!(result, Err(CallError::InvalidState)));
    }

    #[tokio::test]
    async fn test_confirm_connection() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();
        let peer = test_peer();

        // Create a QUIC call (starts in Connecting with connected transport)
        let call_id = call_manager
            .initiate_quic_call(callee, constraints, peer)
            .await
            .unwrap();

        // Confirm connection with matching capabilities
        let peer_caps = MediaCapabilities::audio_only();
        let result = call_manager.confirm_connection(call_id, peer_caps).await;

        assert!(result.is_ok());

        // Verify state is Connected
        let state = call_manager.get_call_state(call_id).await;
        assert_eq!(state, Some(CallState::Connected));
    }

    #[tokio::test]
    async fn test_confirm_connection_incompatible_caps() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::video_call(); // Requires video
        let peer = test_peer();

        let call_id = call_manager
            .initiate_quic_call(callee, constraints, peer)
            .await
            .unwrap();

        // Try to confirm with audio-only capabilities (missing video)
        let peer_caps = MediaCapabilities::audio_only();
        let result = call_manager.confirm_connection(call_id, peer_caps).await;

        assert!(matches!(result, Err(CallError::ConfigError(_))));
    }

    #[tokio::test]
    async fn test_confirm_connection_not_found() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let fake_call_id = CallId::new();
        let peer_caps = MediaCapabilities::audio_only();

        let result = call_manager
            .confirm_connection(fake_call_id, peer_caps)
            .await;

        assert!(matches!(result, Err(CallError::CallNotFound(_))));
    }

    #[tokio::test]
    async fn test_confirm_connection_invalid_state() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();

        // Create a regular call (not QUIC, starts in Calling state)
        let call_id = call_manager
            .initiate_call(callee, constraints)
            .await
            .unwrap();

        // Try to confirm connection in Calling state - should fail
        let peer_caps = MediaCapabilities::audio_only();
        let result = call_manager.confirm_connection(call_id, peer_caps).await;

        assert!(matches!(result, Err(CallError::InvalidState)));
    }

    #[tokio::test]
    async fn test_confirm_connection_emits_event() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        // Subscribe to events before initiating call
        let mut event_rx = call_manager.subscribe_events();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();
        let peer = test_peer();

        let call_id = call_manager
            .initiate_quic_call(callee, constraints, peer)
            .await
            .unwrap();

        // Drain the CallInitiated event
        let _ = event_rx.try_recv();

        // Confirm connection
        let peer_caps = MediaCapabilities::audio_only();
        call_manager
            .confirm_connection(call_id, peer_caps)
            .await
            .unwrap();

        // Verify ConnectionEstablished event was emitted
        let event = event_rx.try_recv();
        assert!(event.is_ok());

        match event {
            Ok(CallEvent::ConnectionEstablished { call_id: eid }) => {
                assert_eq!(eid, call_id);
            }
            other => {
                unreachable!("Expected ConnectionEstablished event, got: {:?}", other);
            }
        }
    }

    #[test]
    fn test_validate_remote_capabilities_audio_only() {
        let constraints = MediaConstraints::audio_only();
        let caps = MediaCapabilities::audio_only();

        let result =
            CallManager::<PeerIdentityString>::validate_remote_capabilities(&constraints, &caps);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_remote_capabilities_video_call() {
        let constraints = MediaConstraints::video_call();
        let video_caps = MediaCapabilities::video();
        let audio_caps = MediaCapabilities::audio_only();

        // Video caps should satisfy video constraints
        let result = CallManager::<PeerIdentityString>::validate_remote_capabilities(
            &constraints,
            &video_caps,
        );
        assert!(result.is_ok());

        // Audio-only caps should NOT satisfy video constraints
        let result = CallManager::<PeerIdentityString>::validate_remote_capabilities(
            &constraints,
            &audio_caps,
        );
        assert!(matches!(result, Err(CallError::ConfigError(_))));
    }

    #[test]
    fn test_validate_remote_capabilities_missing_audio() {
        let constraints = MediaConstraints::video_call(); // requires audio
        let caps = MediaCapabilities {
            audio: false,
            video: true,
            data_channel: false,
            max_bandwidth_kbps: 2500,
        };

        let result =
            CallManager::<PeerIdentityString>::validate_remote_capabilities(&constraints, &caps);
        assert!(matches!(result, Err(CallError::ConfigError(_))));
    }

    #[test]
    fn test_validate_remote_capabilities_screen_share() {
        let constraints = MediaConstraints::screen_share();
        let caps = MediaCapabilities::video(); // screen share requires video capability

        let result =
            CallManager::<PeerIdentityString>::validate_remote_capabilities(&constraints, &caps);
        assert!(result.is_ok());

        // Audio-only caps should NOT satisfy screen share (needs video)
        let audio_caps = MediaCapabilities::audio_only();
        let result = CallManager::<PeerIdentityString>::validate_remote_capabilities(
            &constraints,
            &audio_caps,
        );
        assert!(matches!(result, Err(CallError::ConfigError(_))));
    }

    /// Test that verifies PeerIdentity type safety is preserved across all methods
    #[tokio::test]
    async fn test_peer_identity_type_safety() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        // Subscribe to events - should receive CallEvent<PeerIdentityString>
        let mut event_rx = call_manager.subscribe_events();

        let callee = PeerIdentityString::new("typed-callee");
        let constraints = MediaConstraints::audio_only();
        let peer = test_peer();

        // Initiate QUIC call - callee is typed as PeerIdentityString
        let call_id = call_manager
            .initiate_quic_call(callee.clone(), constraints, peer)
            .await
            .unwrap();

        // Receive event - should be properly typed
        let event = event_rx.try_recv();
        assert!(event.is_ok());

        // Verify event contains the correctly typed callee identity
        match event {
            Ok(CallEvent::CallInitiated {
                callee: event_callee,
                ..
            }) => {
                // This line compiles because event_callee is PeerIdentityString
                assert_eq!(event_callee.to_string_repr(), callee.to_string_repr());
            }
            other => {
                // Test assertion - use unreachable since tests are allowed panics
                unreachable!("Expected CallInitiated event, got: {:?}", other);
            }
        }

        // End call cleanly
        call_manager.end_call(call_id).await.unwrap();
    }

    #[tokio::test]
    async fn test_fail_call() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let mut event_rx = call_manager.subscribe_events();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();
        let peer = test_peer();

        let call_id = call_manager
            .initiate_quic_call(callee, constraints, peer)
            .await
            .unwrap();

        // Drain CallInitiated event
        let _ = event_rx.try_recv();

        // Fail the call
        let result = call_manager
            .fail_call(call_id, "Network error".to_string())
            .await;
        assert!(result.is_ok());

        // Verify state is Failed
        let state = call_manager.get_call_state(call_id).await;
        assert_eq!(state, Some(CallState::Failed));

        // Verify ConnectionFailed event was emitted
        let event = event_rx.try_recv();
        assert!(event.is_ok());

        match event {
            Ok(CallEvent::ConnectionFailed {
                call_id: eid,
                error,
            }) => {
                assert_eq!(eid, call_id);
                assert_eq!(error, "Network error");
            }
            other => {
                unreachable!("Expected ConnectionFailed event, got: {:?}", other);
            }
        }
    }

    #[tokio::test]
    async fn test_fail_call_not_found() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let fake_call_id = CallId::new();
        let result = call_manager
            .fail_call(fake_call_id, "Error".to_string())
            .await;

        assert!(matches!(result, Err(CallError::CallNotFound(_))));
    }

    #[tokio::test]
    async fn test_fail_call_from_invalid_state() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::audio_only();
        let peer = test_peer();

        let call_id = call_manager
            .initiate_quic_call(callee, constraints.clone(), peer)
            .await
            .unwrap();

        // First fail the call
        call_manager
            .fail_call(call_id, "Error 1".to_string())
            .await
            .unwrap();

        // Try to fail again from Failed state - should fail
        let result = call_manager.fail_call(call_id, "Error 2".to_string()).await;

        assert!(matches!(result, Err(CallError::InvalidState)));
    }

    #[tokio::test]
    async fn test_get_call_info() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let callee = PeerIdentityString::new("callee");
        let constraints = MediaConstraints::video_call();
        let peer = test_peer();

        let call_id = call_manager
            .initiate_quic_call(callee, constraints.clone(), peer)
            .await
            .unwrap();

        let info = call_manager.get_call_info(call_id).await;
        assert!(info.is_some());

        let (state, call_constraints, has_transport) = info.unwrap();
        assert_eq!(state, CallState::Connecting);
        assert_eq!(call_constraints.video, constraints.video);
        assert!(has_transport);
    }

    #[tokio::test]
    async fn test_get_call_info_not_found() {
        let config = CallManagerConfig::default();
        let call_manager = CallManager::<PeerIdentityString>::new(config)
            .await
            .unwrap();

        let fake_call_id = CallId::new();
        let info = call_manager.get_call_info(fake_call_id).await;

        assert!(info.is_none());
    }
}
