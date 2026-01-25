//! Comprehensive call state machine tests
//!
//! This module tests state transitions for both legacy WebRTC and QUIC-native flows.

use saorsa_webrtc_core::{
    call::CallError,
    identity::PeerIdentityString,
    link_transport::PeerConnection,
    types::{CallId, CallState, MediaCapabilities, MediaConstraints},
    CallManager, CallManagerConfig,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Helper to create a test PeerConnection for QUIC calls
fn test_peer() -> PeerConnection {
    PeerConnection {
        peer_id: "test-peer".to_string(),
        remote_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9000),
    }
}

#[tokio::test]
async fn state_transition_calling_to_connected() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();
    let callee = PeerIdentityString::new("callee");
    let constraints = MediaConstraints::audio_only();
    let id = mgr
        .initiate_call(callee, constraints.clone())
        .await
        .unwrap();

    mgr.accept_call(id, constraints).await.unwrap();
    assert_eq!(mgr.get_call_state(id).await, Some(CallState::Connected));
}

#[tokio::test]
async fn state_transition_calling_to_failed_on_reject() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();
    let id = mgr
        .initiate_call(
            PeerIdentityString::new("callee"),
            MediaConstraints::audio_only(),
        )
        .await
        .unwrap();
    mgr.reject_call(id).await.unwrap();
    assert_eq!(mgr.get_call_state(id).await, Some(CallState::Failed));
}

#[tokio::test]
async fn invalid_transitions_after_connected() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();
    let id = mgr
        .initiate_call(
            PeerIdentityString::new("callee"),
            MediaConstraints::audio_only(),
        )
        .await
        .unwrap();
    mgr.accept_call(id, MediaConstraints::audio_only())
        .await
        .unwrap();

    let rej = mgr.reject_call(id).await;
    assert!(matches!(rej, Err(CallError::InvalidState)));

    let acc2 = mgr.accept_call(id, MediaConstraints::audio_only()).await;
    assert!(matches!(acc2, Err(CallError::InvalidState)));
}

#[tokio::test]
async fn invalid_transitions_after_rejected() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();
    let id = mgr
        .initiate_call(
            PeerIdentityString::new("callee"),
            MediaConstraints::audio_only(),
        )
        .await
        .unwrap();
    mgr.reject_call(id).await.unwrap();

    let acc = mgr.accept_call(id, MediaConstraints::audio_only()).await;
    assert!(matches!(acc, Err(CallError::InvalidState)));
}

#[tokio::test]
async fn end_call_is_idempotent_by_removal() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();
    let id = mgr
        .initiate_call(
            PeerIdentityString::new("callee"),
            MediaConstraints::audio_only(),
        )
        .await
        .unwrap();

    mgr.end_call(id).await.unwrap();
    assert_eq!(mgr.get_call_state(id).await, None);

    let again = mgr.end_call(id).await;
    assert!(matches!(again, Err(CallError::CallNotFound(_))));
}

#[tokio::test]
async fn concurrent_call_limit_is_enforced() {
    let cfg = CallManagerConfig {
        max_concurrent_calls: 1,
    };
    let mgr = CallManager::<PeerIdentityString>::new(cfg).await.unwrap();

    let _id1 = mgr
        .initiate_call(
            PeerIdentityString::new("peer1"),
            MediaConstraints::audio_only(),
        )
        .await
        .unwrap();
    let id2 = mgr
        .initiate_call(
            PeerIdentityString::new("peer2"),
            MediaConstraints::audio_only(),
        )
        .await;
    assert!(
        matches!(id2, Err(CallError::ConfigError(ref msg)) if msg.contains("Maximum concurrent calls"))
    );
}

#[tokio::test]
#[allow(deprecated)]
async fn call_manager_errors_on_non_existent_calls() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();
    let fake = CallId::new();

    assert!(matches!(
        mgr.accept_call(fake, MediaConstraints::audio_only()).await,
        Err(CallError::CallNotFound(_))
    ));
    assert!(matches!(
        mgr.reject_call(fake).await,
        Err(CallError::CallNotFound(_))
    ));
    assert!(matches!(
        mgr.end_call(fake).await,
        Err(CallError::CallNotFound(_))
    ));
    assert!(matches!(
        mgr.create_offer(fake).await,
        Err(CallError::CallNotFound(_))
    ));
    assert!(matches!(
        mgr.handle_answer(fake, "x".to_string()).await,
        Err(CallError::CallNotFound(_))
    ));
    // Legacy ICE methods (deprecated)
    assert!(matches!(
        mgr.add_ice_candidate(fake, "x".to_string()).await,
        Err(CallError::CallNotFound(_))
    ));
    assert!(matches!(
        mgr.start_ice_gathering(fake).await,
        Err(CallError::CallNotFound(_))
    ));
}

// ============================================================================
// QUIC-Native State Machine Tests
// ============================================================================

#[tokio::test]
async fn quic_call_state_connecting_on_initiate() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();

    let callee = PeerIdentityString::new("quic-callee");
    let constraints = MediaConstraints::audio_only();
    let peer = test_peer();

    // QUIC calls start in Connecting state (not Calling)
    let call_id = mgr
        .initiate_quic_call(callee, constraints, peer)
        .await
        .unwrap();

    assert_eq!(
        mgr.get_call_state(call_id).await,
        Some(CallState::Connecting)
    );
}

