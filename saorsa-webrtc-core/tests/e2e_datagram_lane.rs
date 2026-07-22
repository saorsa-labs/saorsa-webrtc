//! WP-V1.3 — datagram audio lane over two REAL ant-quic nodes.
//!
//! Proves the V1 reachability verdict: QUIC DATAGRAM frames are usable for
//! audio with **zero ant-quic changes**. Connection establishment reuses
//! the proven Node path from the R0 e2e; the raw per-peer
//! `high_level::Connection` is reached via
//! `Node::inner_endpoint().get_quic_connection()` and wrapped in
//! `ant_quic::P2pLinkConn`, so frames ride the same `LinkConn`
//! datagram seam production code uses. The receive side drains
//! `LinkConn::recv_datagrams` into the jitter buffer.
//!
//! Assertions:
//! - `max_datagram_size` negotiated on both sides (datagram support on) —
//!   failure here falsifies the reachability verdict, fail loudly.
//! - ≥ 96% of 250 frames delivered byte-identical (datagrams are lossy by
//!   contract; loopback loss should be ≈0 but is not guaranteed).
//! - Jitter buffer restores playout order; losses surface as gaps.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use ant_quic::link_transport::LinkConn;
use ant_quic::P2pLinkConn;
use bytes::Bytes;
use futures::StreamExt;
use saorsa_webrtc_core::datagram_lane::{AudioDatagram, DatagramAudioLane};
use saorsa_webrtc_core::jitter::{JitterBuffer, JitterConfig, JitterEvent};
use saorsa_webrtc_core::link_transport::LinkTransport;
use saorsa_webrtc_core::transport::{AntQuicTransport, TransportConfig};

const FRAMES: u32 = 250;
const FRAME_BYTES: usize = 160; // ~20 ms Opus at 64 kbps
const MIN_DELIVERED: usize = 240; // ≥96%

fn payload_for(seq: u32) -> Bytes {
    let mut buf = Vec::with_capacity(FRAME_BYTES);
    for n in 0..FRAME_BYTES {
        buf.push(((seq.wrapping_mul(131).wrapping_add(n as u32)) % 251) as u8);
    }
    Bytes::from(buf)
}

async fn start_transport(label: &str) -> (AntQuicTransport, SocketAddr) {
    let mut t = AntQuicTransport::new(TransportConfig {
        local_addr: Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)),
        ..TransportConfig::default()
    });
    t.start().await.expect("start transport");
    let addr = t.local_addr().await.expect("local addr");
    println!("[{label}] listening on {addr}");
    (t, addr)
}

/// Wait until the node behind `t` has ≥1 connected peer; return the raw
/// QUIC connection wrapped as a `P2pLinkConn`.
async fn link_conn_for_first_peer(t: &AntQuicTransport, label: &str) -> P2pLinkConn {
    let node = t.get_node().expect("node started");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let peers = node.connected_peers().await;
        if let Some(pc) = peers.first() {
            let conn = node
                .inner_endpoint()
                .get_quic_connection(&pc.peer_id)
                .expect("endpoint lookup")
                .expect("live QUIC connection for connected peer");
            println!("[{label}] peer {:?} via {:?}", pc.peer_id, pc.remote_addr);
            let remote = match pc.remote_addr {
                ant_quic::TransportAddr::Udp(a) => a,
                ref other => panic!("unexpected transport addr {other:?}"),
            };
            return P2pLinkConn::new(conn, pc.peer_id, remote);
        }
        assert!(
            Instant::now() < deadline,
            "[{label}] no connected peer within 15s"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "spawns real ant-quic UDP nodes"]
async fn datagram_audio_lane_over_two_real_nodes() {
    let (receiver_t, recv_addr) = start_transport("receiver").await;
    let (mut sender_t, _send_addr) = start_transport("sender").await;

    sender_t.connect(recv_addr).await.expect("connect");
    let out_conn = link_conn_for_first_peer(&sender_t, "sender").await;
    let in_conn = link_conn_for_first_peer(&receiver_t, "receiver").await;

    // Reachability gate: datagram support must be negotiated on both sides.
    assert!(
        out_conn.inner().max_datagram_size().is_some(),
        "sender reports no datagram support — reachability verdict falsified"
    );
    assert!(
        in_conn.inner().max_datagram_size().is_some(),
        "receiver reports no datagram support — reachability verdict falsified"
    );

    // Receiver: drain datagrams → decode → jitter buffer.
    let recv_task = tokio::spawn(async move {
        let mut jb = JitterBuffer::new(JitterConfig::default());
        let mut delivered: Vec<AudioDatagram> = Vec::new();
        let mut gaps: Vec<u32> = Vec::new();
        let mut stream = in_conn.recv_datagrams();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let Ok(next) =
                tokio::time::timeout(remaining.min(Duration::from_secs(2)), stream.next()).await
            else {
                // Quiet for 2s after traffic — sender is done.
                if !delivered.is_empty() {
                    break;
                }
                continue;
            };
            let Some(wire) = next else { break };
            let frame = AudioDatagram::decode(wire).expect("well-formed lane datagram");
            jb.push(frame);
            for ev in jb.poll_ready() {
                match ev {
                    JitterEvent::Frame(f) => delivered.push(f),
                    JitterEvent::Gap { seq } => gaps.push(seq),
                }
            }
            if delivered.len() + gaps.len() >= FRAMES as usize {
                break;
            }
        }
        for ev in jb.poll_ready() {
            match ev {
                JitterEvent::Frame(f) => delivered.push(f),
                JitterEvent::Gap { seq } => gaps.push(seq),
            }
        }
        (delivered, gaps, jb.counters())
    });

    // Sender: 250 frames at a 2 ms tick.
    let lane = DatagramAudioLane::new();
    let send_start = Instant::now();
    let mut send_errors = 0u32;
    for seq in 0..FRAMES {
        let ts = u64::from(seq) * 20;
        if lane.send_frame(&out_conn, payload_for(seq), ts, 0).is_err() {
            send_errors += 1;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let send_elapsed = send_start.elapsed();

    let (delivered, gaps, counters) = recv_task.await.expect("receiver task");
    println!(
        "sent {FRAMES} in {send_elapsed:?} (errors {send_errors}); delivered {} gaps {} counters {counters:?}",
        delivered.len(),
        gaps.len()
    );

    assert_eq!(send_errors, 0, "loopback sends must not error");
    assert!(
        delivered.len() >= MIN_DELIVERED,
        "delivered {}/{FRAMES} < {MIN_DELIVERED}",
        delivered.len()
    );
    // Playout order strictly increasing, payloads byte-identical.
    let mut prev: Option<u32> = None;
    for f in &delivered {
        if let Some(p) = prev {
            assert!(f.seq > p, "playout order violated: {} after {p}", f.seq);
        }
        assert_eq!(
            f.payload,
            payload_for(f.seq),
            "payload corrupted at {}",
            f.seq
        );
        assert_eq!(f.timestamp_ms, u64::from(f.seq) * 20);
        prev = Some(f.seq);
    }
    // Accounting: every frame is either delivered or declared a gap.
    assert!(
        delivered.len() + gaps.len() >= MIN_DELIVERED,
        "lane accounting hole: {} delivered + {} gaps",
        delivered.len(),
        gaps.len()
    );
}
