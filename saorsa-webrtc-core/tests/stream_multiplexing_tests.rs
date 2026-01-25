//! Stream Multiplexing Tests
//!
//! Tests for stream type mapping, concurrent stream handling, and priority ordering.

use saorsa_webrtc_core::link_transport::{PeerConnection, StreamType};
use saorsa_webrtc_core::quic_bridge::{stream_tags, RtpPacket, StreamType as BridgeStreamType};
use saorsa_webrtc_core::quic_media_transport::{QuicMediaTransport, StreamPriority};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

fn test_peer() -> PeerConnection {
    PeerConnection {
        peer_id: "multiplex-test-peer".to_string(),
        remote_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9000),
    }
}

// ============================================================================
// Stream Type to Stream ID Mapping Tests
// ============================================================================

#[test]
fn stream_type_to_tag_mapping() {
    // Verify the stream type tags are unique and correctly defined
    assert_eq!(stream_tags::AUDIO, 0x21);
    assert_eq!(stream_tags::VIDEO, 0x22);
    assert_eq!(stream_tags::SCREEN_SHARE, 0x23);
    assert_eq!(stream_tags::RTCP_FEEDBACK, 0x24);
    assert_eq!(stream_tags::DATA, 0x25);

    // Verify all tags are distinct
    let tags = [
        stream_tags::AUDIO,
        stream_tags::VIDEO,
        stream_tags::SCREEN_SHARE,
        stream_tags::RTCP_FEEDBACK,
        stream_tags::DATA,
    ];

    for (i, tag1) in tags.iter().enumerate() {
        for (j, tag2) in tags.iter().enumerate() {
            if i != j {
                assert_ne!(tag1, tag2, "Stream tags must be unique");
            }
        }
    }
}

#[test]
fn bridge_stream_type_to_tag_roundtrip() {
    let types = [
        BridgeStreamType::Audio,
        BridgeStreamType::Video,
        BridgeStreamType::ScreenShare,
        BridgeStreamType::RtcpFeedback,
        BridgeStreamType::Data,
    ];

    for stream_type in types {
        let tag = stream_type.to_tag();
        let restored = BridgeStreamType::from_tag(tag);
        assert_eq!(
            restored,
            Some(stream_type),
            "Roundtrip failed for {:?}",
            stream_type
        );
    }
}

#[test]
fn invalid_stream_tag_returns_none() {
    // Tags outside the valid range should return None
    assert_eq!(BridgeStreamType::from_tag(0x00), None);
    assert_eq!(BridgeStreamType::from_tag(0x20), None); // Just below range
    assert_eq!(BridgeStreamType::from_tag(0x26), None); // Just above range
    assert_eq!(BridgeStreamType::from_tag(0xFF), None);
}

// ============================================================================
// Concurrent Stream Tests
// ============================================================================

#[tokio::test]
async fn audio_video_screen_concurrent_streams() {
    let transport = QuicMediaTransport::new();
    transport.connect(test_peer()).await.unwrap();

    // Open all three media streams concurrently
    transport.open_stream(StreamType::Audio).await.unwrap();
    transport.open_stream(StreamType::Video).await.unwrap();
    transport.open_stream(StreamType::Screen).await.unwrap();

    // Verify all are open
    let active = transport.active_streams().await;
    assert_eq!(active.len(), 3);

    // Verify each type is present
    let types = transport.open_stream_types().await;
    assert!(types.contains(&StreamType::Audio));
    assert!(types.contains(&StreamType::Video));
    assert!(types.contains(&StreamType::Screen));
}

#[tokio::test]
async fn all_five_stream_types_concurrent() {
    let transport = QuicMediaTransport::new();
    transport.connect(test_peer()).await.unwrap();

    // Open all stream types
    transport.open_all_streams().await.unwrap();

    // Verify all five are open
    let active = transport.active_streams().await;
    assert_eq!(active.len(), 5);

    // Verify each type
    let types = transport.open_stream_types().await;
    assert!(types.contains(&StreamType::Audio));
    assert!(types.contains(&StreamType::Video));
    assert!(types.contains(&StreamType::Screen));
    assert!(types.contains(&StreamType::RtcpFeedback));
    assert!(types.contains(&StreamType::Data));
}

