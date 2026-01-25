//! Integration tests for track backend functionality
//!
//! Tests the complete flow of creating QUIC-backed tracks, sending/receiving
//! data, and verifying statistics propagation.

use saorsa_webrtc_core::link_transport::PeerConnection;
use saorsa_webrtc_core::media::{
    AudioTrack, GenericTrack, MediaStreamManager, QuicTrackBackend, TrackBackend, VideoTrack,
};
use saorsa_webrtc_core::quic_media_transport::QuicMediaTransport;
use saorsa_webrtc_core::types::MediaType;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

fn test_peer() -> PeerConnection {
    PeerConnection {
        peer_id: "integration-test-peer".to_string(),
        remote_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080),
    }
}

// ============================================================================
// End-to-End Track Creation Tests
// ============================================================================

#[tokio::test]
async fn test_create_quic_transport_and_audio_track() {
    // Create transport
    let transport = Arc::new(QuicMediaTransport::new());

    // Connect transport
    transport.connect(test_peer()).await.unwrap();
    assert!(transport.is_connected().await);

    // Create audio track with the transport
    let audio = AudioTrack::with_quic("audio-e2e", Arc::clone(&transport));

    // Verify track is connected
    assert!(audio.is_connected());
    assert_eq!(audio.backend().backend_type(), "quic");
}

#[tokio::test]
async fn test_create_quic_transport_and_video_track() {
    // Create transport
    let transport = Arc::new(QuicMediaTransport::new());

    // Connect transport
    transport.connect(test_peer()).await.unwrap();

    // Create video track with the transport
    let video = VideoTrack::with_quic("video-e2e", Arc::clone(&transport), 1280, 720);

    // Verify track is connected
    assert!(video.is_connected());
    assert_eq!(video.width, 1280);
    assert_eq!(video.height, 720);
}

// ============================================================================
// Media Streaming Flow Tests
// ============================================================================

#[tokio::test]
async fn test_send_audio_through_quic_track() {
    let transport = Arc::new(QuicMediaTransport::new());
    transport.connect(test_peer()).await.unwrap();

    let audio = AudioTrack::with_quic("audio-stream", transport);

    // Send simulated Opus audio packet
    let audio_data = vec![0x78, 0x9c, 0x00, 0x01, 0x02, 0x03];
    let result = audio.send_audio(&audio_data).await;
    assert!(result.is_ok());

    // Verify stats were updated
    let stats = audio.stats();
    assert_eq!(stats.bytes_sent, 6);
    assert_eq!(stats.packets_sent, 1);
}

#[tokio::test]
async fn test_send_video_through_quic_track() {
    let transport = Arc::new(QuicMediaTransport::new());
    transport.connect(test_peer()).await.unwrap();

    let video = VideoTrack::with_quic("video-stream", transport, 640, 480);

    // Send simulated H.264 NAL unit
    let video_data = vec![0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x0a];
    let result = video.send_frame(&video_data).await;
    assert!(result.is_ok());

    // Verify stats were updated
    let stats = video.stats();
    assert_eq!(stats.bytes_sent, 8);
    assert_eq!(stats.packets_sent, 1);
}

#[tokio::test]
async fn test_send_multiple_packets() {
    let transport = Arc::new(QuicMediaTransport::new());
    transport.connect(test_peer()).await.unwrap();

    let audio = AudioTrack::with_quic("audio-multi", transport);

    // Send multiple packets
    for i in 0..10 {
        let packet = vec![0x78, 0x9c, i as u8, i as u8];
        audio.send_audio(&packet).await.unwrap();
    }

    // Verify cumulative stats
    let stats = audio.stats();
    assert_eq!(stats.bytes_sent, 40); // 10 packets * 4 bytes
    assert_eq!(stats.packets_sent, 10);
}

// ============================================================================
// Statistics Propagation Tests
// ============================================================================

#[tokio::test]
async fn test_stats_propagation_through_generic_track() {
    let transport = Arc::new(QuicMediaTransport::new());
    transport.connect(test_peer()).await.unwrap();

    let audio = AudioTrack::with_quic("audio-stats", Arc::clone(&transport));
    let generic = GenericTrack::audio(audio);

    // Send through generic track
    let data = vec![0x01, 0x02, 0x03, 0x04, 0x05];
    generic.send(&data).await.unwrap();

    // Stats should be accessible through generic track
    let stats = generic.stats();
    assert_eq!(stats.bytes_sent, 5);
    assert_eq!(stats.packets_sent, 1);
}

#[tokio::test]
async fn test_multiple_tracks_share_transport() {
    let transport = Arc::new(QuicMediaTransport::new());
    transport.connect(test_peer()).await.unwrap();

    // Create multiple tracks sharing the same transport
    let audio = AudioTrack::with_quic("audio-shared", Arc::clone(&transport));
    let video = VideoTrack::with_quic("video-shared", Arc::clone(&transport), 1280, 720);

    // Send through both
    audio.send_audio(&[0x01, 0x02]).await.unwrap();
    video.send_frame(&[0x00, 0x00, 0x00, 0x01]).await.unwrap();

    // Each track has its own stats
    assert_eq!(audio.stats().bytes_sent, 2);
    assert_eq!(video.stats().bytes_sent, 4);
}

// ============================================================================
// MediaStreamManager Integration Tests
// ============================================================================

