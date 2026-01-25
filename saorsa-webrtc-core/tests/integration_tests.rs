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

// ============================================================================
// END-TO-END QUIC CALL FLOW TESTS (Task 2)
// ============================================================================

/// Test complete call lifecycle with media stream setup and teardown
#[tokio::test]
async fn test_e2e_complete_call_lifecycle_with_media() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let mut media_manager = MediaStreamManager::new();
    media_manager.initialize().await.unwrap();

    let callee = PeerIdentityString::new("callee");
    let constraints = MediaConstraints::video_call();
    let peer = test_peer();

    // Step 1: Initiate call
    let call_id = call_manager
        .initiate_quic_call(callee.clone(), constraints.clone(), peer)
        .await
        .unwrap();

    assert_eq!(
        call_manager.get_call_state(call_id).await,
        Some(CallState::Connecting)
    );

    // Step 2: Create media tracks
    let _audio_track = media_manager.create_audio_track().await.unwrap();
    let _video_track = media_manager.create_video_track().await.unwrap();

    assert_eq!(media_manager.get_webrtc_tracks().len(), 2);

    // Step 3: Confirm connection
    let peer_caps = MediaCapabilities {
        audio: true,
        video: true,
        data_channel: false,
        max_bandwidth_kbps: 2500,
    };

    call_manager
        .confirm_connection(call_id, peer_caps)
        .await
        .unwrap();

    assert_eq!(
        call_manager.get_call_state(call_id).await,
        Some(CallState::Connected)
    );

    // Step 4: Verify call info
    let (state, call_constraints, has_transport) =
        call_manager.get_call_info(call_id).await.unwrap();
    assert_eq!(state, CallState::Connected);
    assert!(call_constraints.has_audio());
    assert!(call_constraints.has_video());
    assert!(has_transport);

    // Step 5: End call
    call_manager.end_call(call_id).await.unwrap();

    // Step 6: Verify cleanup
    assert_eq!(call_manager.get_call_state(call_id).await, None);

    // Media tracks remain in media manager (they're independent)
    assert_eq!(media_manager.get_webrtc_tracks().len(), 2);
}

/// Test bidirectional media stream setup
#[tokio::test]
async fn test_e2e_bidirectional_media_streams() {
    use saorsa_webrtc_core::quic_streams::{MediaStreamType, QoSParams, QuicMediaStreamManager};

    // Create stream managers for both peers
    let mut peer1_streams = QuicMediaStreamManager::new(QoSParams::video());
    let mut peer2_streams = QuicMediaStreamManager::new(QoSParams::video());

    // Peer 1 creates outgoing streams
    let p1_audio = peer1_streams.create_stream(MediaStreamType::Audio).unwrap();
    let p1_video = peer1_streams.create_stream(MediaStreamType::Video).unwrap();

    // Peer 2 creates outgoing streams
    let p2_audio = peer2_streams.create_stream(MediaStreamType::Audio).unwrap();
    let p2_video = peer2_streams.create_stream(MediaStreamType::Video).unwrap();

    // Verify both peers have their streams
    assert_eq!(peer1_streams.active_streams().len(), 2);
    assert_eq!(peer2_streams.active_streams().len(), 2);

    // Verify stream types
    assert_eq!(
        peer1_streams.get_stream(p1_audio).unwrap().stream_type,
        MediaStreamType::Audio
    );
    assert_eq!(
        peer1_streams.get_stream(p1_video).unwrap().stream_type,
        MediaStreamType::Video
    );

    // Close streams (call end)
    peer1_streams.close_stream(p1_audio).unwrap();
    peer1_streams.close_stream(p1_video).unwrap();
    peer2_streams.close_stream(p2_audio).unwrap();
    peer2_streams.close_stream(p2_video).unwrap();

    // Verify cleanup
    assert_eq!(peer1_streams.active_streams().len(), 0);
    assert_eq!(peer2_streams.active_streams().len(), 0);
}

