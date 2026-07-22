//! WP-V0.3 — two-REAL-nodes audio e2e (the proof this repo previously lacked).
//!
//! Two actual `ant_quic::Node`s on localhost (ephemeral UDP ports, real QUIC
//! sockets — no `MockDataPath`, no in-process mpsc):
//!
//! 1. Signaling handshake `CapabilityExchange → ConnectionConfirm →
//!    ConnectionReady` + `Bye` teardown over `SignalingTransport`.
//! 2. Path A — RTP-framed audio via `WebRtcQuicBridge` (tagged postcard
//!    packets over `send_bytes`/`receive_bytes`).
//! 3. Path B — raw audio frames via `LinkTransport::send`
//!    (`StreamType::Audio`, `[type][u16 BE len][payload]` framing), with a
//!    reply leg proving bidirectionality.
//!
//! Assertions per the revival design (docs/design/revival-v0-v1.md WP-V0.3):
//! complete (0 loss at this layer), byte-identical delivery keyed by
//! sequence, one-way p95 frame latency < 50 ms on loopback (generous bound;
//! catches pathological blocking), clean teardown.
//!
//! **Measured transport semantics (WP-V0.3 finding):** `ant_quic::Node`'s
//! message API (`send`/`recv`) is reliable *per message* but does **not**
//! guarantee cross-message ordering — each send rides its own uni-stream and
//! streams complete independently (observed empirically: strictly sequential
//! sends arrive reordered on loopback). These tests therefore assert
//! zero-loss + byte-identity keyed by embedded sequence number and *report*
//! reorder counts instead of asserting total order. Consumers needing order
//! must sequence at the application layer (RTP seq + jitter buffer —
//! WP-V1.3) or use a single long-lived stream (WP-V1.2 design input).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use saorsa_webrtc_core::link_transport::{LinkTransport, PeerConnection, StreamType};
use saorsa_webrtc_core::quic_bridge::{
    QuicBridgeConfig, RtpPacket, StreamType as BridgeStreamType, WebRtcQuicBridge,
};
use saorsa_webrtc_core::signaling::{SignalingMessage, SignalingTransport};
use saorsa_webrtc_core::transport::{AntQuicTransport, TransportConfig};

/// 5 s of 20 ms frames.
const FRAME_COUNT: usize = 250;
/// Synthetic 20 ms mono PCM-ish frame size (payload bytes on the wire).
const FRAME_BYTES: usize = 160;
/// Loopback one-way latency gate (p95). Generous; catches pathological
/// blocking, not jitter tuning.
const P95_LATENCY_BUDGET: Duration = Duration::from_millis(50);
/// Outer per-phase deadline so a wedged path fails crisply instead of
/// riding the transport's internal 30 s recv timeout repeatedly.
const PHASE_DEADLINE: Duration = Duration::from_secs(45);

/// Start a real ant-quic node bound to 127.0.0.1 on an ephemeral port.
async fn start_transport(label: &str) -> (AntQuicTransport, SocketAddr) {
    let mut t = AntQuicTransport::new(TransportConfig {
        local_addr: Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)),
    });
    t.start().await.expect("start transport");
    let addr = t.local_addr().await.expect("local addr");
    println!("[{label}] node up on {addr}");
    (t, addr)
}

/// Deterministic frame: [seq: u32 BE][elapsed_nanos_at_send: u64 BE][filler].
/// The timestamp field is patched at send time; byte-identity is asserted on
/// the full payload as sent (sender keeps copies).
fn make_frame(seq: u32, epoch: &Instant) -> Vec<u8> {
    let mut buf = Vec::with_capacity(FRAME_BYTES);
    buf.extend_from_slice(&seq.to_be_bytes());
    let nanos = u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
    buf.extend_from_slice(&nanos.to_be_bytes());
    // Deterministic filler derived from seq — a stub-detectable pattern.
    for n in buf.len()..FRAME_BYTES {
        buf.push(((seq.wrapping_mul(31).wrapping_add(n as u32)) % 251) as u8);
    }
    buf
}