#[test]
fn test_media_manager_quic_track_creation_flow() {
    let transport = Arc::new(QuicMediaTransport::new());
    let mut manager = MediaStreamManager::with_quic_transport(transport);

    // Create audio track
    let audio_result = manager.create_quic_audio_track();
    assert!(audio_result.is_ok());

    // Create video track
    let video_result = manager.create_quic_video_track(1920, 1080);
    assert!(video_result.is_ok());

    // Verify both tracks exist
    let tracks = manager.get_tracks();
    assert_eq!(tracks.len(), 2);

    // Verify track types
    assert!(tracks[0].is_audio());
    assert!(tracks[1].is_video());
}

#[test]
fn test_media_manager_screen_track_creation() {
    let transport = Arc::new(QuicMediaTransport::new());
    let mut manager = MediaStreamManager::with_quic_transport(transport);

    // Create screen track
    let screen_result = manager.create_quic_screen_track(1920, 1080);
    assert!(screen_result.is_ok());

    let track = screen_result.unwrap();
    assert!(track.is_screen());
    assert_eq!(track.media_type(), MediaType::ScreenShare);
}

#[test]
fn test_media_manager_track_removal() {
    let transport = Arc::new(QuicMediaTransport::new());
    let mut manager = MediaStreamManager::with_quic_transport(transport);

    manager.create_quic_audio_track().unwrap();
    manager.create_quic_video_track(1280, 720).unwrap();

    assert_eq!(manager.get_tracks().len(), 2);

    // Get the audio track ID
    let audio_id = manager.get_tracks()[0].id().to_string();

    // Remove it
    let removed = manager.remove_track(&audio_id);
    assert!(removed);
    assert_eq!(manager.get_tracks().len(), 1);

    // The remaining track should be video
    assert!(manager.get_tracks()[0].is_video());
}

// ============================================================================
// Track Backend Abstraction Tests
// ============================================================================

#[tokio::test]
async fn test_track_backend_polymorphism() {
    let transport = Arc::new(QuicMediaTransport::new());
    transport.connect(test_peer()).await.unwrap();

    // Create different backends
    let quic_backend: Arc<dyn TrackBackend> = Arc::new(QuicTrackBackend::new(
        Arc::clone(&transport),
        MediaType::Audio,
    ));

    // Use through trait interface
    assert!(quic_backend.is_connected());
    assert_eq!(quic_backend.backend_type(), "quic");

    // Send data
    let result = quic_backend.send(&[0x01, 0x02, 0x03]).await;
    assert!(result.is_ok());

    // Check stats
    let stats = quic_backend.stats();
    assert_eq!(stats.bytes_sent, 3);
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_send_fails_when_not_connected() {
    let transport = Arc::new(QuicMediaTransport::new());
    // Don't connect!

    let audio = AudioTrack::with_quic("audio-no-conn", transport);
    let result = audio.send_audio(&[0x01]).await;

    assert!(result.is_err());
}

#[test]
fn test_quic_track_creation_without_transport() {
    let mut manager = MediaStreamManager::new();

    // Should fail because no QUIC transport is configured
    let result = manager.create_quic_audio_track();
    assert!(result.is_err());
}

// ============================================================================
// Stream Lifecycle Integration Tests
// ============================================================================

#[tokio::test]
async fn test_stream_open_close_cycle() {
    let transport = Arc::new(QuicMediaTransport::new());
    transport.connect(test_peer()).await.unwrap();

    let backend = QuicTrackBackend::new(Arc::clone(&transport), MediaType::Audio);

    // Open stream
    backend.ensure_stream().await.unwrap();
    assert!(backend.is_stream_open().await);

    // Send data while open
    backend.send(&[0x01]).await.unwrap();

    // Close stream
    let closed = backend.close_stream().await;
    assert!(closed);
    assert!(!backend.is_stream_open().await);

    // Reopen and send again
    backend.ensure_stream().await.unwrap();
    backend.send(&[0x02]).await.unwrap();

    // Verify both sends are counted
    let stats = backend.stats();
    assert_eq!(stats.bytes_sent, 2);
    assert_eq!(stats.packets_sent, 2);
}

// ============================================================================
// Full Call Flow Integration
// ============================================================================

#[tokio::test]
async fn test_full_quic_media_flow() {
    // 1. Create transport
    let transport = Arc::new(QuicMediaTransport::new());

    // 2. Create manager with transport
    let mut manager = MediaStreamManager::with_quic_transport(Arc::clone(&transport));

    // 3. Connect transport
    transport.connect(test_peer()).await.unwrap();

    // 4. Create tracks (create both, then access via get_tracks)
    manager.create_quic_audio_track().unwrap();
    manager.create_quic_video_track(1280, 720).unwrap();

    // 5. Verify tracks are connected (transport is connected)
    let tracks = manager.get_tracks();
    assert_eq!(tracks.len(), 2);
    assert!(tracks[0].is_connected());
    assert!(tracks[1].is_connected());

    // 6. Send data through tracks
    tracks[0].send(&[0x78, 0x9c]).await.unwrap();
    tracks[1].send(&[0x00, 0x00, 0x00, 0x01]).await.unwrap();

    // 7. Remove audio track
    let audio_id = manager.get_tracks()[0].id().to_string();
    manager.remove_track(&audio_id);

    // 8. Only video remains
    assert_eq!(manager.get_tracks().len(), 1);
    assert!(manager.get_tracks()[0].is_video());
}
