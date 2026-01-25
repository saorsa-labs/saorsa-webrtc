# Unified QUIC Media Transport - Project Roadmap

> **Goal**: Eliminate dual-transport architecture by routing all WebRTC media through ant-quic QUIC streams, removing webrtc-ice dependency and STUN/TURN server requirements.

## Overview

**Problem Statement**: saorsa-webrtc-core v0.2.1 has a split-brain transport architecture:
- **Signaling**: Uses ant-quic (correctly)
- **Media**: Uses webrtc crate's ICE/UDP (incorrectly)

This results in:
1. Duplicated NAT traversal (ant-quic AND webrtc-ice)
2. Media flowing over separate UDP sockets (not QUIC)
3. Requirement for STUN/TURN servers (defeats decentralization)
4. Wasted ant-quic NAT work for actual calls

**Solution**: Implement RTP-over-QUIC using the same ant-quic connections that the gossip overlay establishes.

## Success Criteria

- [ ] Zero webrtc-ice dependency
- [ ] ant-quic 0.20.0 or later
- [ ] Media flows over QUIC streams (not separate UDP)
- [ ] Single connection for signaling + media per peer
- [ ] Works through all NAT types (via ant-quic's native traversal)
- [ ] No STUN/TURN server requirements
- [ ] 100% test pass rate with zero warnings

---

## Milestone 1: Dependency Upgrade & Foundation

**Objective**: Upgrade ant-quic from 0.10.3 to 0.20.0 and establish migration foundation.

### Phase 1.1: Dependency Audit & Upgrade
- Analyze current webrtc crate usage patterns
- Upgrade ant-quic to 0.20.0 in Cargo.toml
- Resolve API breaking changes in transport.rs
- Update saorsa-transport dependency alignment
- Ensure zero compilation warnings

### Phase 1.2: Transport Adapter Layer
- Create abstraction layer for ant-quic API differences
- Implement LinkTransport wrapper for new API
- Update AntQuicTransport to use new ant-quic API
- Add stream type support (0x20-0x2F for WebRTC)
- Maintain backward compatibility during transition

### Phase 1.3: Feature Flag Infrastructure
- Add `quic-native` feature flag (default)
- Add `legacy-webrtc` feature flag (optional)
- Configure conditional compilation for dual-mode
- Update Cargo.toml with feature-gated dependencies
- Test both feature configurations

---

## Milestone 2: Transport Unification

**Objective**: Implement QUIC-native media transport, replacing webrtc crate's ICE/UDP layer.

### Phase 2.1: QuicMediaTransport Implementation
- Create QuicMediaTransport struct with ant-quic Connection
- Implement dedicated QUIC streams per media type (audio/video/screen/data)
- Add length-prefix framing for RTP packets
- Implement send_rtp() and recv_rtp() methods
- Add stream priority and QoS integration

### Phase 2.2: Stream Multiplexing Strategy
- Define stream type mapping (0x01=Audio, 0x02=Video, 0x03=Screen, 0x04=RTCP)
- Implement stream type routing in WebRtcProtocolHandler
- Update quic_bridge.rs to use stream type tagging
- Add bidirectional RTCP feedback stream
- Test multiplexing with multiple concurrent streams

### Phase 2.3: Connection Sharing
- Update SignalingTransport trait with get_quic_connection()
- Implement connection sharing between signaling and media
- Create QuicMediaTransport from SignalingTransport connection
- Remove need for separate ICE negotiation
- Add connection health monitoring

---

## Milestone 3: Call Manager Rewrite

**Objective**: Replace RTCPeerConnection usage with direct QUIC stream management.

### Phase 3.1: Call Structure Refactoring
- Remove RTCPeerConnection from Call struct
- Add QuicMediaTransport field to Call
- Replace peer_connection methods with QUIC operations
- Update CallState transitions for QUIC flow
- Maintain type safety with PeerIdentity trait

### Phase 3.2: SDP/ICE Removal
- Replace create_offer() with capability exchange
- Replace handle_answer() with capability confirmation
- Remove add_ice_candidate() entirely
- Implement confirm_connection() using QUIC connection state
- Simplify SignalingMessage enum (remove ICE fields)

### Phase 3.3: Media Track Adaptation
- Decouple VideoTrack/AudioTrack from webrtc TrackLocalStaticSample
- Create TrackBackend abstraction (QUIC or legacy WebRTC)
- Implement QUIC-backed track type
- Maintain codec layer integration (OpenH264/Opus)
- Update MediaStreamManager for new track types

---

## Milestone 4: Integration & Testing

**Objective**: Comprehensive testing, documentation, and production readiness.

### Phase 4.1: Unit Test Updates
- Update call_state_machine_tests.rs for new Call structure
- Fix quic_transport_tests.rs timing issues
- Add QuicMediaTransport unit tests
- Test stream multiplexing and priority
- Achieve 100% test pass rate

### Phase 4.2: Integration Testing
- Fix integration_quic_loopback.rs data routing
- Add end-to-end media flow tests
- Test NAT traversal scenarios (symmetric, restricted)
- Test connection migration during calls
- Benchmark latency vs standard WebRTC

### Phase 4.3: Documentation & Cleanup
- Update API documentation for new interfaces
- Add migration guide from v0.2.1 to v0.3.0
- Document stream multiplexing strategy
- Remove deprecated code paths
- Final clippy/fmt/doc validation

### Phase 4.4: Release Preparation
- Version bump to 0.3.0
- Update CHANGELOG.md
- Update README.md with new architecture
- Deprecation notices for legacy-webrtc feature
- Publish to crates.io

---

## Architecture After Refactoring

```
┌─────────────────────────────────────────────────────────────────┐
│                    saorsa-webrtc-core v0.3.0                    │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│   UNIFIED PATH:                                                 │
│   ┌─────────────────────────────────────────────────────────┐  │
│   │                    ant-quic 0.20+                        │  │
│   │  ┌───────────────────────────────────────────────────┐  │  │
│   │  │            QUIC Connection (single)                │  │  │
│   │  │                                                    │  │  │
│   │  │   Stream 0x20: Signaling                          │  │  │
│   │  │   Stream 0x21: Audio RTP                          │  │  │
│   │  │   Stream 0x22: Video RTP                          │  │  │
│   │  │   Stream 0x23: Screen Share RTP                   │  │  │
│   │  │   Stream 0x24: RTCP Feedback                      │  │  │
│   │  │   Stream 0x25: Data Channel                       │  │  │
│   │  │                                                    │  │  │
│   │  │   NAT Traversal: Built-in (coordinator-based)     │  │  │
│   │  │   Crypto: TLS 1.3 + Post-Quantum (ML-DSA/ML-KEM)  │  │  │
│   │  └───────────────────────────────────────────────────┘  │  │
│   └─────────────────────────────────────────────────────────┘  │
│                                                                 │
│   NO STUN/TURN REQUIRED                                        │
│   NO SEPARATE ICE NEGOTIATION                                  │
│   NO DUPLICATE NAT TRAVERSAL                                   │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| API breaking changes | Feature flags for gradual migration |
| ant-quic 0.10→0.20 migration | Adapter layer abstracts differences |
| Test infrastructure updates | Parallel test suites during transition |
| Codec integration stability | Keep codec layer unchanged |
| Backward compatibility | Deprecation path over 2-3 releases |

---

## Dependencies

### To Upgrade
- `ant-quic`: 0.10.3 → 0.20.0

### To Remove/Make Optional
- `webrtc`: 0.13 → optional (legacy-webrtc feature)
- `webrtc-ice`: 0.13 → remove entirely
- `webrtc-dtls`: 0.12 → optional
- `webrtc-srtp`: 0.15 → optional
- `webrtc-sctp`: 0.12 → optional
- `webrtc-data`: 0.11 → optional
- `webrtc-media`: 0.10 → optional

### To Keep
- `rtp`: 0.13 (packet parsing only)
- `rtcp`: 0.13 (packet parsing only)
- `saorsa-webrtc-codecs`: 0.2.1 (unchanged)
- `saorsa-transport`: local (already QUIC-native)

---

## Timeline Expectations

This project involves:
- 4 milestones
- 13 phases
- ~40-50 individual tasks

Quality gates apply at every phase:
- Zero compilation errors
- Zero compilation warnings
- 100% test pass rate
- Full documentation on public APIs
