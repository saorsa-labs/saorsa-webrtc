# saorsa-webrtc revival — V0/V1 implementation design

**Status:** Design for implementation (team: design/impl/test — David, 2026-07-22: voice is essential)
**Date:** 2026-07-22
**Consumers:** tic-tac-toe (1:1 calls, V2 of its plan), x0x (transport substrate)
**Companion:** `tic-tac-toe/docs/design/voice-over-x0x.md` (product decision: Buzz huddle cut; voice ships P2P on this crate)

## 1. Context

The 2026-07-22 audit found: right architecture (QUIC-native media over
ant-quic, trait-pluggable signaling), wrong reality (stub Opus, mock-only
transport tests, ant-quic 0.20 pin, no group layer, aspirational status
docs). This design turns the audit into workpackages. Scope fence: **1:1
voice only** — group calls (mesh ≤4) are V2, explicitly out of V0/V1.
Video stays feature-gated and untouched.

## 2. Verified current state (2026-07-22, code-checked)

| Area | Fact | Evidence |
|---|---|---|
| Build | Workspace builds clean on ant-quic 0.20.3 | `cargo check --workspace` 1m07s, warnings only |
| ant_quic:: touchpoints | **2 files only**: `transport.rs` (Node, NodeConfigBuilder, PeerId, accept→PeerConnection{peer_id, remote_addr}) and `protocol_handler.rs` (LinkError, LinkResult, PeerId, ProtocolHandler, StreamType) | grep inventory |
| ant-quic 0.27.34 still exports | `Node/NodeError`, `NodeConfig(Builder)` (`bind_addr(impl Into<TransportAddr>)`), `link_transport` + `link_transport_impl` (ProtocolHandler family), high_level streams | `../ant-quic/src/lib.rs:299-396`, `node_config.rs` |
| **Datagrams: public API already exists** | `high_level::Connection`: `send_datagram`, `send_datagram_wait`, `read_datagram`, `max_datagram_size`, `datagram_drop_stats`, `on_datagram_drop` — all pub, exported via `lib.rs:299` | `../ant-quic/src/high_level/connection.rs:480-712` |
| Datagram gap | `Node`/`PeerConnection` do **not** surface the per-peer `high_level::Connection`; no datagram passthrough on the Node API | `../ant-quic/src/node.rs` pub fn inventory |
| Quick path-override check | Building ant-quic 0.27.34 inside this workspace fails **before any API diff** on dep-tree resolution (`MLKEM1024` missing in the resolved `rustls`/`aws-lc-rs`) — the upgrade starts as a dependency-tree reconciliation (rustls, aws-lc-rs, saorsa-pqc), not an API chase | path-override cargo check |
| Opus | Stub self-labeled "Not suitable for production"; real `opus = "0.3"` dep exists behind unwired feature `opus = ["dep:opus"]` | `codecs/src/opus.rs:1-11`, `codecs/Cargo.toml` |
| Signaling seam | `SignalingTransport` trait (assoc PeerId/Error; send/receive/discover/get_connection_handle); QUIC-native flow `CapabilityExchange → ConnectionConfirm → ConnectionReady` (+`Bye`), all serde | `core/src/signaling.rs:46-195` |
| Media seam | `LinkTransport` trait (start/stop/connect/accept/send/receive keyed by `StreamType` 0x20–0x24) | `core/src/link_transport.rs` |
| x0x stream seam | ADR-0022: `StreamProtocol` prefix bytes 0x01–0x03 taken (Forward/Socks); `register_stream_acceptor`/`open_peer_stream`; `PeerStream` wraps `HighLevelSendStream/RecvStream` | `../x0x/src/streams.rs:355-491` |
| No collision | webrtc StreamType (0x20–0x24) vs x0x StreamProtocol (0x01–0x03) — disjoint; nesting design chosen in WP-V1.2 | both enums |

## 3. V0 — revive (prove the crate does what it claims)

