//! Integration tests for end-to-end WebRTC functionality

use saorsa_webrtc_core::signaling::SignalingMessage;
use saorsa_webrtc_core::{
    CallId, CallManager, CallManagerConfig, CallState, MediaConstraints, MediaStreamManager,
    MediaType, PeerIdentityString, SignalingHandler, SignalingTransport,
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

    // Test stream operations
    let data = vec![1, 2, 3, 4, 5];
    stream_manager
        .send_data(audio_stream_id, &data)
        .await
        .unwrap();

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