#[tokio::test]
async fn quic_call_state_connecting_to_connected() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();

    let callee = PeerIdentityString::new("quic-callee");
    let constraints = MediaConstraints::audio_only();
    let peer = test_peer();

    let call_id = mgr
        .initiate_quic_call(callee, constraints, peer)
        .await
        .unwrap();

    // Confirm with matching capabilities
    let peer_caps = MediaCapabilities::audio_only();
    mgr.confirm_connection(call_id, peer_caps).await.unwrap();

    assert_eq!(
        mgr.get_call_state(call_id).await,
        Some(CallState::Connected)
    );
}

#[tokio::test]
async fn quic_call_confirm_requires_matching_capabilities() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();

    let callee = PeerIdentityString::new("quic-callee");
    let constraints = MediaConstraints::video_call(); // Requires video
    let peer = test_peer();

    let call_id = mgr
        .initiate_quic_call(callee, constraints, peer)
        .await
        .unwrap();

    // Try to confirm with audio-only (missing video)
    let peer_caps = MediaCapabilities::audio_only();
    let result = mgr.confirm_connection(call_id, peer_caps).await;

    // Should fail due to capability mismatch
    assert!(result.is_err());

    // State should remain Connecting
    assert_eq!(
        mgr.get_call_state(call_id).await,
        Some(CallState::Connecting)
    );
}

#[tokio::test]
async fn quic_call_fail_sets_failed_state() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();

    let callee = PeerIdentityString::new("quic-callee");
    let constraints = MediaConstraints::audio_only();
    let peer = test_peer();

    let call_id = mgr
        .initiate_quic_call(callee, constraints, peer)
        .await
        .unwrap();

    // Fail the call
    mgr.fail_call(call_id, "Network error".to_string())
        .await
        .unwrap();

    assert_eq!(mgr.get_call_state(call_id).await, Some(CallState::Failed));
}

#[tokio::test]
async fn capability_exchange_transitions_calling_to_connecting() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();

    let callee = PeerIdentityString::new("callee");
    let constraints = MediaConstraints::video_call();

    // Regular call starts in Calling
    let call_id = mgr.initiate_call(callee, constraints).await.unwrap();

    assert_eq!(mgr.get_call_state(call_id).await, Some(CallState::Calling));

    // Exchange capabilities
    let caps = mgr.exchange_capabilities(call_id).await.unwrap();

    // Verify capabilities derived from constraints
    assert!(caps.audio);
    assert!(caps.video);

    // State should now be Connecting
    assert_eq!(
        mgr.get_call_state(call_id).await,
        Some(CallState::Connecting)
    );
}

#[tokio::test]
async fn quic_call_invalid_confirm_on_connected() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();

    let callee = PeerIdentityString::new("quic-callee");
    let constraints = MediaConstraints::audio_only();
    let peer = test_peer();

    let call_id = mgr
        .initiate_quic_call(callee, constraints, peer)
        .await
        .unwrap();

    // First confirm succeeds
    let peer_caps = MediaCapabilities::audio_only();
    mgr.confirm_connection(call_id, peer_caps.clone())
        .await
        .unwrap();
    assert_eq!(
        mgr.get_call_state(call_id).await,
        Some(CallState::Connected)
    );

    // Second confirm should fail (invalid state transition)
    let result = mgr.confirm_connection(call_id, peer_caps).await;
    assert!(matches!(result, Err(CallError::InvalidState)));
}

#[tokio::test]
async fn quic_call_end_from_any_state() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();

    // Test ending from Connecting state
    let peer = test_peer();
    let call_id = mgr
        .initiate_quic_call(
            PeerIdentityString::new("callee1"),
            MediaConstraints::audio_only(),
            peer.clone(),
        )
        .await
        .unwrap();

    assert_eq!(
        mgr.get_call_state(call_id).await,
        Some(CallState::Connecting)
    );
    mgr.end_call(call_id).await.unwrap();
    assert_eq!(mgr.get_call_state(call_id).await, None);

    // Test ending from Connected state
    let call_id = mgr
        .initiate_quic_call(
            PeerIdentityString::new("callee2"),
            MediaConstraints::audio_only(),
            peer.clone(),
        )
        .await
        .unwrap();

    mgr.confirm_connection(call_id, MediaCapabilities::audio_only())
        .await
        .unwrap();
    assert_eq!(
        mgr.get_call_state(call_id).await,
        Some(CallState::Connected)
    );
    mgr.end_call(call_id).await.unwrap();
    assert_eq!(mgr.get_call_state(call_id).await, None);

    // Test ending from Failed state
    let call_id = mgr
        .initiate_quic_call(
            PeerIdentityString::new("callee3"),
            MediaConstraints::audio_only(),
            peer,
        )
        .await
        .unwrap();

    mgr.fail_call(call_id, "error".to_string()).await.unwrap();
    assert_eq!(mgr.get_call_state(call_id).await, Some(CallState::Failed));
    mgr.end_call(call_id).await.unwrap();
    assert_eq!(mgr.get_call_state(call_id).await, None);
}

#[tokio::test]
async fn quic_call_operations_on_non_existent_calls() {
    let mgr = CallManager::<PeerIdentityString>::new(CallManagerConfig::default())
        .await
        .unwrap();
    let fake = CallId::new();

    // QUIC-native operations should fail on non-existent calls
    assert!(matches!(
        mgr.confirm_connection(fake, MediaCapabilities::audio_only())
            .await,
        Err(CallError::CallNotFound(_))
    ));
    assert!(matches!(
        mgr.fail_call(fake, "error".to_string()).await,
        Err(CallError::CallNotFound(_))
    ));
    assert!(matches!(
        mgr.exchange_capabilities(fake).await,
        Err(CallError::CallNotFound(_))
    ));
}