### WP-V0.1 — dependency + ant-quic 0.27.34 upgrade
**Scope:** reconcile the dep tree first (rustls / aws-lc-rs / saorsa-pqc to
ant-quic 0.27.34's versions — the path-override check proves this is the
first blocker), then fix the two ant_quic:: files: `TransportAddr`
adoption in `transport.rs` config/bind paths; re-check `ProtocolHandler`
trait signature drift in `protocol_handler.rs`; keypair types from the
matching saorsa-pqc. Copy current-API usage patterns from
`../x0x/src/network.rs` (same version). Kill the workspace-vs-crate
version inconsistency (workspace 0.21 / crate 0.20): single workspace dep.
**Files:** `Cargo.toml` (workspace + core), `core/src/transport.rs`,
`core/src/protocol_handler.rs`.
**Acceptance:** `cargo check --workspace` + full test suite green on
0.27.34; `cargo tree -i ant-quic` shows exactly one version.

### WP-V0.2 — real Opus
**Scope:** make `opus` a **default** feature of saorsa-webrtc-codecs; wire
`opus::Encoder/Decoder` into `OpusEncoder::encode` / `OpusDecoder::decode`
(48 kHz/mono default, 20 ms frames, bitrate from config); the stub moves
behind `#[cfg(any(test, feature = "stub-codecs"))]` with its lying
doc-comment deleted; `AudioFrame` stays as the PCM interface.
**Files:** `codecs/src/opus.rs`, `codecs/Cargo.toml`.
**Acceptance:** interop test `codecs/tests/opus_interop.rs`: (a) our
encoder → raw `opus` crate decoder → PCM energy/shape assertion on a 440 Hz
tone; (b) round-trip PSNR sanity; (c) encoded 20 ms frame ≤ ~200 bytes at
64 kbps (i.e. actually compressed — the stub fails this).

### WP-V0.3 — two-real-nodes e2e (the missing proof)
**Scope:** the test the repo claims but lacks: two real ant-quic `Node`s
on localhost, `AntQuicTransport` signaling (CapabilityExchange→…→Ready),
then audio over `QuicMediaTransport`: (a) RTP-framed path via
`WebRtcQuicBridge`, (b) raw-Opus-frame path via `LinkTransport::send`
(`StreamType::Audio`). Assert: delivery order, 5 s of 20 ms frames with
zero loss on loopback, one-way latency p95 < 20 ms on loopback, clean
teardown (`Bye`).
**Files:** `core/tests/e2e_two_nodes_audio.rs` (real, not `MockDataPath`);
keep the mock tests but rename them honestly (`mock_` prefix).
**Acceptance:** test green in CI on macOS + Linux; the words
"loopback"/"validated" in docs point at THIS file.

### WP-V0.4 — truth pass on status docs
**Scope:** `FINAL_COMPLETION_SUMMARY.md` and `IMPLEMENTATION_STATUS.md`
replaced by a single `STATUS.md` generated from what V0 actually proves;
delete "Complete 🎉" claims. Justfile with the standard recipes (org
policy). CI: fmt/clippy -D warnings/nextest on PR.
**Acceptance:** no doc claims a capability without naming its test.

## 4. V1 — x0x integration (voice rides the x0x mesh)

### WP-V1.1 — `X0xSignaling` adapter (home: **x0x repo**, `x0x::voice` module)
**Decision + rationale:** the adapter lives in **x0x** (not a saorsa-webrtc
feature): it depends on x0x's `Agent` (send_direct/subscribe_direct),
AgentId, and trust gates — saorsa-webrtc must not depend on x0x (dependency
direction: app → x0x → ant-quic; saorsa-webrtc parallel to x0x). x0x gains
an optional `voice` cargo feature depending on `saorsa-webrtc-core`.
**Scope:** `impl SignalingTransport for X0xSignaling` — `type PeerId =
AgentId` (has Display/FromStr via hex), `type Error = NetworkError`;
`send_message` = serde_json (postcard later) `SignalingMessage` in a typed
DM payload (`x0x-webrtc-sig-v1` prefix, Ephemeral class in the ADR-0023
taxonomy — signaling is control traffic, never history); `receive_message`
= background `subscribe_direct` reader → mpsc, filtering the prefix;
`discover_peer_endpoint` → `Ok(None)` (QUIC-native path needs none).
Flow: 3 signaling DMs ≈ 1.5 RTT before media.
**Acceptance:** unit round-trip over two in-process agents; deny-test that
signaling payloads never reach the history store.

