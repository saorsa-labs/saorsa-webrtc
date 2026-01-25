# Phase 2.2: Stream Multiplexing Strategy

## Overview
Implement stream type routing in WebRtcProtocolHandler and update quic_bridge.rs to use the stream type tagging system established in Phase 2.1.

## Context from Phase 2.1
- QuicMediaTransport implemented with 1700+ lines
- StreamType enum supports Audio=0x21, Video=0x22, Screen=0x23, RtcpFeedback (0x24), Data
- RTP framing with 2-byte length prefix implemented
- send_rtp() and recv_rtp() methods available
- Stream priority configured (Audio=High, Video=Medium, Data=Low)
- 145+ tests passing with zero warnings

## Tasks

### Task 1: Update StreamType mapping in WebRtcProtocolHandler
- **Files**: `saorsa-webrtc-core/src/protocol_handler.rs`
- **Description**: Add stream type routing to WebRtcProtocolHandler
- **Requirements**:
  - Add method to determine stream type from incoming data
  - Route streams based on first byte (type tag)
  - Map to QuicMediaTransport StreamType
  - Handle unknown stream types gracefully
- **Tests**: Stream type routing tests
- **Status**: pending

### Task 2: Update quic_bridge.rs for stream type tagging
- **Files**: `saorsa-webrtc-core/src/quic_bridge.rs`
- **Description**: Update WebRtcQuicBridge to use stream type tags when sending
- **Requirements**:
  - Prefix outgoing data with stream type tag byte
  - Parse stream type from incoming data
  - Maintain compatibility with existing RtpPacket struct
  - Update send/receive methods to include type context
- **Tests**: Stream tagging roundtrip tests
- **Status**: pending

### Task 3: Add bidirectional RTCP feedback stream
- **Files**: `saorsa-webrtc-core/src/quic_media_transport.rs`, `saorsa-webrtc-core/src/quic_bridge.rs`
- **Description**: Implement dedicated RTCP feedback stream for QoS
- **Requirements**:
  - Create RTCP stream type (0x24 already defined as RtcpFeedback)
  - Implement send_rtcp() and recv_rtcp() methods
  - Support RTCP compound packets (SR, RR, SDES, BYE)
  - Add RTCP statistics tracking
- **Tests**: RTCP send/receive tests
- **Status**: pending

### Task 4: Implement stream demultiplexer
- **Files**: `saorsa-webrtc-core/src/protocol_handler.rs`
- **Description**: Add stream demultiplexer for incoming QUIC data
- **Requirements**:
  - Parse stream type from first byte
  - Route to appropriate handler (audio/video/screen/rtcp/data)
  - Handle partial frames (buffer until complete)
  - Support priority-based processing
- **Tests**: Demultiplexer with mixed stream types
- **Status**: pending

### Task 5: Test multiplexing with concurrent streams
- **Files**: `saorsa-webrtc-core/tests/integration_tests.rs`
- **Description**: Add integration tests for concurrent multi-stream operation
- **Requirements**:
  - Test sending on multiple streams simultaneously
  - Test receiving from multiple streams
  - Verify priority handling under load
  - Test stream type isolation
- **Tests**: Concurrent stream integration tests
- **Status**: pending

### Task 6: Run cargo clippy and ensure zero warnings
- **Files**: All source files
- **Description**: Run clippy and fix any new warnings
- **Requirements**:
  - `cargo clippy --all-features -- -D warnings`
  - Fix any warnings
- **Tests**: Zero clippy warnings
- **Status**: pending

### Task 7: Run cargo test and ensure all tests pass
- **Files**: All test files
- **Description**: Run full test suite with stream multiplexing
- **Tests**: 100% test pass rate
- **Status**: pending

## Completion Criteria
- [ ] Stream type routing in WebRtcProtocolHandler
- [ ] Stream type tagging in quic_bridge.rs
- [ ] Bidirectional RTCP feedback stream working
- [ ] Stream demultiplexer routing correctly
- [ ] Concurrent stream tests passing
- [ ] Zero compilation warnings
- [ ] 100% test pass rate

## Dependencies
- Requires Phase 2.1 completion (QuicMediaTransport, StreamType enum)
- Output used by Phase 2.3 (Connection Sharing)

## Expected Outcomes
After Phase 2.2:
1. All media types (audio, video, screen, RTCP, data) correctly multiplexed
2. Stream type tagging enables proper routing
3. RTCP feedback flows on dedicated stream
4. Ready for connection sharing in Phase 2.3
