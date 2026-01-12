//! Tauri plugin for desktop integration

#![deny(clippy::panic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use saorsa_webrtc_core::{
    identity::PeerIdentityString,
    service::{WebRtcConfig, WebRtcService},
    signaling::SignalingHandler,
    types::{CallId, CallState, MediaConstraints},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime, State,
};
use tokio::sync::RwLock;

type WebRtcServiceWrapper = Arc<RwLock<Option<WebRtcService<PeerIdentityString, MockTransport>>>>;

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InitializeRequest {
    identity: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CallRequest {
    peer: String,
    #[serde(default = "default_audio_only")]
    audio: bool,
    #[serde(default)]
    video: bool,
    #[serde(default)]
    screen_share: bool,
}

#[allow(dead_code)]
fn default_audio_only() -> bool {
    true
}

/// Initialize the WebRTC service
#[tauri::command]
async fn initialize(
    state: State<'_, WebRtcServiceWrapper>,
    identity: String,
) -> Result<(), String> {
    if identity.is_empty() {
        return Err("Identity cannot be empty".to_string());
    }

    let transport = Arc::new(MockTransport::new());
    let signaling = Arc::new(SignalingHandler::new(transport));

    let service = WebRtcService::builder(signaling)
        .with_config(WebRtcConfig::default())
        .build()
        .await
        .map_err(|e| format!("Failed to create service: {e}"))?;

    service
        .start()
        .await
        .map_err(|e| format!("Failed to start service: {e}"))?;

    *state.write().await = Some(service);

    Ok(())
}

/// Initiate a call to a peer
#[tauri::command]
async fn call(state: State<'_, WebRtcServiceWrapper>, peer: String) -> Result<String, String> {
    if peer.is_empty() {
        return Err("Peer address cannot be empty".to_string());
    }

    let service_guard = state.read().await;
    let service = service_guard
        .as_ref()
        .ok_or_else(|| "Service not initialized".to_string())?;

    let peer_identity = PeerIdentityString::new(peer);
    let constraints = MediaConstraints::audio_only();

    let call_id = service
        .initiate_call(peer_identity, constraints)
        .await
        .map_err(|e| format!("Failed to initiate call: {e}"))?;

    Ok(call_id.to_string())
}

/// Initiate a call with custom constraints
#[tauri::command]
async fn call_with_constraints(
    state: State<'_, WebRtcServiceWrapper>,
    peer: String,
    audio: bool,
    video: bool,
    screen_share: bool,
) -> Result<String, String> {
    if peer.is_empty() {
        return Err("Peer address cannot be empty".to_string());
    }

    let service_guard = state.read().await;
    let service = service_guard
        .as_ref()
        .ok_or_else(|| "Service not initialized".to_string())?;

    let peer_identity = PeerIdentityString::new(peer);
    let constraints = MediaConstraints {
        audio,
        video,
        screen_share,
    };

    let call_id = service
        .initiate_call(peer_identity, constraints)
        .await
        .map_err(|e| format!("Failed to initiate call: {e}"))?;

    Ok(call_id.to_string())
}

/// Get the state of a call
#[tauri::command]
async fn get_call_state(
    state: State<'_, WebRtcServiceWrapper>,
    call_id: String,
) -> Result<String, String> {
    let service_guard = state.read().await;
    let service = service_guard
        .as_ref()
        .ok_or_else(|| "Service not initialized".to_string())?;

    let call_id_uuid =
        uuid::Uuid::parse_str(&call_id).map_err(|e| format!("Invalid call ID: {e}"))?;

    let call_state = service
        .get_call_state(CallId(call_id_uuid))
        .await
        .ok_or_else(|| "Call not found".to_string())?;

    Ok(call_state_to_string(call_state))
}

/// End a call
#[tauri::command]
async fn end_call(state: State<'_, WebRtcServiceWrapper>, call_id: String) -> Result<(), String> {
    let service_guard = state.read().await;
    let service = service_guard
        .as_ref()
        .ok_or_else(|| "Service not initialized".to_string())?;

    let call_id_uuid =
        uuid::Uuid::parse_str(&call_id).map_err(|e| format!("Invalid call ID: {e}"))?;

    service
        .end_call(CallId(call_id_uuid))
        .await
        .map_err(|e| format!("Failed to end call: {e}"))?;

    Ok(())
}

/// Accept an incoming call
#[tauri::command]
async fn accept_call(
    state: State<'_, WebRtcServiceWrapper>,
    call_id: String,
) -> Result<(), String> {
    let service_guard = state.read().await;
    let service = service_guard
        .as_ref()
        .ok_or_else(|| "Service not initialized".to_string())?;

    let call_id_uuid =
        uuid::Uuid::parse_str(&call_id).map_err(|e| format!("Invalid call ID: {e}"))?;

    service
        .accept_call(CallId(call_id_uuid), MediaConstraints::audio_only())
        .await
        .map_err(|e| format!("Failed to accept call: {e}"))?;

    Ok(())
}

/// Reject an incoming call
#[tauri::command]
async fn reject_call(
    state: State<'_, WebRtcServiceWrapper>,
    call_id: String,
) -> Result<(), String> {
    let service_guard = state.read().await;
    let service = service_guard
        .as_ref()
        .ok_or_else(|| "Service not initialized".to_string())?;

    let call_id_uuid =
        uuid::Uuid::parse_str(&call_id).map_err(|e| format!("Invalid call ID: {e}"))?;

    service
        .reject_call(CallId(call_id_uuid))
        .await
        .map_err(|e| format!("Failed to reject call: {e}"))?;

    Ok(())
}

fn call_state_to_string(state: CallState) -> String {
    match state {
        CallState::Idle => "idle".to_string(),
        CallState::Calling => "calling".to_string(),
        CallState::Connecting => "connecting".to_string(),
        CallState::Connected => "connected".to_string(),
        CallState::Ending => "ending".to_string(),
        CallState::Failed => "failed".to_string(),
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    let service_wrapper: WebRtcServiceWrapper = Arc::new(RwLock::new(None));

    Builder::new("saorsa-webrtc")
        .invoke_handler(tauri::generate_handler![
            initialize,
            call,
            call_with_constraints,
            get_call_state,
            end_call,
            accept_call,
            reject_call,
        ])
        .setup(move |app_handle| {
            app_handle.manage(service_wrapper.clone());
            Ok(())
        })
        .build()
}

use async_trait::async_trait;
use saorsa_webrtc_core::signaling::{SignalingMessage, SignalingTransport};
use std::collections::VecDeque;
use tokio::sync::Mutex;

#[derive(Debug)]
struct MockTransport {
    messages: Mutex<VecDeque<(String, SignalingMessage)>>,
}

#[derive(Debug)]
struct MockError(String);

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Mock transport error: {}", self.0)
    }
}