/// Test media stream creation with different constraints
#[tokio::test]
async fn test_e2e_media_streams_various_constraints() {
    let mut media_manager = MediaStreamManager::new();
    media_manager.initialize().await.unwrap();

    // Audio-only call
    {
        let audio_track = media_manager.create_audio_track().await.unwrap();
        assert_eq!(audio_track.track_type, MediaType::Audio);
        assert!(audio_track.id.starts_with("audio-"));
    }

    // Video call (audio + video)
    {
        let video_track = media_manager.create_video_track().await.unwrap();
        assert_eq!(video_track.track_type, MediaType::Video);
        assert!(video_track.id.starts_with("video-"));
    }

    // Verify tracks created
    assert_eq!(media_manager.get_webrtc_tracks().len(), 2);

    // Verify track IDs are unique
    let tracks = media_manager.get_webrtc_tracks();
    let ids: Vec<&str> = tracks.iter().map(|t| t.id.as_str()).collect();
    let unique_ids: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique_ids.len());
}

/// Test resource cleanup after call failure
#[tokio::test]
async fn test_e2e_resource_cleanup_on_failure() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let callee = PeerIdentityString::new("callee");
    let constraints = MediaConstraints::audio_only();
    let peer = test_peer();

    // Initiate call
    let call_id = call_manager
        .initiate_quic_call(callee, constraints, peer)
        .await
        .unwrap();

    assert_eq!(
        call_manager.get_call_state(call_id).await,
        Some(CallState::Connecting)
    );

    // Fail the call
    call_manager
        .fail_call(call_id, "Test failure".to_string())
        .await
        .unwrap();

    assert_eq!(
        call_manager.get_call_state(call_id).await,
        Some(CallState::Failed)
    );

    // Verify call can be ended even in Failed state
    call_manager.end_call(call_id).await.unwrap();
    assert_eq!(call_manager.get_call_state(call_id).await, None);
}

/// Test QUIC stream multiplexing for different media types
#[tokio::test]
async fn test_e2e_quic_stream_multiplexing() {
    use saorsa_webrtc_core::quic_streams::{MediaStreamType, QoSParams, QuicMediaStreamManager};

    let mut stream_manager = QuicMediaStreamManager::new(QoSParams::video());

    // Create all media stream types
    let audio_id = stream_manager
        .create_stream(MediaStreamType::Audio)
        .unwrap();
    let video_id = stream_manager
        .create_stream(MediaStreamType::Video)
        .unwrap();
    let screen_id = stream_manager
        .create_stream(MediaStreamType::ScreenShare)
        .unwrap();
    let data_id = stream_manager
        .create_stream(MediaStreamType::DataChannel)
        .unwrap();

    // Verify all streams active
    assert_eq!(stream_manager.active_streams().len(), 4);

    // Verify stream IDs are sequential
    assert_eq!(audio_id, 0);
    assert_eq!(video_id, 1);
    assert_eq!(screen_id, 2);
    assert_eq!(data_id, 3);

    // Verify each stream has correct type
    assert_eq!(
        stream_manager.get_stream(audio_id).unwrap().stream_type,
        MediaStreamType::Audio
    );
    assert_eq!(
        stream_manager.get_stream(video_id).unwrap().stream_type,
        MediaStreamType::Video
    );
    assert_eq!(
        stream_manager.get_stream(screen_id).unwrap().stream_type,
        MediaStreamType::ScreenShare
    );
    assert_eq!(
        stream_manager.get_stream(data_id).unwrap().stream_type,
        MediaStreamType::DataChannel
    );

    // Close streams in different order
    stream_manager.close_stream(video_id).unwrap();
    assert_eq!(stream_manager.active_streams().len(), 3);

    stream_manager.close_stream(audio_id).unwrap();
    assert_eq!(stream_manager.active_streams().len(), 2);

    stream_manager.close_stream(data_id).unwrap();
    stream_manager.close_stream(screen_id).unwrap();
    assert_eq!(stream_manager.active_streams().len(), 0);
}

/// Test signaling message flow for QUIC-native calls
#[tokio::test]
async fn test_e2e_quic_signaling_flow() {
    use saorsa_webrtc_core::signaling::SignalingMessage;

    // Step 1: Capability exchange
    let cap_exchange = SignalingMessage::CapabilityExchange {
        session_id: "session-1".to_string(),
        audio: true,
        video: true,
        data_channel: false,
        max_bandwidth_kbps: 2500,
        quic_endpoint: Some("192.168.1.1:4433".parse().unwrap()),
    };

    assert!(cap_exchange.is_quic_native());
    assert_eq!(cap_exchange.session_id(), "session-1");

    // Step 2: Connection confirm
    let confirm = SignalingMessage::ConnectionConfirm {
        session_id: "session-1".to_string(),
        audio: true,
        video: true,
        data_channel: false,
        max_bandwidth_kbps: 2500,
        quic_endpoint: Some("192.168.1.2:4433".parse().unwrap()),
    };

    assert!(confirm.is_quic_native());
    assert_eq!(confirm.session_id(), cap_exchange.session_id());

    // Step 3: Connection ready
    let ready = SignalingMessage::ConnectionReady {
        session_id: "session-1".to_string(),
    };

    assert!(ready.is_quic_native());
    assert_eq!(ready.session_id(), "session-1");

    // Verify all messages share same session
    assert_eq!(cap_exchange.session_id(), confirm.session_id());
    assert_eq!(confirm.session_id(), ready.session_id());
}

