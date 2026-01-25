# Phase 2.3: Connection Sharing

## Overview
Share QUIC connections between signaling and media transport, eliminating the need for separate ICE negotiation.

## Context from Phase 2.2
- Stream type tagging implemented (0x21-0x25 range)
- Stream routing in WebRtcProtocolHandler working
- RTCP feedback stream support added
- 150+ tests passing with zero warnings

## Tasks

### Task 1: Update SignalingTransport trait with get_quic_connection()
- **Files**: `saorsa-webrtc-core/src/signaling.rs`
- **Description**: Add method to expose underlying QUIC connection
- **Requirements**:
  - Add `get_connection()` method to SignalingTransport trait
  - Return Arc<Connection> or similar handle
  - Maintain connection ownership/lifetime properly
- **Tests**: Connection retrieval tests
- **Status**: pending

### Task 2: Implement connection sharing between signaling and media
- **Files**: `saorsa-webrtc-core/src/signaling.rs`, `saorsa-webrtc-core/src/quic_media_transport.rs`
- **Description**: Share QUIC connection for both signaling and media
- **Requirements**:
  - Create QuicMediaTransport from SignalingTransport connection
  - Support both uses on same connection
  - Ensure stream type isolation
- **Tests**: Shared connection tests
- **Status**: pending

### Task 3: Create QuicMediaTransport from SignalingTransport connection
- **Files**: `saorsa-webrtc-core/src/quic_media_transport.rs`
- **Description**: Add factory method to create media transport from signaling connection
- **Requirements**:
  - Add `from_signaling(signaling: &SignalingTransport)` method
  - Share underlying QUIC connection
  - Properly initialize stream handles
- **Tests**: Factory method tests
- **Status**: pending

### Task 4: Remove need for separate ICE negotiation
- **Files**: `saorsa-webrtc-core/src/call.rs` (if feature=legacy-webrtc)
- **Description**: Update call flow to skip ICE when using QUIC-native transport
- **Requirements**:
  - Check for QUIC-native feature flag
  - Skip ICE candidate exchange when using ant-quic
  - Use existing QUIC connection for media
- **Tests**: QUIC-native call flow tests
- **Status**: pending

### Task 5: Add connection health monitoring
- **Files**: `saorsa-webrtc-core/src/quic_media_transport.rs`
- **Description**: Monitor connection health and handle failures
- **Requirements**:
  - Track connection state changes
  - Implement reconnection handling
  - Add health check methods
- **Tests**: Health monitoring tests
- **Status**: pending

### Task 6: Run cargo clippy and ensure zero warnings
- **Files**: All source files
- **Description**: Run clippy and fix any warnings
- **Tests**: Zero clippy warnings
- **Status**: pending

### Task 7: Run cargo test and ensure all tests pass
- **Files**: All test files
- **Description**: Run full test suite
- **Tests**: 100% test pass rate
- **Status**: pending

## Completion Criteria
- [ ] SignalingTransport exposes get_connection()
- [ ] Media and signaling share QUIC connection
- [ ] QuicMediaTransport can be created from signaling
- [ ] ICE negotiation skipped for QUIC-native
- [ ] Connection health monitoring working
- [ ] Zero compilation warnings
- [ ] 100% test pass rate

## Dependencies
- Requires Phase 2.2 completion (stream multiplexing)
- Completes Milestone 2 (Transport Unification)

## Expected Outcomes
After Phase 2.3:
1. Single QUIC connection for all WebRTC traffic
2. No separate ICE/STUN/TURN required
3. Simplified connection setup
4. Milestone 2 complete - ready for Milestone 3
