//! Integration tests for end-to-end WebRTC functionality

use saorsa_webrtc_core::signaling::SignalingMessage;
use saorsa_webrtc_core::{
    CallId, CallManager, CallManagerConfig, CallState, MediaConstraints, MediaStreamManager,
    MediaType, PeerIdentity, PeerIdentityString, SignalingHandler, SignalingTransport,
};
use std::sync::Arc;

// Mock transport for integration testing
struct MockSignalingTransport {
    peer_messages: std::sync::Mutex<std::collections::HashMap<String, Vec<SignalingMessage>>>,
}

impl MockSignalingTransport {
    fn new() -> Self {
        Self {
            peer_messages: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    fn send_to_peer(&self, peer: &str, message: SignalingMessage) {
        self.peer_messages
            .lock()
            .unwrap()
            .entry(peer.to_string())
            .or_default()
            .push(message);
    }

    fn receive_from_peer(&self, peer: &str) -> Option<SignalingMessage> {
        self.peer_messages
            .lock()
            .unwrap()
            .get_mut(peer)
            .and_then(|messages| messages.pop())
    }
}

#[async_trait::async_trait]
impl SignalingTransport for MockSignalingTransport {
    type PeerId = String;
    type Error = std::io::Error;

    async fn send_message(
        &self,
        peer: &String,
        message: SignalingMessage,
    ) -> Result<(), Self::Error> {
        self.send_to_peer(peer, message);
        Ok(())
    }

    async fn receive_message(&self) -> Result<(String, SignalingMessage), Self::Error> {
        // For integration tests, we'll simulate message passing differently
        // This is a simplified version - in real integration we'd have message queues
        Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "No messages",
        ))
    }

    async fn discover_peer_endpoint(
        &self,
        _peer: &String,
    ) -> Result<Option<std::net::SocketAddr>, Self::Error> {
        Ok(Some("127.0.0.1:8080".parse().unwrap()))
    }
}

#[tokio::test]
async fn test_full_call_flow() {
    // Create mock transport
    let transport = Arc::new(MockSignalingTransport::new());
    let _signaling = Arc::new(SignalingHandler::new(transport.clone()));

    // Create call manager
    let call_config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(call_config)
        .await
        .unwrap();

    // Create media manager
    let media_manager = MediaStreamManager::new();
    media_manager.initialize().await.unwrap();

    // Test call initiation
    let callee = PeerIdentityString::new("callee-peer");
    let constraints = MediaConstraints::video_call();

    let call_id = call_manager
        .initiate_call(callee.clone(), constraints.clone())
        .await
        .unwrap();

    // Verify call was created
    let call_state = call_manager.get_call_state(call_id).await;
    assert_eq!(call_state, Some(CallState::Calling));

    // Test call acceptance
    call_manager
        .accept_call(call_id, constraints)
        .await
        .unwrap();

    // Verify call state changed
    let call_state = call_manager.get_call_state(call_id).await;
    assert_eq!(call_state, Some(CallState::Connected));

    // Test call ending
    call_manager.end_call(call_id).await.unwrap();

    // Verify call was removed
    let call_state = call_manager.get_call_state(call_id).await;
    assert_eq!(call_state, None);
}

#[tokio::test]
async fn test_media_track_creation_integration() {
    let mut media_manager = MediaStreamManager::new();
    media_manager.initialize().await.unwrap();

    // Create audio track
    let audio_track = media_manager.create_audio_track().await.unwrap();
    assert_eq!(audio_track.track_type, MediaType::Audio);
    assert!(audio_track.id.starts_with("audio-"));

    // Create video track
    let video_track = media_manager.create_video_track().await.unwrap();
    assert_eq!(video_track.track_type, MediaType::Video);
    assert!(video_track.id.starts_with("video-"));

    // Verify tracks are stored
    let all_tracks = media_manager.get_webrtc_tracks();
    assert_eq!(all_tracks.len(), 2);
}

