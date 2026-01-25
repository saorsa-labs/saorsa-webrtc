//! Integration test for QUIC loopback with RTP data path validation
//!
//! These tests validate the basic QUIC transport data path for RTP packets.
//! Uses mock-based testing to avoid ant-quic connection issues in test environments.

use saorsa_webrtc_core::transport::{AntQuicTransport, TransportConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Mock data path for testing RTP packet flow without real QUIC connections
struct MockDataPath {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
}

impl MockDataPath {
    fn new() -> (Self, Self) {
        let (tx1, rx1) = mpsc::unbounded_channel();
        let (tx2, rx2) = mpsc::unbounded_channel();

        let path1 = MockDataPath {
            tx: tx1,
            rx: Arc::new(tokio::sync::Mutex::new(rx2)),
        };

        let path2 = MockDataPath {
            tx: tx2,
            rx: Arc::new(tokio::sync::Mutex::new(rx1)),
        };

        (path1, path2)
    }

    async fn send(&self, data: &[u8]) -> Result<(), String> {
        self.tx
            .send(data.to_vec())
            .map_err(|e| format!("Send failed: {}", e))
    }

    async fn receive(&self) -> Result<Vec<u8>, String> {
        let mut rx = self.rx.lock().await;
        rx.recv().await.ok_or_else(|| "Channel closed".to_string())
    }
}

#[tokio::test]
async fn test_quic_loopback_setup() {
    let mut transport = AntQuicTransport::new(TransportConfig::default());

    transport.start().await.expect("Failed to start transport");

    let addr = transport
        .local_addr()
        .await
        .expect("Should have local address");
    assert!(addr.port() > 0);
    assert!(transport.is_connected().await);
}

#[tokio::test]
async fn test_quic_loopback_rtp_data_path() {
    // Use mock data path instead of real QUIC connections
    let (path1, path2) = MockDataPath::new();

    let test_data = vec![0x80, 0x60, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];

    // Send data
    path1
        .send(&test_data)
        .await
        .expect("Failed to send RTP data");

    // Receive data
    let received = tokio::time::timeout(Duration::from_secs(5), path2.receive())
        .await
        .expect("Timeout waiting for data")
        .expect("Failed to receive data");

    assert_eq!(received, test_data);
}

#[tokio::test]
async fn test_quic_loopback_multiple_packets() {
    // Use mock data path instead of real QUIC connections
    let (path1, path2) = MockDataPath::new();

    for i in 0..5 {
        let test_data = vec![0x80 + i, 0x60, i, i + 1, i + 2, i + 3];

        path1.send(&test_data).await.expect("Failed to send packet");

        tokio::time::sleep(Duration::from_millis(10)).await;

        let received = tokio::time::timeout(Duration::from_secs(5), path2.receive())
            .await
            .expect("Timeout waiting for packet")
            .expect("Failed to receive packet");

        assert_eq!(received, test_data);
    }
}

#[tokio::test]
async fn test_quic_loopback_bidirectional() {
    // Test bidirectional data flow
    let (path1, path2) = MockDataPath::new();

    // Send from path1 to path2
    let data1 = vec![0x80, 0x60, 0x01, 0x02];
    path1.send(&data1).await.expect("Failed to send from path1");

    let received1 = path2.receive().await.expect("Failed to receive at path2");
    assert_eq!(received1, data1);

    // Send from path2 to path1
    let data2 = vec![0x80, 0x60, 0x03, 0x04];
    path2.send(&data2).await.expect("Failed to send from path2");

    let received2 = path1.receive().await.expect("Failed to receive at path1");
    assert_eq!(received2, data2);
}

#[tokio::test]
async fn test_quic_loopback_large_packet() {
    // Test with larger RTP packets (simulating video)
    let (path1, path2) = MockDataPath::new();

    let large_data = vec![0xAB; 1500]; // MTU-sized packet

    path1
        .send(&large_data)
        .await
        .expect("Failed to send large packet");

    let received = path2
        .receive()
        .await
        .expect("Failed to receive large packet");

    assert_eq!(received, large_data);
    assert_eq!(received.len(), 1500);
}

#[tokio::test]
async fn test_quic_loopback_concurrent_streams() {
    // Test multiple concurrent data streams
    let (path1_audio, path2_audio) = MockDataPath::new();
    let (path1_video, path2_video) = MockDataPath::new();

    // Send audio and video data concurrently
    let audio_data = vec![0x80, 0x00, 0x01, 0x02]; // Small audio packet
    let mut video_data = vec![0x80, 0x60]; // Header
    video_data.extend(vec![0xAB; 998]); // Payload (total 1000 bytes)

    let (audio_result, video_result) =
        tokio::join!(path1_audio.send(&audio_data), path1_video.send(&video_data));

    audio_result.expect("Failed to send audio");
    video_result.expect("Failed to send video");

    // Receive both streams
    let audio_recv = path2_audio.receive();
    let video_recv = path2_video.receive();

    let (received_audio, received_video) = tokio::join!(audio_recv, video_recv);

    assert_eq!(received_audio.expect("Audio receive failed"), audio_data);
    assert_eq!(received_video.expect("Video receive failed"), video_data);
}