// ============================================================================
// MULTI-PEER TEST SCENARIOS (Task 3)
// ============================================================================

/// Test simultaneous calls to multiple peers
#[tokio::test]
async fn test_multi_peer_simultaneous_calls() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let peer1 = PeerIdentityString::new("peer1");
    let peer2 = PeerIdentityString::new("peer2");
    let peer3 = PeerIdentityString::new("peer3");

    let constraints = MediaConstraints::audio_only();

    // Initiate calls to all three peers simultaneously
    let call1 = call_manager
        .initiate_call(peer1, constraints.clone())
        .await
        .unwrap();

    let call2 = call_manager
        .initiate_call(peer2, constraints.clone())
        .await
        .unwrap();

    let call3 = call_manager
        .initiate_call(peer3, constraints.clone())
        .await
        .unwrap();

    // Verify all calls are in Calling state
    assert_eq!(
        call_manager.get_call_state(call1).await,
        Some(CallState::Calling)
    );
    assert_eq!(
        call_manager.get_call_state(call2).await,
        Some(CallState::Calling)
    );
    assert_eq!(
        call_manager.get_call_state(call3).await,
        Some(CallState::Calling)
    );

    // Accept all calls
    call_manager
        .accept_call(call1, constraints.clone())
        .await
        .unwrap();
    call_manager
        .accept_call(call2, constraints.clone())
        .await
        .unwrap();
    call_manager
        .accept_call(call3, constraints.clone())
        .await
        .unwrap();

    // Verify all calls are connected
    assert_eq!(
        call_manager.get_call_state(call1).await,
        Some(CallState::Connected)
    );
    assert_eq!(
        call_manager.get_call_state(call2).await,
        Some(CallState::Connected)
    );
    assert_eq!(
        call_manager.get_call_state(call3).await,
        Some(CallState::Connected)
    );

    // End calls one by one
    call_manager.end_call(call1).await.unwrap();
    assert_eq!(call_manager.get_call_state(call1).await, None);

    // Other calls should still be active
    assert_eq!(
        call_manager.get_call_state(call2).await,
        Some(CallState::Connected)
    );
    assert_eq!(
        call_manager.get_call_state(call3).await,
        Some(CallState::Connected)
    );

    // End remaining calls
    call_manager.end_call(call2).await.unwrap();
    call_manager.end_call(call3).await.unwrap();

    assert_eq!(call_manager.get_call_state(call2).await, None);
    assert_eq!(call_manager.get_call_state(call3).await, None);
}

/// Test resource isolation between concurrent calls
#[tokio::test]
async fn test_multi_peer_resource_isolation() {
    use saorsa_webrtc_core::quic_streams::{MediaStreamType, QoSParams, QuicMediaStreamManager};

    // Create separate stream managers for different peers
    let mut peer1_streams = QuicMediaStreamManager::new(QoSParams::audio());
    let mut peer2_streams = QuicMediaStreamManager::new(QoSParams::video());
    let mut peer3_streams = QuicMediaStreamManager::new(QoSParams::video());

    // Create streams for peer 1 (audio only)
    let p1_audio = peer1_streams.create_stream(MediaStreamType::Audio).unwrap();

    // Create streams for peer 2 (video call)
    let p2_audio = peer2_streams.create_stream(MediaStreamType::Audio).unwrap();
    let p2_video = peer2_streams.create_stream(MediaStreamType::Video).unwrap();

    // Create streams for peer 3 (screen share)
    let p3_audio = peer3_streams.create_stream(MediaStreamType::Audio).unwrap();
    let p3_screen = peer3_streams
        .create_stream(MediaStreamType::ScreenShare)
        .unwrap();

    // Verify stream counts
    assert_eq!(peer1_streams.active_streams().len(), 1);
    assert_eq!(peer2_streams.active_streams().len(), 2);
    assert_eq!(peer3_streams.active_streams().len(), 2);

    // Verify stream types are independent
    assert_eq!(
        peer1_streams.get_stream(p1_audio).unwrap().stream_type,
        MediaStreamType::Audio
    );
    assert_eq!(
        peer2_streams.get_stream(p2_audio).unwrap().stream_type,
        MediaStreamType::Audio
    );
    assert_eq!(
        peer2_streams.get_stream(p2_video).unwrap().stream_type,
        MediaStreamType::Video
    );
    assert_eq!(
        peer3_streams.get_stream(p3_audio).unwrap().stream_type,
        MediaStreamType::Audio
    );
    assert_eq!(
        peer3_streams.get_stream(p3_screen).unwrap().stream_type,
        MediaStreamType::ScreenShare
    );

    // Close peer 2 streams (simulate call end)
    peer2_streams.close_stream(p2_audio).unwrap();
    peer2_streams.close_stream(p2_video).unwrap();
    assert_eq!(peer2_streams.active_streams().len(), 0);

    // Verify other peers' streams are unaffected
    assert_eq!(peer1_streams.active_streams().len(), 1);
    assert_eq!(peer3_streams.active_streams().len(), 2);
}