fn frame_seq(payload: &[u8]) -> u32 {
    u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]])
}

fn frame_send_nanos(payload: &[u8]) -> u64 {
    u64::from_be_bytes([
        payload[4],
        payload[5],
        payload[6],
        payload[7],
        payload[8],
        payload[9],
        payload[10],
        payload[11],
    ])
}

fn one_way_latency(epoch: &Instant, payload: &[u8]) -> Duration {
    let now = u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
    Duration::from_nanos(now.saturating_sub(frame_send_nanos(payload)))
}

fn latency_report(label: &str, mut lat: Vec<Duration>) -> Duration {
    lat.sort_unstable();
    let p = |q: f64| lat[((lat.len() as f64 - 1.0) * q) as usize];
    let (p50, p95, max) = (p(0.50), p(0.95), *lat.last().expect("non-empty"));
    println!(
        "[{label}] frames={} one-way latency p50={p50:?} p95={p95:?} max={max:?}",
        lat.len()
    );
    p95
}

// ---------------------------------------------------------------------------
// 1. Signaling handshake over two real nodes
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signaling_handshake_and_bye_over_two_real_nodes() {
    let (mut a, _addr_a) = start_transport("sig-a").await;
    let (b, addr_b) = start_transport("sig-b").await;

    let peer_b = a.connect_to_peer(addr_b).await.expect("a connects to b");
    let session = "e2e-sig-session".to_string();

    // Responder task on B: expect CapabilityExchange, reply ConnectionConfirm,
    // expect ConnectionReady, expect Bye.
    let session_b = session.clone();
    let responder = tokio::spawn(async move {
        let (peer_a, msg) = b.receive_message().await.expect("b recv capex");
        match msg {
            SignalingMessage::CapabilityExchange {
                session_id, audio, ..
            } => {
                assert_eq!(session_id, session_b);
                assert!(audio, "caller must offer audio");
            }
            other => panic!("expected CapabilityExchange, got {other:?}"),
        }
        b.send_message(
            &peer_a,
            SignalingMessage::ConnectionConfirm {
                session_id: session_b.clone(),
                audio: true,
                video: false,
                data_channel: false,
                max_bandwidth_kbps: 64,
                quic_endpoint: None,
            },
        )
        .await
        .expect("b sends confirm");

        let (_, msg) = b.receive_message().await.expect("b recv ready");
        assert!(
            matches!(msg, SignalingMessage::ConnectionReady { ref session_id } if *session_id == session_b),
            "expected ConnectionReady, got {msg:?}"
        );

        let (_, msg) = b.receive_message().await.expect("b recv bye");
        assert!(
            matches!(msg, SignalingMessage::Bye { ref session_id, .. } if *session_id == session_b),
            "expected Bye, got {msg:?}"
        );
        b
    });

    a.send_message(
        &peer_b,
        SignalingMessage::CapabilityExchange {
            session_id: session.clone(),
            audio: true,
            video: false,
            data_channel: false,
            max_bandwidth_kbps: 64,
            quic_endpoint: None,
        },
    )
    .await
    .expect("a sends capex");

    let (_, confirm) = a.receive_message().await.expect("a recv confirm");
    assert!(
        matches!(confirm, SignalingMessage::ConnectionConfirm { ref session_id, audio: true, .. } if *session_id == session),
        "expected audio ConnectionConfirm, got {confirm:?}"
    );

    a.send_message(
        &peer_b,
        SignalingMessage::ConnectionReady {
            session_id: session.clone(),
        },
    )
    .await
    .expect("a sends ready");

    a.send_message(
        &peer_b,
        SignalingMessage::Bye {
            session_id: session.clone(),
            reason: Some("test complete".to_string()),
        },
    )
    .await
    .expect("a sends bye");

    let b = tokio::time::timeout(PHASE_DEADLINE, responder)
        .await
        .expect("responder within deadline")
        .expect("responder task");

    a.stop().expect("stop a");
    b.stop().expect("stop b");
    println!("[sig] CapabilityExchange → ConnectionConfirm → ConnectionReady → Bye: clean");
}

