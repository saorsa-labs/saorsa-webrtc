# Phase 1.2: Transport Adapter Layer

## Overview
Create abstraction layer for ant-quic API differences and implement stream type support for WebRTC media.

## Context from Phase 1.1
- ant-quic successfully upgraded to 0.20
- All webrtc crate usage documented in audit
- Zero compilation errors and warnings achieved
- All tests passing (131+ tests)

## Tasks

### Task 1: Create LinkTransport wrapper abstraction
- **Files**: `saorsa-webrtc-core/src/link_transport.rs` (new)
- **Description**: Create abstraction layer that wraps ant-quic 0.20 API differences. This allows for cleaner separation between transport concerns and provides migration path for future changes.
- **Requirements**:
  - Define LinkTransport trait with core methods: connect, send, receive, close
  - Implement for AntQuicTransport
  - Support PeerId and Connection abstractions
  - Document API stability guarantees
- **Tests**: Unit tests for LinkTransport trait implementation
- **Status**: pending

### Task 2: Implement stream type support
- **Files**: `saorsa-webrtc-core/src/transport.rs`
- **Description**: Add stream type tagging support for WebRTC media (0x20-0x2F range). This enables multiplexing different media types (audio, video, screen) over single QUIC connection.
- **Requirements**:
  - Define StreamType enum (Audio=0x20, Video=0x21, Screen=0x22, RTCP=0x23, Data=0x24)
  - Add stream_type parameter to send/receive methods
  - Update AntQuicTransport to support tagged streams
  - Maintain backward compatibility with existing transport API
- **Tests**: Stream type routing tests
- **Status**: pending

### Task 3: Update AntQuicTransport for stream types
- **Files**: `saorsa-webrtc-core/src/transport.rs`
- **Description**: Refactor AntQuicTransport to use LinkTransport and stream type support. This consolidates transport changes from Phase 1.1 into cohesive abstraction.
- **Requirements**:
  - Implement LinkTransport trait in AntQuicTransport
  - Update connect/send/receive to use stream types
  - Add get_stream_handle(stream_type) method
  - Verify all existing transport tests still pass
- **Tests**: Integration tests with multiple stream types
- **Status**: pending

### Task 4: Add connection sharing infrastructure
- **Files**: `saorsa-webrtc-core/src/signaling.rs`
- **Description**: Update SignalingTransport trait to expose underlying connection for media transport reuse. This is prerequisite for media/signaling connection sharing in Phase 2.
- **Requirements**:
  - Add get_connection() method to SignalingTransport trait
  - Implement in AntQuicTransport
  - Document connection lifetime and borrowing rules
  - Add safety comments for Arc sharing
- **Tests**: Connection sharing unit tests
- **Status**: pending

### Task 5: Verify backward compatibility
- **Files**: All transport-related files
- **Description**: Ensure all existing code paths continue to work with new LinkTransport abstraction and stream types.
- **Tests**: Full test suite must pass with no regressions
- **Status**: pending

### Task 6: Run cargo clippy and fix all warnings
- **Files**: All source files
- **Description**: Run `cargo clippy --all-features --all-targets -- -D warnings` and fix every warning.
- **Tests**: Zero clippy warnings
- **Status**: pending

### Task 7: Run cargo test and ensure all tests pass
- **Files**: All test files
- **Description**: Run full test suite, fix any test failures caused by transport changes.
- **Tests**: 100% test pass rate
- **Status**: pending

## Completion Criteria
- [ ] LinkTransport trait defined and implemented
- [ ] StreamType enum with 0x20-0x2F support
- [ ] AntQuicTransport uses LinkTransport abstraction
- [ ] Connection sharing infrastructure in place
- [ ] All existing tests passing (no regressions)
- [ ] Zero compilation warnings
- [ ] 100% test pass rate

## Dependencies
- Requires Phase 1.1 completion (ant-quic 0.20 upgrade)
- Output used by Phase 1.3 (feature flags)

## Expected Outcomes
After Phase 1.2:
1. LinkTransport provides stable API abstraction
2. Stream types enable media multiplexing
3. Connection sharing enables Phase 2 media transport reuse
4. Zero technical debt introduced
5. Fully backward compatible
