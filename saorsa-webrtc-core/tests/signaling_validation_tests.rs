//! Signaling validation and edge case tests

use saorsa_webrtc_core::{
    call::CallError, identity::PeerIdentityString, signaling::SignalingMessage,
    types::MediaConstraints, CallManager, CallManagerConfig,
};

#[tokio::test]
#[allow(deprecated)]
async fn handle_answer_rejects_empty_sdp() {
    // Tests legacy SDP answer handling (deprecated for QUIC-native calls)
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

    let res = mgr.handle_answer(id, String::new()).await;
    assert!(matches!(res, Err(CallError::ConfigError(ref msg)) if msg.contains("cannot be empty")));
}

#[tokio::test]
#[allow(deprecated)]
async fn handle_answer_rejects_malformed_sdp() {
    // Tests legacy SDP answer handling (deprecated for QUIC-native calls)
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

    let res = mgr.handle_answer(id, "not-an-sdp".to_string()).await;
    assert!(matches!(res, Err(CallError::ConfigError(_))));
}

#[tokio::test]
#[allow(deprecated)]
async fn add_ice_candidate_handles_empty() {
    // Tests legacy ICE method with empty candidate
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

    let res_empty = mgr.add_ice_candidate(id, String::new()).await;
    assert!(res_empty.is_ok() || matches!(res_empty, Err(CallError::ConfigError(_))));
}

#[tokio::test]
#[allow(deprecated)]
async fn add_ice_candidate_handles_garbage() {
    // Tests legacy ICE method with garbage data
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

    let res_bad = mgr
        .add_ice_candidate(id, "garbage-candidate-data".to_string())
        .await;
    assert!(res_bad.is_ok() || matches!(res_bad, Err(CallError::ConfigError(_))));
}

#[tokio::test]
async fn signaling_message_large_payload_roundtrip() {
    let large_sdp = "v=0\n".to_string() + &"a=mid:0\n".repeat(64 * 1024);
    let msg = SignalingMessage::Offer {
        session_id: "sess".to_string(),
        sdp: large_sdp.clone(),
        quic_endpoint: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: SignalingMessage = serde_json::from_str(&json).unwrap();
    if let SignalingMessage::Offer { sdp, .. } = back {
        assert_eq!(sdp.len(), large_sdp.len());
    } else {
        panic!("wrong variant");
    }
}

#[tokio::test]
async fn signaling_message_all_variants_serialize() {
    let variants = vec![
        SignalingMessage::Offer {
            session_id: "s1".to_string(),
            sdp: "v=0".to_string(),
            quic_endpoint: None,
        },
        SignalingMessage::Answer {
            session_id: "s2".to_string(),
            sdp: "v=0".to_string(),
            quic_endpoint: None,
        },
        SignalingMessage::IceCandidate {
            session_id: "s3".to_string(),
            candidate: "candidate:123".to_string(),
            sdp_mid: Some("0".to_string()),
            sdp_mline_index: Some(0),
        },
    ];

    for msg in variants {
        let json = serde_json::to_string(&msg).unwrap();
        let _back: SignalingMessage = serde_json::from_str(&json).unwrap();
    }
}