#[tokio::test]
async fn test_quic_stream_management_integration() {
    let mut stream_manager = saorsa_webrtc_core::quic_streams::QuicMediaStreamManager::new(
        saorsa_webrtc_core::quic_streams::QoSParams::audio(),
    );

    // Create different types of streams
    let audio_stream_id = stream_manager
        .create_stream(saorsa_webrtc_core::quic_streams::MediaStreamType::Audio)
        .unwrap();

    let video_stream_id = stream_manager
        .create_stream(saorsa_webrtc_core::quic_streams::MediaStreamType::Video)
        .unwrap();

    let screen_stream_id = stream_manager
        .create_stream(saorsa_webrtc_core::quic_streams::MediaStreamType::ScreenShare)
        .unwrap();

    // Verify streams were created
    assert_eq!(audio_stream_id, 0);
    assert_eq!(video_stream_id, 1);
    assert_eq!(screen_stream_id, 2);

    // Verify stream properties
    let audio_stream = stream_manager.get_stream(audio_stream_id).unwrap();
    assert_eq!(
        audio_stream.stream_type,
        saorsa_webrtc_core::quic_streams::MediaStreamType::Audio
    );

    let video_stream = stream_manager.get_stream(video_stream_id).unwrap();
    assert_eq!(
        video_stream.stream_type,
        saorsa_webrtc_core::quic_streams::MediaStreamType::Video
    );

    // Note: send_data requires a configured transport, which is tested
    // separately in the QUIC loopback integration tests.

    // Test stream closing
    stream_manager.close_stream(audio_stream_id).unwrap();
    assert!(stream_manager.get_stream(audio_stream_id).is_none());

    // Verify remaining streams
    let active_streams = stream_manager.active_streams();
    assert_eq!(active_streams.len(), 2);
}

#[tokio::test]
async fn test_signaling_transport_integration() {
    let transport = MockSignalingTransport::new();

    // Test sending messages
    let offer = SignalingMessage::Offer {
        session_id: "test-session".to_string(),
        sdp: "test-sdp".to_string(),
        quic_endpoint: None,
    };

    transport.send_to_peer("peer1", offer.clone());

    // Test receiving messages
    let received = transport.receive_from_peer("peer1");
    assert_eq!(received, Some(offer));
}

#[tokio::test]
async fn test_call_constraints_integration() {
    let call_config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(call_config)
        .await
        .unwrap();

    // Test audio-only call
    let callee = PeerIdentityString::new("audio-peer");
    let audio_constraints = MediaConstraints::audio_only();

    let audio_call_id = call_manager
        .initiate_call(callee.clone(), audio_constraints)
        .await
        .unwrap();

    // Test video call
    let video_callee = PeerIdentityString::new("video-peer");
    let video_constraints = MediaConstraints::video_call();

    let video_call_id = call_manager
        .initiate_call(video_callee, video_constraints)
        .await
        .unwrap();

    // Test screen share call
    let screen_callee = PeerIdentityString::new("screen-peer");
    let screen_constraints = MediaConstraints::screen_share();

    let screen_call_id = call_manager
        .initiate_call(screen_callee, screen_constraints)
        .await
        .unwrap();

    // Verify all calls exist
    assert_eq!(
        call_manager.get_call_state(audio_call_id).await,
        Some(CallState::Calling)
    );
    assert_eq!(
        call_manager.get_call_state(video_call_id).await,
        Some(CallState::Calling)
    );
    assert_eq!(
        call_manager.get_call_state(screen_call_id).await,
        Some(CallState::Calling)
    );

    // Clean up
    call_manager.end_call(audio_call_id).await.unwrap();
    call_manager.end_call(video_call_id).await.unwrap();
    call_manager.end_call(screen_call_id).await.unwrap();
}

#[tokio::test]
#[allow(deprecated)]
async fn test_error_handling_integration() {
    let call_config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(call_config)
        .await
        .unwrap();

    // Test operations on non-existent calls
    let fake_call_id = CallId::new();

    assert!(call_manager
        .accept_call(fake_call_id, MediaConstraints::audio_only())
        .await
        .is_err());
    assert!(call_manager.reject_call(fake_call_id).await.is_err());
    assert!(call_manager.end_call(fake_call_id).await.is_err());
    assert!(call_manager.create_offer(fake_call_id).await.is_err());
    // Legacy ICE methods (deprecated)
    assert!(call_manager
        .add_ice_candidate(fake_call_id, "dummy".to_string())
        .await
        .is_err());
    assert!(call_manager
        .start_ice_gathering(fake_call_id)
        .await
        .is_err());
}

