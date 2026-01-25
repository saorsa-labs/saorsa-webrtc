# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-01-25

### Added
- **QUIC-native media transport**: All media now flows over QUIC streams instead of separate ICE/UDP connections
- `QuicMediaTransport` for unified media transport over ant-quic connections
- `QuicMediaStreamManager` for managing multiple concurrent media streams with QoS
- Stream multiplexing with dedicated streams per media type (Audio 0x21, Video 0x22, Screen 0x23, RTCP 0x24, Data 0x25)
- `MediaCapabilities` struct for capability exchange replacing SDP
- `initiate_quic_call()` and `confirm_connection()` for QUIC-native call setup
- `exchange_capabilities()` for negotiating media parameters
- `TrackBackend` trait for polymorphic track implementation (QUIC or legacy WebRTC)
- `QuicTrackBackend` for QUIC-based media tracks
- `quic-native` feature flag (default) for QUIC-only builds
- `legacy-webrtc` feature flag for backward compatibility
- Stream priority system: Audio/RTCP (high), Video (medium), Screen (low), Data (best effort)
- Comprehensive documentation: Migration Guide, Stream Multiplexing Architecture

### Changed
- Upgraded `ant-quic` from 0.10.3 to 0.20.0
- Media now shares the same QUIC connection as signaling (single NAT binding)
- `Call` struct now includes `QuicMediaTransport` alongside legacy `RTCPeerConnection`
- `CallState` transitions updated for QUIC flow
- `SignalingTransport` extended with `get_quic_connection()` for connection sharing

### Deprecated
- `create_offer()` - Use `exchange_capabilities()` instead
- `handle_answer()` - Use `confirm_connection()` instead
- `add_ice_candidate()` - Not needed for QUIC calls
- `start_ice_gathering()` - Not needed for QUIC calls
- `LegacyWebRtcBackend` - Use `QuicTrackBackend` for new code
- `MediaStreamManager::new()` - Use `MediaStreamManager::with_quic()` instead
- `AudioTrack::with_webrtc()` - Use `AudioTrack::with_quic()` instead
- `VideoTrack::with_webrtc()` - Use `VideoTrack::with_quic()` instead

### Removed
- STUN/TURN server requirements (ant-quic handles NAT traversal natively)
- Duplicate NAT traversal (no more ICE + QUIC parallel paths)

### Fixed
- Call state machine tests for QUIC-native flow
- Integration tests for loopback media routing
- RTP bridge tests for stream multiplexing

### Migration
See [MIGRATION_GUIDE.md](docs/MIGRATION_GUIDE.md) for detailed migration instructions from v0.2.1.

## [0.2.1] - 2025-12-15

### Added
- Initial dual-transport architecture
- WebRTC-based call management
- Signaling over ant-quic
- Media over webrtc crate ICE/UDP

### Known Issues
- Duplicate NAT traversal (both ant-quic and webrtc-ice)
- Media flows over separate UDP sockets (not QUIC)
- Requires STUN/TURN servers for NAT traversal

---

[0.3.0]: https://github.com/dirvine/saorsa-webrtc/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/dirvine/saorsa-webrtc/releases/tag/v0.2.1