### WP-V1.2 — `LinkTransport` over x0x ADR-0022 streams
**Scope:** new x0x `StreamProtocol::WebRtcV1 = 0x04` (one prefix byte —
keeps the connect-ACL/attestation model per ADR-0022; inner byte = webrtc
`StreamType` 0x20–0x24). `X0xLinkTransport` wraps
`register_stream_acceptor(WebRtcV1)` + `open_peer_stream`, implements
`LinkTransport` for the **reliable lanes** (RtcpFeedback, Data, control);
Audio uses WP-V1.3's datagram lane when available, falling back to the
reliable Audio stream when not.
**Acceptance:** WP-V0.3's e2e re-run with `X0xLinkTransport` between two
x0xd-backed agents (localhost), connect-ACL enforced (unlisted peer → gate
rejection test).

### WP-V1.3 — datagram audio lane (upstream ant-quic ask, precisely scoped)
**Finding:** the datagram machinery is already public on
`high_level::Connection` (`send_datagram` / `read_datagram` /
`max_datagram_size` / drop-stats, `connection.rs:480-712`); what's missing
is only **plumbing from `Node`/`PeerConnection` to the peer's
`high_level::Connection`**.
**Upstream ask (exact surface):** on `ant_quic::Node`:
`pub async fn send_datagram(&self, peer: &PeerId, data: Bytes) ->
Result<(), NodeError>` and `pub fn subscribe_datagrams(&self, peer:
&PeerId) -> Result<DatagramReceiver, NodeError>` (thin delegates to the
existing high_level API), plus `max_datagram_size(peer)`. First
implementation step: check whether `Node::inner_endpoint()` (`node.rs:518`)
already reaches the per-peer connection — if yes this may be zero upstream
API and one x0x-side helper.
**Scope here:** `StreamType::Audio` frames (1 Opus frame + 8-byte
seq/timestamp header per datagram, ≤ `max_datagram_size`) over the lane;
jitter buffer (fixed 60 ms initial) + drop-stats surfaced.
**Acceptance:** loss-tolerance test — 5% synthetic datagram drop, audio
keeps flowing, p95 added latency < 40 ms vs lossless (the HOL-blocking test
the reliable lane fails by construction).

### WP-V1.4 — capture/playout (scope definition)
**Scope defined, thin module:** `saorsa-webrtc-audio` crate: cpal
capture → 20 ms PCM frames → Opus; inverse for playout; device
enumeration; no DSP (AEC/AGC explicitly out, documented — headset-first).
**Acceptance:** two-Studio audible call harness (below) uses it.

## 5. Milestones & product gate

| Milestone | Contents | Gate |
|---|---|---|
| **R0** | WP-V0.1 + V0.4 | CI green on 0.27.34 |
| **R1** | WP-V0.2 + V0.3 | two-node e2e + Opus interop green — the crate's claims are true |
| **R2** | WP-V1.1 + V1.2 | e2e over x0x agents, ACL-gated |
| **R3** | WP-V1.3 + V1.4 | **product gate:** two Mac Studios, real mics, one QUIC flow in the packet capture, audible round-trip < 150 ms mouth-to-ear on LAN, 5%-loss soak stays intelligible |

R3 is driven by the Studio ssh/mosh + computer-control harness (same
pattern as tic-tac-toe's functional plan) and recorded as the demo
artifact.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Dep-tree reconciliation (rustls/aws-lc-rs/saorsa-pqc) is the hidden cost of WP-V0.1, not the API diff | Proven by the path-override check; budgeted first, copied from x0x's known-good tree |
| Datagram plumbing needs an upstream ant-quic release | Ask is 3 thin delegates; interim fallback = reliable Audio stream (works, degrades under loss); ant-quic releases are routine (0.27.x cadence) |
| Latency budget (capture 20 ms + encode + jitter 60 ms + net) vs 150 ms gate | Budget table maintained in STATUS.md from R1 measurements; jitter buffer adaptive in V2 if needed |
| Stub habits recur (aspirational docs) | WP-V0.4 rule: no capability claim without a named test; CI enforces doc build |
| saorsa-pqc/ML-KEM version skew between this crate and x0x | Single source: match x0x's Cargo.lock versions exactly at every step |

## 7. Team split (recommended)

- **Agent A (transport):** WP-V0.1 → WP-V1.2 → WP-V1.3 (x0x-side + upstream ask)
- **Agent B (codec/audio):** WP-V0.2 → WP-V1.4
- **Agent C (test):** WP-V0.3 harness first (against A/B's branches), then R2/R3 gates; owns the Studio harness
- Sequential gate: nothing in V1 starts until R1 is green (the crate must first be true).
