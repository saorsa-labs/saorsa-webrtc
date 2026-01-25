//! Comprehensive call state machine tests

use saorsa_webrtc_core::{
    call::CallError,
    identity::PeerIdentityString,
    types::{CallId, CallState, MediaConstraints},
    CallManager, CallManagerConfig,
};

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