// ---------------------------------------------------------------------------
// 2. Path A — RTP framing via WebRtcQuicBridge
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rtp_bridge_audio_path_over_two_real_nodes() {
    let (mut a, _addr_a) = start_transport("rtp-a").await;
    let (b, addr_b) = start_transport("rtp-b").await;
    let _peer_b = a.connect_to_peer(addr_b).await.expect("a connects to b");

    let epoch = Instant::now();
    let bridge_a = WebRtcQuicBridge::with_transport(QuicBridgeConfig::default(), a);
    let bridge_b = WebRtcQuicBridge::with_transport(QuicBridgeConfig::default(), b);

    // Receiver first: collect FRAME_COUNT RTP packets.
    let epoch_rx = epoch;
    let receiver = tokio::spawn(async move {
        let mut packets: Vec<RtpPacket> = Vec::with_capacity(FRAME_COUNT);
        let mut latencies = Vec::with_capacity(FRAME_COUNT);
        while packets.len() < FRAME_COUNT {
            let pkt = bridge_b.receive_rtp_packet().await.expect("recv rtp");
            latencies.push(one_way_latency(&epoch_rx, &pkt.payload));
            packets.push(pkt);
        }
        (packets, latencies)
    });

    // Sender: FRAME_COUNT deterministic frames, keep byte copies.
    let mut sent_payloads = Vec::with_capacity(FRAME_COUNT);
    for seq in 0..FRAME_COUNT {
        let payload = make_frame(seq as u32, &epoch);
        let pkt = RtpPacket::new(
            96, // dynamic audio payload type
            seq as u16,
            (seq as u32) * 960, // 20 ms @ 48 kHz
            0xD00D_FEED,
            payload.clone(),
            BridgeStreamType::Audio,
        )
        .expect("rtp packet");
        bridge_a.send_rtp_packet(&pkt).await.expect("send rtp");
        sent_payloads.push(payload);
    }

    let (packets, latencies) = tokio::time::timeout(PHASE_DEADLINE, receiver)
        .await
        .expect("receiver within deadline")
        .expect("receiver task");

    // Complete + byte-identical, keyed by sequence (cross-message order is
    // not guaranteed by the transport — see module docs).
    assert_eq!(packets.len(), FRAME_COUNT, "zero loss");
    let mut slots: Vec<Option<&RtpPacket>> = vec![None; FRAME_COUNT];
    let mut reorders = 0usize;
    let mut max_seen: i64 = -1;
    for pkt in &packets {
        let seq = pkt.sequence_number as usize;
        assert!(seq < FRAME_COUNT, "sequence in range");
        assert!(slots[seq].is_none(), "no duplicate delivery for seq {seq}");
        assert_eq!(pkt.stream_type, BridgeStreamType::Audio);
        assert_eq!(frame_seq(&pkt.payload) as usize, seq);
        assert_eq!(pkt.payload, sent_payloads[seq], "byte-identical payload");
        if (seq as i64) < max_seen {
            reorders += 1;
        }
        max_seen = max_seen.max(seq as i64);
        slots[seq] = Some(pkt);
    }
    assert!(slots.iter().all(Option::is_some), "every frame delivered");
    println!("[rtp-bridge] reordered arrivals: {reorders}/{FRAME_COUNT}");
    let p95 = latency_report("rtp-bridge", latencies);
    assert!(
        p95 < P95_LATENCY_BUDGET,
        "p95 one-way latency {p95:?} exceeds {P95_LATENCY_BUDGET:?} on loopback"
    );
}

