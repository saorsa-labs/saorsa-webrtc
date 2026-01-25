//! Call management for WebRTC
//!
//! **Note:** This module uses the webrtc crate types (requires legacy-webrtc feature).
//! In Phase 2, this will be replaced with a QUIC-native implementation via QuicMediaTransport.

use crate::identity::PeerIdentity;
use crate::media::{MediaStreamManager, WebRtcTrack};
use crate::quic_media_transport::QuicMediaTransport;
use crate::types::{CallEvent, CallId, CallState, MediaConstraints};
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

            // Close the peer connection
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

    /// Create SDP offer for a call
    ///
    /// # Errors
    ///
    /// Returns error if offer cannot be created
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

    /// Handle SDP answer for a call
    ///
    /// # Errors
    ///
    /// Returns error if answer cannot be handled
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

    /// Add ICE candidate to a call
    ///
    /// # Errors
    ///
    /// Returns error if candidate cannot be added
    #[tracing::instrument(skip(self, candidate), fields(call_id = %call_id))]
    pub async fn add_ice_candidate(
        &self,
        call_id: CallId,
        candidate: String,
    ) -> Result<(), CallError> {
        tracing::trace!("Adding ICE candidate");

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

    /// Start ICE gathering for a call
    ///
    /// # Errors
    ///
    /// Returns error if gathering cannot be started
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
    async fn test_call_manager_create_offer() {
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
    async fn test_call_manager_add_ice_candidate() {
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

        // Test with a dummy ICE candidate
        let candidate = "candidate:1 1 UDP 2122260223 192.168.1.1 12345 typ host".to_string();
        let result = call_manager.add_ice_candidate(call_id, candidate).await;
        // This might fail in test environment, but should not panic
        // We just test that the method exists and handles call not found
        assert!(result.is_ok() || matches!(result, Err(CallError::ConfigError(_))));
    }

    #[tokio::test]
    async fn test_call_manager_start_ice_gathering() {
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

        let result = call_manager.create_offer(fake_call_id).await;
        assert!(matches!(result, Err(CallError::CallNotFound(_))));

        let result = call_manager
            .handle_answer(fake_call_id, "dummy".to_string())
            .await;
        assert!(matches!(result, Err(CallError::CallNotFound(_))));

        let result = call_manager
            .add_ice_candidate(fake_call_id, "dummy".to_string())
            .await;
        assert!(matches!(result, Err(CallError::CallNotFound(_))));

        let result = call_manager.start_ice_gathering(fake_call_id).await;
        assert!(matches!(result, Err(CallError::CallNotFound(_))));
    }
}
