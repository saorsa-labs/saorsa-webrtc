# Phase 2.1: QuicMediaTransport Implementation

## Overview
Create the core QuicMediaTransport struct that replaces webrtc crate's ICE/UDP layer with direct QUIC streams for media transport.

## Context from Milestone 1
- ant-quic 0.20 successfully integrated
- LinkTransport abstraction layer in place
- StreamType enum supports 0x20-0x2F range
- Feature flags enable dual-mode operation
- All 131+ tests passing with zero warnings

## Tasks

### Task 1: Create QuicMediaTransport struct
- **Files**: `saorsa-webrtc-core/src/quic_media_transport.rs` (new)
- **Description**: Create the core struct that wraps an ant-quic Connection for media transport
- **Requirements**:
  - Define QuicMediaTransport with Connection handle
  - Store stream handles for each media type
  - Implement Send + Sync for thread safety
  - Add connection state tracking
- **Tests**: Unit tests for struct creation and state
- **Status**: pending

### Task 2: Implement dedicated QUIC streams per media type
- **Files**: `saorsa-webrtc-core/src/quic_media_transport.rs`
- **Description**: Create separate QUIC streams for audio, video, screen, and RTCP
- **Requirements**:
  - Open stream with type tag (0x20=Signaling, 0x21=Audio, 0x22=Video, 0x23=Screen, 0x24=Data)
  - Maintain stream handles in HashMap
  - Implement get_or_create_stream(stream_type)
  - Handle stream errors gracefully
- **Tests**: Stream creation and retrieval tests
- **Status**: pending

### Task 3: Add length-prefix framing for RTP packets
- **Files**: `saorsa-webrtc-core/src/quic_media_transport.rs`
- **Description**: Implement framing protocol for RTP packets over QUIC streams
- **Requirements**:
  - Use 2-byte length prefix (big-endian u16)
  - Support packets up to 65535 bytes
  - Add frame_rtp() and unframe_rtp() helpers
  - Document framing format
- **Tests**: Framing roundtrip tests
- **Status**: pending

### Task 4: Implement send_rtp() and recv_rtp() methods
- **Files**: `saorsa-webrtc-core/src/quic_media_transport.rs`
- **Description**: Core send/receive methods for RTP packets
- **Requirements**:
  - send_rtp(stream_type, rtp_packet) -> Result<()>
  - recv_rtp() -> Result<(StreamType, RtpPacket)>
  - Use framing from Task 3
  - Handle backpressure gracefully
- **Tests**: Send/receive integration tests
- **Status**: pending

### Task 5: Add stream priority and QoS integration
- **Files**: `saorsa-webrtc-core/src/quic_media_transport.rs`
- **Description**: Configure QUIC stream priorities for media types
- **Requirements**:
  - Set audio streams to highest priority
  - Set video streams to medium priority
  - Set data streams to lower priority
  - Document priority mapping
- **Tests**: Priority configuration tests
- **Status**: pending

### Task 6: Export module and run cargo clippy
- **Files**: `saorsa-webrtc-core/src/lib.rs`, all source files
- **Description**: Export new module and ensure zero warnings
- **Requirements**:
  - Add `pub mod quic_media_transport;` to lib.rs
  - Run `cargo clippy --all-features -- -D warnings`
  - Fix any warnings
- **Tests**: Zero clippy warnings
- **Status**: pending

### Task 7: Run cargo test and ensure all tests pass
- **Files**: All test files
- **Description**: Run full test suite with new QuicMediaTransport
- **Tests**: 100% test pass rate
- **Status**: pending

## Completion Criteria
- [ ] QuicMediaTransport struct defined with Connection handle
- [ ] Dedicated streams per media type (audio, video, screen, RTCP, data)
- [ ] Length-prefix framing implemented
- [ ] send_rtp() and recv_rtp() working
- [ ] Stream priority configured
- [ ] Module exported in lib.rs
- [ ] Zero compilation warnings
- [ ] 100% test pass rate

## Dependencies
- Requires Milestone 1 completion (feature flags, transport adapter)
- Output used by Phase 2.2 (stream multiplexing) and Phase 2.3 (connection sharing)

## Expected Outcomes
After Phase 2.1:
1. QuicMediaTransport can send/receive RTP packets over QUIC
2. Streams are properly typed and prioritized
3. Framing ensures packet boundaries are preserved
4. Ready for integration with WebRtcProtocolHandler