#[tokio::test]
async fn test_media_constraints_validation() {
    // Test constraint creation
    let audio_only = MediaConstraints::audio_only();
    assert!(audio_only.has_audio());
    assert!(!audio_only.has_video());
    assert!(!audio_only.has_screen_share());

    let video_call = MediaConstraints::video_call();
    assert!(video_call.has_audio());
    assert!(video_call.has_video());
    assert!(!video_call.has_screen_share());

    let screen_share = MediaConstraints::screen_share();
    assert!(screen_share.has_audio());
    assert!(!screen_share.has_video());
    assert!(screen_share.has_screen_share());

    // Test media types conversion
    let audio_types = audio_only.to_media_types();
    assert_eq!(audio_types.len(), 1);
    assert_eq!(audio_types[0], MediaType::Audio);

    let video_types = video_call.to_media_types();
    assert_eq!(video_types.len(), 2);
    assert!(video_types.contains(&MediaType::Audio));
    assert!(video_types.contains(&MediaType::Video));

    let screen_types = screen_share.to_media_types();
    assert_eq!(screen_types.len(), 2);
    assert!(screen_types.contains(&MediaType::Audio));
    assert!(screen_types.contains(&MediaType::ScreenShare));
}

// ============================================================================
// QUIC-NATIVE CALL FLOW INTEGRATION TESTS
// ============================================================================

/// Helper to create a test PeerConnection
fn test_peer() -> saorsa_webrtc_core::link_transport::PeerConnection {
    saorsa_webrtc_core::link_transport::PeerConnection {
        peer_id: "test-peer".to_string(),
        remote_addr: "127.0.0.1:9000".parse().unwrap(),
    }
}

use saorsa_webrtc_core::types::MediaCapabilities;
use saorsa_webrtc_core::CallEvent;

/// Test the full QUIC-native call flow: initiate → exchange → confirm → end
#[tokio::test]
async fn test_quic_native_call_flow_full() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    // Subscribe to events
    let mut event_rx = call_manager.subscribe_events();

    let callee = PeerIdentityString::new("quic-callee");
    let constraints = MediaConstraints::audio_only();
    let peer = test_peer();

    // Step 1: Initiate QUIC call
    let call_id = call_manager
        .initiate_quic_call(callee.clone(), constraints.clone(), peer)
        .await
        .unwrap();

    // Verify CallInitiated event
    let event = event_rx.try_recv().unwrap();
    match event {
        CallEvent::CallInitiated {
            call_id: eid,
            callee: ecallee,
            ..
        } => {
            assert_eq!(eid, call_id);
            assert_eq!(ecallee.to_string_repr(), callee.to_string_repr());
        }
        other => panic!("Expected CallInitiated, got: {:?}", other),
    }

    // Verify state is Connecting (QUIC calls start in Connecting)
    let state = call_manager.get_call_state(call_id).await;
    assert_eq!(state, Some(CallState::Connecting));

    // Step 2: Confirm connection with matching capabilities
    let peer_caps = MediaCapabilities::audio_only();
    call_manager
        .confirm_connection(call_id, peer_caps)
        .await
        .unwrap();

    // Verify ConnectionEstablished event
    let event = event_rx.try_recv().unwrap();
    match event {
        CallEvent::ConnectionEstablished { call_id: eid } => {
            assert_eq!(eid, call_id);
        }
        other => panic!("Expected ConnectionEstablished, got: {:?}", other),
    }

    // Verify state is Connected
    let state = call_manager.get_call_state(call_id).await;
    assert_eq!(state, Some(CallState::Connected));

    // Step 3: End call
    call_manager.end_call(call_id).await.unwrap();

    // Verify CallEnded event
    let event = event_rx.try_recv().unwrap();
    match event {
        CallEvent::CallEnded { call_id: eid } => {
            assert_eq!(eid, call_id);
        }
        other => panic!("Expected CallEnded, got: {:?}", other),
    }

    // Verify call removed
    let state = call_manager.get_call_state(call_id).await;
    assert_eq!(state, None);
}

/// Test capability exchange flow (for non-QUIC-initiated calls)
#[tokio::test]
async fn test_quic_native_capability_exchange_flow() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let callee = PeerIdentityString::new("callee");
    let constraints = MediaConstraints::video_call();

    // Initiate regular call (starts in Calling state)
    let call_id = call_manager
        .initiate_call(callee, constraints.clone())
        .await
        .unwrap();

    // Verify initial state is Calling
    let state = call_manager.get_call_state(call_id).await;
    assert_eq!(state, Some(CallState::Calling));

    // Exchange capabilities
    let caps = call_manager.exchange_capabilities(call_id).await.unwrap();

    // Verify capabilities match constraints
    assert!(caps.audio);
    assert!(caps.video);
    assert_eq!(caps.max_bandwidth_kbps, 2500); // Video calls get higher bandwidth

    // Verify state transitioned to Connecting
    let state = call_manager.get_call_state(call_id).await;
    assert_eq!(state, Some(CallState::Connecting));
}