#[tokio::test]
async fn send_on_multiple_streams_concurrently() {
    let transport = QuicMediaTransport::new();
    transport.connect(test_peer()).await.unwrap();

    // Send on different streams
    transport.send_audio(&[0x80, 0x0E]).await.unwrap();
    transport.send_video(&[0x80, 0x60]).await.unwrap();
    transport.send_screen(&[0x80, 0x65]).await.unwrap();
    transport.send_rtcp(&[0x80, 0xC8]).await.unwrap();
    transport.send_data(&[0x01, 0x02]).await.unwrap();

    // Verify stats
    let stats = transport.stats().await;
    assert_eq!(stats.packets_sent, 5);
}

// ============================================================================
// Stream Priority Ordering Tests
// ============================================================================

#[test]
fn stream_priority_ordering_audio_highest() {
    // Audio should have highest priority (lowest numeric value)
    assert!(StreamPriority::from(StreamType::Audio) < StreamPriority::from(StreamType::Video));
    assert!(StreamPriority::from(StreamType::Audio) < StreamPriority::from(StreamType::Screen));
    assert!(StreamPriority::from(StreamType::Audio) < StreamPriority::from(StreamType::Data));
}

#[test]
fn stream_priority_ordering_video_medium() {
    // Video should have medium priority
    assert!(StreamPriority::from(StreamType::Video) > StreamPriority::from(StreamType::Audio));
    assert!(StreamPriority::from(StreamType::Video) < StreamPriority::from(StreamType::Screen));
    assert!(StreamPriority::from(StreamType::Video) < StreamPriority::from(StreamType::Data));
}

#[test]
fn stream_priority_ordering_data_lowest() {
    // Data should have lowest priority
    assert!(StreamPriority::from(StreamType::Data) > StreamPriority::from(StreamType::Audio));
    assert!(StreamPriority::from(StreamType::Data) > StreamPriority::from(StreamType::Video));
    // Screen and Data have same priority (Low)
    assert_eq!(
        StreamPriority::from(StreamType::Data),
        StreamPriority::from(StreamType::Screen)
    );
}

#[test]
fn rtcp_priority_equals_audio() {
    // RTCP feedback should have same priority as audio (critical for QoS)
    assert_eq!(
        StreamPriority::from(StreamType::RtcpFeedback),
        StreamPriority::from(StreamType::Audio)
    );
    assert_eq!(
        StreamPriority::from(StreamType::RtcpFeedback),
        StreamPriority::High
    );
}

#[tokio::test]
async fn priority_sorting_with_active_streams() {
    let transport = QuicMediaTransport::new();
    transport.connect(test_peer()).await.unwrap();

    // Open streams in non-priority order
    transport.open_stream(StreamType::Data).await.unwrap();
    transport.open_stream(StreamType::Video).await.unwrap();
    transport.open_stream(StreamType::Audio).await.unwrap();

    // Get priorities - should be sorted
    let priorities = transport.stream_priorities().await;

    // First should be Audio (High), then Video (Medium), then Data (Low)
    assert_eq!(priorities[0].0, StreamType::Audio);
    assert_eq!(priorities[0].1, StreamPriority::High);

    assert_eq!(priorities[1].0, StreamType::Video);
    assert_eq!(priorities[1].1, StreamPriority::Medium);

    assert_eq!(priorities[2].0, StreamType::Data);
    assert_eq!(priorities[2].1, StreamPriority::Low);
}

// ============================================================================
// RTCP Stream Separate from Media Tests
// ============================================================================

#[tokio::test]
async fn rtcp_stream_separate_from_audio() {
    let transport = QuicMediaTransport::new();
    transport.connect(test_peer()).await.unwrap();

    // Open both audio and RTCP streams
    transport.open_stream(StreamType::Audio).await.unwrap();
    transport
        .open_stream(StreamType::RtcpFeedback)
        .await
        .unwrap();

    // Verify both are distinct streams
    let active = transport.active_streams().await;
    assert_eq!(active.len(), 2);

    // Send on each
    transport.send_audio(&[0x80, 0x0E]).await.unwrap();
    transport.send_rtcp(&[0x80, 0xC9]).await.unwrap();

    // Verify both have data
    let stats = transport.stats().await;
    assert_eq!(stats.packets_sent, 2);
}

#[test]
fn rtcp_has_distinct_stream_tag() {
    // RTCP uses a different tag than Audio even though same priority
    assert_ne!(stream_tags::AUDIO, stream_tags::RTCP_FEEDBACK);
    assert_eq!(stream_tags::AUDIO, 0x21);
    assert_eq!(stream_tags::RTCP_FEEDBACK, 0x24);
}

// ============================================================================
// Data Channel Stream Isolation Tests
// ============================================================================