/// Test call rejection handling with multiple peers
#[tokio::test]
#[allow(deprecated)]
async fn test_multi_peer_call_rejection() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let peer1 = PeerIdentityString::new("peer1");
    let peer2 = PeerIdentityString::new("peer2");

    let constraints = MediaConstraints::video_call();

    // Initiate calls
    let call1 = call_manager
        .initiate_call(peer1, constraints.clone())
        .await
        .unwrap();

    let call2 = call_manager
        .initiate_call(peer2, constraints.clone())
        .await
        .unwrap();

    // Reject call1
    call_manager.reject_call(call1).await.unwrap();

    // Accept call2
    call_manager.accept_call(call2, constraints).await.unwrap();

    // Verify states (rejected calls go to Failed state)
    assert_eq!(
        call_manager.get_call_state(call1).await,
        Some(CallState::Failed)
    );
    assert_eq!(
        call_manager.get_call_state(call2).await,
        Some(CallState::Connected)
    );

    // Clean up rejected call
    call_manager.end_call(call1).await.unwrap();
    assert_eq!(call_manager.get_call_state(call1).await, None);

    // Clean up
    call_manager.end_call(call2).await.unwrap();
}

/// Test concurrent media tracks for multiple calls
#[tokio::test]
async fn test_multi_peer_concurrent_media_tracks() {
    let mut media_manager = MediaStreamManager::new();
    media_manager.initialize().await.unwrap();

    // Create tracks for call 1 (audio only)
    {
        let track = media_manager.create_audio_track().await.unwrap();
        assert!(track.id.starts_with("audio-"));
    }

    // Create tracks for call 2 (video call)
    {
        let audio = media_manager.create_audio_track().await.unwrap();
        assert!(audio.id.starts_with("audio-"));
    }
    {
        let video = media_manager.create_video_track().await.unwrap();
        assert!(video.id.starts_with("video-"));
    }

    // Verify total tracks created
    assert_eq!(media_manager.get_webrtc_tracks().len(), 3);

    // Verify all tracks have unique IDs
    let tracks = media_manager.get_webrtc_tracks();
    let ids: Vec<&str> = tracks.iter().map(|t| t.id.as_str()).collect();
    let unique_ids: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique_ids.len());
}

