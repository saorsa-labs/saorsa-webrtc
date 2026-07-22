# Implementation Status

Last updated: 2026-07-22 (revival R0). Rule of this file: **no capability is
claimed without naming its passing test.**

## Working (test-backed)

| Capability | Proof |
|---|---|
| QUIC-native 1:1 media transport over **ant-quic 0.27.34** (single workspace version) | full workspace suite green post-upgrade; `cargo tree -i ant-quic` shows one version |
| **Real Opus** encode/decode (48 kHz mono, 20 ms frames, VoIP profile) — default feature | `saorsa-webrtc-codecs/tests/opus_interop.rs`: interop with the raw `opus` crate decoder, lag-aligned SNR > 10 dB, compression assertion (20 ms frame ≤ ~200 B at 64 kbps vs 1,920 B PCM) |
| Two **real** ant-quic nodes exchanging audio frames (RTP framing via `WebRtcQuicBridge`, and raw frames via `LinkTransport` `StreamType::Audio`) | `saorsa-webrtc-core/tests/e2e_two_nodes_audio.rs`: 250/250 frames byte-identical each path; loopback one-way p95 < 4 ms |
| QUIC-native signaling flow (`CapabilityExchange → ConnectionConfirm → ConnectionReady → Bye`) over real nodes | same e2e file, `signaling_handshake_and_bye_over_two_real_nodes` |
| Pluggable signaling (`SignalingTransport` trait) with `AntQuicTransport` impl | `core/src/signaling.rs` unit tests + the e2e above |

## Known transport semantics (measured, not speculative)

- ant-quic's per-message API does **not** preserve cross-message order
  (~4–5% reorder observed on loopback; each send rides its own uni-stream).
  Receivers must reorder by sequence number; a jitter buffer is **required**
  for real-time audio (WP-V1.3). See the module docs in
  `e2e_two_nodes_audio.rs`.

## Not implemented (types/stubs only — do not rely on)

- **Group calls:** `CallArchitecture::{Mesh, SFU}` are type definitions
  only. No mesh manager, no SFU, no mixer exists.
- **Video codecs:** OpenH264 is a stub behind a feature; the Opus stub
  survives only behind `cfg(any(test, feature = "stub-codecs"))` for
  transport-layer tests.
- **`QuicMediaTransport::recv_rtp`:** placeholder pending LinkTransport
  integration (WP-V1.2); the working RTP path is `WebRtcQuicBridge`.
- **Audio capture/playout:** no mic/speaker I/O in this workspace
  (planned `saorsa-webrtc-audio`, WP-V1.4).
- **x0x adapters** (`X0xSignaling`, `LinkTransport` over ADR-0022 streams,
  datagram audio lane): designed (`docs/design/revival-v0-v1.md` V1), not
  yet implemented.

## Platform bindings

Swift/Kotlin/FFI/Tauri/CLI modules exist with their own unit tests but are
untested against the revived 0.27-based core beyond compilation; treat as
unvalidated until they appear in the table above with a named test.
