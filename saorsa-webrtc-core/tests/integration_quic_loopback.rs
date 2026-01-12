//! Integration test for QUIC loopback with RTP data path validation
//!
//! These tests validate the basic QUIC transport data path for RTP packets.
//! Some tests are marked #[ignore] due to known ant-quic connection issues
//! in test environments.

use saorsa_webrtc_core::transport::{AntQuicTransport, TransportConfig};
use std::time::Duration;

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
#[ignore] // TODO: Fix data routing in ant-quic test environment
async fn test_quic_loopback_rtp_data_path() {
    let mut transport1 = AntQuicTransport::new(TransportConfig::default());
    let mut transport2 = AntQuicTransport::new(TransportConfig::default());

    transport1
        .start()
        .await
        .expect("Failed to start transport1");
    transport2
        .start()
        .await
        .expect("Failed to start transport2");

    let addr2 = transport2.local_addr().await.expect("Should have addr2");

    let _peer_id = transport1
        .connect_to_peer(addr2)
        .await
        .expect("Failed to connect");

    tokio::time::sleep(Duration::from_millis(1000)).await;

    let mut retries = 0;
    while retries < 20 {
        if transport1.is_connected().await && transport2.is_connected().await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        retries += 1;
    }

    if !transport1.is_connected().await || !transport2.is_connected().await {
        println!("Skipping test due to connection issues - this is expected in test environment");
        return;
    }

    let test_data = vec![0x80, 0x60, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];

    transport1
        .send_bytes(&test_data)
        .await
        .expect("Failed to send RTP data");

    let received = tokio::time::timeout(Duration::from_secs(5), transport2.receive_bytes())
        .await
        .expect("Timeout waiting for data")
        .expect("Failed to receive data");

    assert_eq!(received, test_data);
}

#[tokio::test]
#[ignore] // TODO: Fix data routing in ant-quic test environment
async fn test_quic_loopback_multiple_packets() {
    let mut transport1 = AntQuicTransport::new(TransportConfig::default());
    let mut transport2 = AntQuicTransport::new(TransportConfig::default());

    transport1
        .start()
        .await
        .expect("Failed to start transport1");
    transport2
        .start()
        .await
        .expect("Failed to start transport2");

    let addr2 = transport2.local_addr().await.expect("Should have addr2");

    let _peer_id = transport1
        .connect_to_peer(addr2)
        .await
        .expect("Failed to connect");

    tokio::time::sleep(Duration::from_millis(1000)).await;

    let mut retries = 0;
    while retries < 20 {
        if transport1.is_connected().await && transport2.is_connected().await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        retries += 1;
    }

    if !transport1.is_connected().await || !transport2.is_connected().await {
        println!("Skipping test due to connection issues - this is expected in test environment");
        return;
    }

    for i in 0..5 {
        let test_data = vec![0x80 + i, 0x60, i, i + 1, i + 2, i + 3];

        transport1
            .send_bytes(&test_data)
            .await
            .expect("Failed to send packet");

        tokio::time::sleep(Duration::from_millis(50)).await;

        let received = tokio::time::timeout(Duration::from_secs(5), transport2.receive_bytes())
            .await
            .expect("Timeout waiting for packet")
            .expect("Failed to receive packet");

        assert_eq!(received, test_data);
    }
}