/// Test QUIC-native multi-peer calls
#[tokio::test]
async fn test_multi_peer_quic_native_calls() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let peer1 = PeerIdentityString::new("peer1");
    let peer2 = PeerIdentityString::new("peer2");

    let audio_constraints = MediaConstraints::audio_only();
    let video_constraints = MediaConstraints::video_call();

    // Create different peer connections
    let peer1_conn = saorsa_webrtc_core::link_transport::PeerConnection {
        peer_id: "peer1".to_string(),
        remote_addr: "127.0.0.1:9001".parse().unwrap(),
    };
    let peer2_conn = saorsa_webrtc_core::link_transport::PeerConnection {
        peer_id: "peer2".to_string(),
        remote_addr: "127.0.0.1:9002".parse().unwrap(),
    };

    // Initiate QUIC calls
    let call1 = call_manager
        .initiate_quic_call(peer1, audio_constraints.clone(), peer1_conn)
        .await
        .unwrap();

    let call2 = call_manager
        .initiate_quic_call(peer2, video_constraints.clone(), peer2_conn)
        .await
        .unwrap();

    // Verify both calls are in Connecting state
    assert_eq!(
        call_manager.get_call_state(call1).await,
        Some(CallState::Connecting)
    );
    assert_eq!(
        call_manager.get_call_state(call2).await,
        Some(CallState::Connecting)
    );

    // Confirm connections with appropriate capabilities
    let audio_caps = MediaCapabilities::audio_only();
    let video_caps = MediaCapabilities {
        audio: true,
        video: true,
        data_channel: false,
        max_bandwidth_kbps: 2500,
    };

    call_manager
        .confirm_connection(call1, audio_caps)
        .await
        .unwrap();
    call_manager
        .confirm_connection(call2, video_caps)
        .await
        .unwrap();

    // Verify both calls are connected
    assert_eq!(
        call_manager.get_call_state(call1).await,
        Some(CallState::Connected)
    );
    assert_eq!(
        call_manager.get_call_state(call2).await,
        Some(CallState::Connected)
    );

    // Verify call info
    let (state1, constraints1, has_transport1) = call_manager.get_call_info(call1).await.unwrap();
    let (state2, constraints2, has_transport2) = call_manager.get_call_info(call2).await.unwrap();

    assert_eq!(state1, CallState::Connected);
    assert_eq!(state2, CallState::Connected);
    assert!(has_transport1);
    assert!(has_transport2);

    assert!(constraints1.has_audio());
    assert!(!constraints1.has_video());

    assert!(constraints2.has_audio());
    assert!(constraints2.has_video());

    // Clean up
    call_manager.end_call(call1).await.unwrap();
    call_manager.end_call(call2).await.unwrap();
}

// ============================================================================
// ERROR HANDLING INTEGRATION TESTS (Task 4)
// ============================================================================

/// Test invalid state transitions
#[tokio::test]
async fn test_error_invalid_state_transitions() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let peer = PeerIdentityString::new("peer");
    let constraints = MediaConstraints::audio_only();
    let peer_conn = test_peer();

    // Initiate QUIC call
    let call_id = call_manager
        .initiate_quic_call(peer, constraints.clone(), peer_conn)
        .await
        .unwrap();

    // Confirm connection properly
    let caps = MediaCapabilities::audio_only();
    call_manager
        .confirm_connection(call_id, caps.clone())
        .await
        .unwrap();

    // Try to confirm again (already Connected)
    let result = call_manager.confirm_connection(call_id, caps).await;
    assert!(result.is_err());

    // Try to accept an already connected call
    let result = call_manager.accept_call(call_id, constraints.clone()).await;
    assert!(result.is_err());

    // Clean up
    call_manager.end_call(call_id).await.unwrap();
}

/// Test capability mismatch scenarios
#[tokio::test]
async fn test_error_capability_mismatch_scenarios() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    // Test 1: Video call with audio-only response
    {
        let peer = PeerIdentityString::new("peer1");
        let video_constraints = MediaConstraints::video_call();
        let peer_conn = test_peer();

        let call_id = call_manager
            .initiate_quic_call(peer, video_constraints, peer_conn)
            .await
            .unwrap();

        // Try to confirm with audio-only (missing video)
        let audio_caps = MediaCapabilities::audio_only();
        let result = call_manager.confirm_connection(call_id, audio_caps).await;
        assert!(result.is_err());

        call_manager.end_call(call_id).await.unwrap();
    }

    // Test 2: Audio-only call with video response (should succeed - peer has more capabilities)
    {
        let peer = PeerIdentityString::new("peer2");
        let audio_constraints = MediaConstraints::audio_only();
        let peer_conn = saorsa_webrtc_core::link_transport::PeerConnection {
            peer_id: "peer2".to_string(),
            remote_addr: "127.0.0.1:9001".parse().unwrap(),
        };

        let call_id = call_manager
            .initiate_quic_call(peer, audio_constraints, peer_conn)
            .await
            .unwrap();

        // Confirm with video capabilities (peer has more than required)
        let video_caps = MediaCapabilities {
            audio: true,
            video: true,
            data_channel: false,
            max_bandwidth_kbps: 2500,
        };
        let result = call_manager.confirm_connection(call_id, video_caps).await;
        // This should succeed since peer has at least the required capabilities
        assert!(result.is_ok());

        call_manager.end_call(call_id).await.unwrap();
    }
}