#[tokio::test]
async fn data_channel_stream_isolation() {
    let transport = QuicMediaTransport::new();
    transport.connect(test_peer()).await.unwrap();

    // Open data channel and media streams
    transport.open_stream(StreamType::Data).await.unwrap();
    transport.open_stream(StreamType::Audio).await.unwrap();
    transport.open_stream(StreamType::Video).await.unwrap();

    // Verify data channel is separate
    let types = transport.open_stream_types().await;
    assert!(types.contains(&StreamType::Data));
    assert!(types.contains(&StreamType::Audio));
    assert!(types.contains(&StreamType::Video));

    // Send data on each
    transport.send_data(&[0x01, 0x02, 0x03]).await.unwrap();
    transport.send_audio(&[0x80, 0x0E]).await.unwrap();
    transport.send_video(&[0x80, 0x60]).await.unwrap();

    // Each stream has independent stats
    let stats = transport.stats().await;
    assert_eq!(stats.packets_sent, 3);
}

#[test]
fn data_has_lowest_priority() {
    // Data channel should not interfere with real-time media
    assert!(StreamPriority::from(StreamType::Data) > StreamPriority::from(StreamType::Audio));
    assert!(StreamPriority::from(StreamType::Data) > StreamPriority::from(StreamType::Video));
    assert!(StreamPriority::from(StreamType::Data) >= StreamPriority::from(StreamType::Screen));
}

// ============================================================================
// RTP Packet with Stream Type Tag Tests
// ============================================================================

#[test]
fn rtp_packet_preserves_stream_type() {
    let stream_types = [
        BridgeStreamType::Audio,
        BridgeStreamType::Video,
        BridgeStreamType::ScreenShare,
        BridgeStreamType::Data,
    ];

    for stream_type in stream_types {
        let packet = RtpPacket::new(
            96,
            1000,
            12345,
            0xDEADBEEF,
            vec![0x01, 0x02, 0x03],
            stream_type,
        )
        .unwrap();

        let tagged = packet.to_tagged_bytes().unwrap();
        let restored = RtpPacket::from_tagged_bytes(&tagged).unwrap();

        assert_eq!(restored.stream_type, stream_type);
    }
}

#[test]
fn tagged_packet_first_byte_is_stream_type() {
    let packet = RtpPacket::new(
        96,
        1000,
        12345,
        0xDEADBEEF,
        vec![0x01, 0x02],
        BridgeStreamType::Video,
    )
    .unwrap();

    let tagged = packet.to_tagged_bytes().unwrap();

    // First byte should be the video stream type tag
    assert_eq!(tagged[0], stream_tags::VIDEO);
}

// ============================================================================
// Stream Type Real-Time Classification Tests
// ============================================================================

#[test]
fn realtime_stream_classification() {
    // Audio, Video, ScreenShare, and RTCP are real-time
    assert!(BridgeStreamType::Audio.is_realtime());
    assert!(BridgeStreamType::Video.is_realtime());
    assert!(BridgeStreamType::ScreenShare.is_realtime());
    assert!(BridgeStreamType::RtcpFeedback.is_realtime());

    // Data is not real-time
    assert!(!BridgeStreamType::Data.is_realtime());
}

// ============================================================================
// Stream Open/Close Lifecycle Tests
// ============================================================================

#[tokio::test]
async fn stream_open_close_reopen_cycle() {
    let transport = QuicMediaTransport::new();
    transport.connect(test_peer()).await.unwrap();

    // Open stream
    transport.open_stream(StreamType::Audio).await.unwrap();
    assert_eq!(transport.open_stream_count().await, 1);

    // Close stream
    transport.close_stream(StreamType::Audio).await;
    assert_eq!(transport.open_stream_count().await, 0);

    // Reopen should work
    transport.reopen_stream(StreamType::Audio).await.unwrap();
    assert_eq!(transport.open_stream_count().await, 1);
}

#[tokio::test]
async fn close_specific_stream_leaves_others_open() {
    let transport = QuicMediaTransport::new();
    transport.connect(test_peer()).await.unwrap();

    // Open all streams
    transport.open_all_streams().await.unwrap();
    assert_eq!(transport.open_stream_count().await, 5);

    // Close just video
    transport.close_stream(StreamType::Video).await;
    assert_eq!(transport.open_stream_count().await, 4);

    // Verify video is closed but others remain
    let types = transport.open_stream_types().await;
    assert!(!types.contains(&StreamType::Video));
    assert!(types.contains(&StreamType::Audio));
    assert!(types.contains(&StreamType::Screen));
    assert!(types.contains(&StreamType::RtcpFeedback));
    assert!(types.contains(&StreamType::Data));
}