/// Test call failure handling
#[tokio::test]
async fn test_quic_native_call_failure() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let mut event_rx = call_manager.subscribe_events();

    let callee = PeerIdentityString::new("callee");
    let constraints = MediaConstraints::audio_only();
    let peer = test_peer();

    // Initiate QUIC call
    let call_id = call_manager
        .initiate_quic_call(callee, constraints, peer)
        .await
        .unwrap();

    // Drain CallInitiated event
    let _ = event_rx.try_recv();

    // Fail the call
    call_manager
        .fail_call(call_id, "Network timeout".to_string())
        .await
        .unwrap();

    // Verify ConnectionFailed event
    let event = event_rx.try_recv().unwrap();
    match event {
        CallEvent::ConnectionFailed {
            call_id: eid,
            error,
        } => {
            assert_eq!(eid, call_id);
            assert_eq!(error, "Network timeout");
        }
        other => panic!("Expected ConnectionFailed, got: {:?}", other),
    }

    // Verify state is Failed
    let state = call_manager.get_call_state(call_id).await;
    assert_eq!(state, Some(CallState::Failed));
}

/// Test capability mismatch rejection
#[tokio::test]
async fn test_quic_native_capability_mismatch() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let callee = PeerIdentityString::new("callee");
    let constraints = MediaConstraints::video_call(); // Requires video
    let peer = test_peer();

    // Initiate QUIC call
    let call_id = call_manager
        .initiate_quic_call(callee, constraints, peer)
        .await
        .unwrap();

    // Try to confirm with audio-only capabilities (missing video)
    let peer_caps = MediaCapabilities::audio_only();
    let result = call_manager.confirm_connection(call_id, peer_caps).await;

    // Should fail due to capability mismatch
    assert!(result.is_err());

    // Verify state is still Connecting (not Failed)
    let state = call_manager.get_call_state(call_id).await;
    assert_eq!(state, Some(CallState::Connecting));
}

/// Test get_call_info helper
#[tokio::test]
async fn test_quic_native_call_info() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let callee = PeerIdentityString::new("callee");
    let constraints = MediaConstraints::screen_share();
    let peer = test_peer();

    let call_id = call_manager
        .initiate_quic_call(callee, constraints.clone(), peer)
        .await
        .unwrap();

    // Get call info
    let info = call_manager.get_call_info(call_id).await;
    assert!(info.is_some());

    let (state, info_constraints, has_transport) = info.unwrap();

    // Verify info
    assert_eq!(state, CallState::Connecting);
    assert!(info_constraints.screen_share);
    assert!(has_transport); // QUIC calls have media transport
}

/// Test SignalingMessage QUIC-native variants
#[tokio::test]
async fn test_signaling_message_quic_variants() {
    use saorsa_webrtc_core::signaling::SignalingMessage;

    // Test CapabilityExchange
    let cap_exchange = SignalingMessage::CapabilityExchange {
        session_id: "sess-1".to_string(),
        audio: true,
        video: false,
        data_channel: true,
        max_bandwidth_kbps: 1000,
        quic_endpoint: Some("192.168.1.1:4433".parse().unwrap()),
    };

    assert!(cap_exchange.is_quic_native());
    assert!(!cap_exchange.is_legacy_webrtc());
    assert_eq!(cap_exchange.session_id(), "sess-1");

    // Test serialization roundtrip
    let json = serde_json::to_string(&cap_exchange).unwrap();
    assert!(json.contains("capability_exchange"));

    let deserialized: SignalingMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, cap_exchange);

    // Test ConnectionConfirm
    let confirm = SignalingMessage::ConnectionConfirm {
        session_id: "sess-1".to_string(),
        audio: true,
        video: false,
        data_channel: true,
        max_bandwidth_kbps: 1000,
        quic_endpoint: None,
    };

    assert!(confirm.is_quic_native());
    assert_eq!(confirm.session_id(), "sess-1");

    // Test ConnectionReady
    let ready = SignalingMessage::ConnectionReady {
        session_id: "sess-1".to_string(),
    };

    assert!(ready.is_quic_native());
    assert_eq!(ready.session_id(), "sess-1");
}