/// Test operations on non-existent or ended calls
#[tokio::test]
#[allow(deprecated)]
async fn test_error_operations_on_invalid_calls() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    // Test with completely fake call ID
    let fake_id = CallId::new();

    assert!(call_manager
        .accept_call(fake_id, MediaConstraints::audio_only())
        .await
        .is_err());
    assert!(call_manager.reject_call(fake_id).await.is_err());
    assert!(call_manager.end_call(fake_id).await.is_err());
    assert!(call_manager.create_offer(fake_id).await.is_err());
    assert!(call_manager
        .add_ice_candidate(fake_id, "dummy".to_string())
        .await
        .is_err());

    // Test with ended call
    let peer = PeerIdentityString::new("peer");
    let constraints = MediaConstraints::audio_only();

    let call_id = call_manager
        .initiate_call(peer, constraints.clone())
        .await
        .unwrap();

    call_manager.end_call(call_id).await.unwrap();

    // Operations on ended call should fail
    assert!(call_manager
        .accept_call(call_id, constraints)
        .await
        .is_err());
    assert!(call_manager.reject_call(call_id).await.is_err());
}

/// Test stream errors and recovery
#[tokio::test]
async fn test_error_stream_handling() {
    use saorsa_webrtc_core::quic_streams::{MediaStreamType, QoSParams, QuicMediaStreamManager};

    let mut stream_manager = QuicMediaStreamManager::new(QoSParams::audio());

    // Create a stream
    let stream_id = stream_manager
        .create_stream(MediaStreamType::Audio)
        .unwrap();

    // Close the stream
    stream_manager.close_stream(stream_id).unwrap();

    // Try to get closed stream (should return None)
    assert!(stream_manager.get_stream(stream_id).is_none());

    // Try to close already closed stream (should fail)
    let result = stream_manager.close_stream(stream_id);
    assert!(result.is_err());

    // Try to close non-existent stream
    let result = stream_manager.close_stream(999);
    assert!(result.is_err());
}

/// Test call failure propagation
#[tokio::test]
async fn test_error_call_failure_propagation() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let mut event_rx = call_manager.subscribe_events();

    let peer = PeerIdentityString::new("peer");
    let constraints = MediaConstraints::audio_only();
    let peer_conn = test_peer();

    // Initiate call
    let call_id = call_manager
        .initiate_quic_call(peer, constraints, peer_conn)
        .await
        .unwrap();

    // Drain CallInitiated event
    let _ = event_rx.try_recv();

    // Fail the call
    let error_msg = "Network timeout";
    call_manager
        .fail_call(call_id, error_msg.to_string())
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
            assert_eq!(error, error_msg);
        }
        other => panic!("Expected ConnectionFailed, got: {:?}", other),
    }

    // Verify state
    assert_eq!(
        call_manager.get_call_state(call_id).await,
        Some(CallState::Failed)
    );

    // Clean up
    call_manager.end_call(call_id).await.unwrap();
}

/// Test concurrent call failures don't affect other calls
#[tokio::test]
async fn test_error_isolated_call_failures() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let peer1 = PeerIdentityString::new("peer1");
    let peer2 = PeerIdentityString::new("peer2");
    let constraints = MediaConstraints::audio_only();

    // Start two calls
    let call1 = call_manager
        .initiate_call(peer1, constraints.clone())
        .await
        .unwrap();

    let call2 = call_manager
        .initiate_call(peer2, constraints.clone())
        .await
        .unwrap();

    // Accept both calls
    call_manager
        .accept_call(call1, constraints.clone())
        .await
        .unwrap();
    call_manager
        .accept_call(call2, constraints.clone())
        .await
        .unwrap();

    // Fail call1
    call_manager
        .fail_call(call1, "Test failure".to_string())
        .await
        .unwrap();

    // Verify call1 failed but call2 still connected
    assert_eq!(
        call_manager.get_call_state(call1).await,
        Some(CallState::Failed)
    );
    assert_eq!(
        call_manager.get_call_state(call2).await,
        Some(CallState::Connected)
    );

    // Clean up
    call_manager.end_call(call1).await.unwrap();
    call_manager.end_call(call2).await.unwrap();
}