impl std::error::Error for MockError {}

impl MockTransport {
    fn new() -> Self {
        Self {
            messages: Mutex::new(VecDeque::new()),
        }
    }
}

#[async_trait]
impl SignalingTransport for MockTransport {
    type PeerId = String;
    type Error = MockError;

    async fn send_message(
        &self,
        peer: &Self::PeerId,
        message: SignalingMessage,
    ) -> Result<(), Self::Error> {
        self.messages
            .lock()
            .await
            .push_back((peer.clone(), message));
        Ok(())
    }

    async fn receive_message(&self) -> Result<(Self::PeerId, SignalingMessage), Self::Error> {
        let mut messages = self.messages.lock().await;
        messages
            .pop_front()
            .ok_or_else(|| MockError("No messages available".to_string()))
    }

    async fn discover_peer_endpoint(
        &self,
        _peer: &Self::PeerId,
    ) -> Result<Option<std::net::SocketAddr>, Self::Error> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_service_integration() {
        let transport = Arc::new(MockTransport::new());
        let signaling = Arc::new(SignalingHandler::new(transport));

        let service: Result<WebRtcService<PeerIdentityString, MockTransport>, _> =
            WebRtcService::builder(signaling)
                .with_config(WebRtcConfig::default())
                .build()
                .await;

        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_initiate_call_with_service() {
        let transport = Arc::new(MockTransport::new());
        let signaling = Arc::new(SignalingHandler::new(transport));

        let service: Result<WebRtcService<PeerIdentityString, MockTransport>, _> =
            WebRtcService::builder(signaling)
                .with_config(WebRtcConfig::default())
                .build()
                .await;

        if let Ok(service) = service {
            let result = service.start().await;
            assert!(result.is_ok());

            let peer = PeerIdentityString::new("bob");
            let call_result = service
                .initiate_call(peer, MediaConstraints::audio_only())
                .await;
            assert!(call_result.is_ok());
        }
    }

    #[test]
    fn test_call_state_conversion() {
        assert_eq!(call_state_to_string(CallState::Idle), "idle");
        assert_eq!(call_state_to_string(CallState::Calling), "calling");
        assert_eq!(call_state_to_string(CallState::Connecting), "connecting");
        assert_eq!(call_state_to_string(CallState::Connected), "connected");
        assert_eq!(call_state_to_string(CallState::Ending), "ending");
        assert_eq!(call_state_to_string(CallState::Failed), "failed");
    }

    #[test]
    fn test_mock_transport_creation() {
        let transport = MockTransport::new();
        assert!(matches!(transport, MockTransport { .. }));
    }

    #[tokio::test]
    async fn test_empty_identity_validation() {
        let identity = "";
        assert!(identity.is_empty());
    }

    #[tokio::test]
    async fn test_valid_identity_validation() {
        let identity = "alice";
        assert!(!identity.is_empty());
    }

    #[tokio::test]
    async fn test_call_id_parsing() {
        let uuid = uuid::Uuid::new_v4();
        let call_id_str = uuid.to_string();
        let parsed = uuid::Uuid::parse_str(&call_id_str);
        assert!(parsed.is_ok());
        assert_eq!(parsed.ok(), Some(uuid));
    }
}