// ---------------------------------------------------------------------------
// 3. Path B — raw frames via LinkTransport (StreamType::Audio) + reply leg
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn link_transport_audio_path_over_two_real_nodes() {
    let (mut a, _addr_a) = start_transport("link-a").await;
    let (b, addr_b) = start_transport("link-b").await;

    let peer_b_id = a.connect_to_peer(addr_b).await.expect("a connects to b");
    let peer_b = PeerConnection {
        peer_id: peer_b_id,
        remote_addr: addr_b,
    };

    let epoch = Instant::now();
    const REPLY_COUNT: usize = 10;

    // Receiver on B: collect FRAME_COUNT audio frames, then send REPLY_COUNT
    // frames back to the sender it observed (bidirectionality proof).
    let epoch_rx = epoch;
    let receiver = tokio::spawn(async move {
        let mut got: Vec<Vec<u8>> = Vec::with_capacity(FRAME_COUNT);
        let mut latencies = Vec::with_capacity(FRAME_COUNT);
        let mut reply_to: Option<PeerConnection> = None;
        while got.len() < FRAME_COUNT {
            let (peer, stream_type, payload) = b.receive().await.expect("b receive");
            assert_eq!(stream_type, StreamType::Audio, "audio stream tag");
            latencies.push(one_way_latency(&epoch_rx, &payload));
            reply_to.get_or_insert(peer);
            got.push(payload);
        }
        let peer_a = reply_to.expect("observed sender peer");
        for seq in 0..REPLY_COUNT {
            let frame = make_frame((FRAME_COUNT + seq) as u32, &epoch_rx);
            b.send(&peer_a, StreamType::Audio, &frame)
                .await
                .expect("b replies");
        }
        (b, got, latencies)
    });

    // Sender on A.
    let mut sent: Vec<Vec<u8>> = Vec::with_capacity(FRAME_COUNT);
    for seq in 0..FRAME_COUNT {
        let frame = make_frame(seq as u32, &epoch);
        a.send(&peer_b, StreamType::Audio, &frame)
            .await
            .expect("a sends");
        sent.push(frame);
    }

    // Reply leg: A receives B's frames on its own link.
    let mut replies = Vec::with_capacity(REPLY_COUNT);
    while replies.len() < REPLY_COUNT {
        let (_, stream_type, payload) = tokio::time::timeout(PHASE_DEADLINE, a.receive())
            .await
            .expect("reply within deadline")
            .expect("a receives reply");
        assert_eq!(stream_type, StreamType::Audio);
        replies.push(payload);
    }

    let (b, got, latencies) = tokio::time::timeout(PHASE_DEADLINE, receiver)
        .await
        .expect("receiver within deadline")
        .expect("receiver task");

    assert_eq!(got.len(), FRAME_COUNT, "zero loss");
    let mut seen = vec![false; FRAME_COUNT];
    let mut reorders = 0usize;
    let mut max_seen: i64 = -1;
    for payload in &got {
        let seq = frame_seq(payload) as usize;
        assert!(seq < FRAME_COUNT, "sequence in range");
        assert!(!seen[seq], "no duplicate delivery for seq {seq}");
        assert_eq!(*payload, sent[seq], "byte-identical payload");
        if (seq as i64) < max_seen {
            reorders += 1;
        }
        max_seen = max_seen.max(seq as i64);
        seen[seq] = true;
    }
    assert!(seen.iter().all(|s| *s), "every frame delivered");
    println!("[link-transport] reordered arrivals: {reorders}/{FRAME_COUNT}");

    let mut reply_seen = [false; REPLY_COUNT];
    for payload in &replies {
        let seq = frame_seq(payload) as usize;
        assert!(
            (FRAME_COUNT..FRAME_COUNT + REPLY_COUNT).contains(&seq),
            "reply seq range"
        );
        assert!(!reply_seen[seq - FRAME_COUNT], "no duplicate reply");
        assert_eq!(payload.len(), FRAME_BYTES);
        reply_seen[seq - FRAME_COUNT] = true;
    }
    assert!(reply_seen.iter().all(|s| *s), "every reply delivered");
    let p95 = latency_report("link-transport", latencies);
    assert!(
        p95 < P95_LATENCY_BUDGET,
        "p95 one-way latency {p95:?} exceeds {P95_LATENCY_BUDGET:?} on loopback"
    );

    a.stop().expect("stop a");
    b.stop().expect("stop b");
}