/// Test QUIC transport configuration errors
#[tokio::test]
async fn test_error_quic_transport_config() {
    let mut media_manager = MediaStreamManager::new();
    media_manager.initialize().await.unwrap();

    // Try to create QUIC tracks without transport configuration
    let result = media_manager.create_quic_audio_track();
    assert!(result.is_err());

    let result = media_manager.create_quic_video_track(1920, 1080);
    assert!(result.is_err());

    let result = media_manager.create_quic_screen_track(1920, 1080);
    assert!(result.is_err());
}

// ============================================================================
// CONNECTION STATE MANAGEMENT TESTS (Task 5)
// ============================================================================
// Note: True connection migration is handled by ant-quic. These tests verify
// that saorsa-webrtc-core properly manages call states during connection changes.

/// Test call state persistence across reconnection scenarios
#[tokio::test]
async fn test_connection_state_call_persistence() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let peer = PeerIdentityString::new("peer");
    let constraints = MediaConstraints::audio_only();
    let peer_conn = test_peer();

    // Initiate and confirm call
    let call_id = call_manager
        .initiate_quic_call(peer.clone(), constraints.clone(), peer_conn)
        .await
        .unwrap();

    let caps = MediaCapabilities::audio_only();
    call_manager
        .confirm_connection(call_id, caps)
        .await
        .unwrap();

    assert_eq!(
        call_manager.get_call_state(call_id).await,
        Some(CallState::Connected)
    );

    // Simulate temporary disconnect by checking call still exists
    // In real scenario, QUIC would handle migration transparently
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Call should still be in connected state
    assert_eq!(
        call_manager.get_call_state(call_id).await,
        Some(CallState::Connected)
    );

    // Clean up
    call_manager.end_call(call_id).await.unwrap();
}

/// Test graceful handling of connection state transitions
#[tokio::test]
async fn test_connection_state_transitions() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let mut event_rx = call_manager.subscribe_events();

    let peer = PeerIdentityString::new("peer");
    let constraints = MediaConstraints::video_call();
    let peer_conn = test_peer();

    // Step 1: Initiate → Connecting
    let call_id = call_manager
        .initiate_quic_call(peer, constraints, peer_conn)
        .await
        .unwrap();

    let event = event_rx.try_recv().unwrap();
    assert!(matches!(event, CallEvent::CallInitiated { .. }));
    assert_eq!(
        call_manager.get_call_state(call_id).await,
        Some(CallState::Connecting)
    );

    // Step 2: Connecting → Connected
    let caps = MediaCapabilities {
        audio: true,
        video: true,
        data_channel: false,
        max_bandwidth_kbps: 2500,
    };
    call_manager
        .confirm_connection(call_id, caps)
        .await
        .unwrap();

    let event = event_rx.try_recv().unwrap();
    assert!(matches!(event, CallEvent::ConnectionEstablished { .. }));
    assert_eq!(
        call_manager.get_call_state(call_id).await,
        Some(CallState::Connected)
    );

    // Step 3: Connected → Ended (via end_call)
    call_manager.end_call(call_id).await.unwrap();

    let event = event_rx.try_recv().unwrap();
    assert!(matches!(event, CallEvent::CallEnded { .. }));
    assert_eq!(call_manager.get_call_state(call_id).await, None);
}

/// Test connection state with multiple concurrent calls during state changes
#[tokio::test]
async fn test_connection_state_multiple_calls_transitions() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let peer1 = PeerIdentityString::new("peer1");
    let peer2 = PeerIdentityString::new("peer2");
    let peer3 = PeerIdentityString::new("peer3");

    let constraints = MediaConstraints::audio_only();

    let peer1_conn = saorsa_webrtc_core::link_transport::PeerConnection {
        peer_id: "peer1".to_string(),
        remote_addr: "127.0.0.1:9001".parse().unwrap(),
    };
    let peer2_conn = saorsa_webrtc_core::link_transport::PeerConnection {
        peer_id: "peer2".to_string(),
        remote_addr: "127.0.0.1:9002".parse().unwrap(),
    };

    // Start calls to peer1 and peer2
    let call1 = call_manager
        .initiate_quic_call(peer1, constraints.clone(), peer1_conn)
        .await
        .unwrap();

    let call2 = call_manager
        .initiate_quic_call(peer2, constraints.clone(), peer2_conn)
        .await
        .unwrap();

    // Confirm both
    let caps = MediaCapabilities::audio_only();
    call_manager
        .confirm_connection(call1, caps.clone())
        .await
        .unwrap();
    call_manager
        .confirm_connection(call2, caps.clone())
        .await
        .unwrap();

    // Both calls connected
    assert_eq!(
        call_manager.get_call_state(call1).await,
        Some(CallState::Connected)
    );
    assert_eq!(
        call_manager.get_call_state(call2).await,
        Some(CallState::Connected)
    );

    // Start new call while others are active
    let call3 = call_manager
        .initiate_call(peer3, constraints.clone())
        .await
        .unwrap();

    assert_eq!(
        call_manager.get_call_state(call3).await,
        Some(CallState::Calling)
    );

    // Accept call3
    call_manager.accept_call(call3, constraints).await.unwrap();

    // All three calls should be active
    assert_eq!(
        call_manager.get_call_state(call1).await,
        Some(CallState::Connected)
    );
    assert_eq!(
        call_manager.get_call_state(call2).await,
        Some(CallState::Connected)
    );
    assert_eq!(
        call_manager.get_call_state(call3).await,
        Some(CallState::Connected)
    );

    // End calls in different order
    call_manager.end_call(call2).await.unwrap();
    assert_eq!(call_manager.get_call_state(call2).await, None);

    // Other calls unaffected
    assert_eq!(
        call_manager.get_call_state(call1).await,
        Some(CallState::Connected)
    );
    assert_eq!(
        call_manager.get_call_state(call3).await,
        Some(CallState::Connected)
    );

    // Clean up
    call_manager.end_call(call1).await.unwrap();
    call_manager.end_call(call3).await.unwrap();
}

/// Test media stream continuity during simulated reconnection
#[tokio::test]
async fn test_connection_state_media_stream_continuity() {
    use saorsa_webrtc_core::quic_streams::{MediaStreamType, QoSParams, QuicMediaStreamManager};

    let mut stream_manager = QuicMediaStreamManager::new(QoSParams::video());

    // Create streams for a video call
    let audio_id = stream_manager
        .create_stream(MediaStreamType::Audio)
        .unwrap();
    let video_id = stream_manager
        .create_stream(MediaStreamType::Video)
        .unwrap();

    assert_eq!(stream_manager.active_streams().len(), 2);

    // Simulate reconnection by verifying streams persist
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Streams should still be active
    assert_eq!(stream_manager.active_streams().len(), 2);
    assert!(stream_manager.get_stream(audio_id).is_some());
    assert!(stream_manager.get_stream(video_id).is_some());

    // Verify stream types unchanged
    assert_eq!(
        stream_manager.get_stream(audio_id).unwrap().stream_type,
        MediaStreamType::Audio
    );
    assert_eq!(
        stream_manager.get_stream(video_id).unwrap().stream_type,
        MediaStreamType::Video
    );

    // Clean up
    stream_manager.close_stream(audio_id).unwrap();
    stream_manager.close_stream(video_id).unwrap();
    assert_eq!(stream_manager.active_streams().len(), 0);
}

/// Test NAT rebinding simulation (connection endpoint change)
#[tokio::test]
async fn test_connection_state_endpoint_change() {
    let config = CallManagerConfig::default();
    let call_manager = CallManager::<PeerIdentityString>::new(config)
        .await
        .unwrap();

    let peer = PeerIdentityString::new("peer");
    let constraints = MediaConstraints::audio_only();

    // Initial connection
    let peer_conn1 = saorsa_webrtc_core::link_transport::PeerConnection {
        peer_id: "peer".to_string(),
        remote_addr: "127.0.0.1:9000".parse().unwrap(),
    };

    let call_id = call_manager
        .initiate_quic_call(peer.clone(), constraints.clone(), peer_conn1)
        .await
        .unwrap();

    let caps = MediaCapabilities::audio_only();
    call_manager
        .confirm_connection(call_id, caps)
        .await
        .unwrap();

    assert_eq!(
        call_manager.get_call_state(call_id).await,
        Some(CallState::Connected)
    );

    // Simulate NAT rebinding by getting call info
    // In real scenario, QUIC would update connection path transparently
    let (state, _, has_transport) = call_manager.get_call_info(call_id).await.unwrap();
    assert_eq!(state, CallState::Connected);
    assert!(has_transport);

    // Call should remain connected after rebinding
    assert_eq!(
        call_manager.get_call_state(call_id).await,
        Some(CallState::Connected)
    );

    // Clean up
    call_manager.end_call(call_id).await.unwrap();
}
