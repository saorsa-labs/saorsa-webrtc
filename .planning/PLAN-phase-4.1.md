# Phase 4.1: Unit Test Updates

**Objective**: Achieve 100% test pass rate by fixing ignored tests and ensuring comprehensive coverage of QUIC-native transport.

**From ROADMAP**:
- Update call_state_machine_tests.rs for new Call structure
- Fix quic_transport_tests.rs timing issues
- Add QuicMediaTransport unit tests
- Test stream multiplexing and priority
- Achieve 100% test pass rate

**Current Status**:
- 391 tests passing, 7 tests ignored
- Doctests ignored (need runtime, acceptable)

---

## Task 1: Fix quic_transport_tests.rs Timing Issues

**Files**: `saorsa-webrtc-core/tests/quic_transport_tests.rs`

**Problem**: 2 tests are ignored due to ant-quic connection timing issues in test environment:
- `test_transport_send_receive` (line 32)
- `test_transport_multiple_peers` (line 102)

**Solution**:
- Replace ant-quic transport with mock transport for unit tests
- Create `MockSignalingTransport` that simulates connection behavior
- Keep actual ant-quic tests in integration tests only (Phase 4.2)

**Acceptance Criteria**:
- [ ] Both tests pass or are converted to mock-based tests
- [ ] Remove `#[ignore]` annotations
- [ ] Zero test failures

---

## Task 2: Fix rtp_bridge_tests.rs Connection Issues

**Files**: `saorsa-webrtc-core/tests/rtp_bridge_tests.rs`

**Problem**: `test_bridge_send_receive_roundtrip` is ignored due to ant-quic issues.

**Solution**:
- Convert to mock-based test using local channels
- Test the bridge logic without real network

**Acceptance Criteria**:
- [ ] Test passes without `#[ignore]`
- [ ] Bridge send/receive logic verified

---

## Task 3: Update call_state_machine_tests.rs for QUIC Flows

**Files**: `saorsa-webrtc-core/tests/call_state_machine_tests.rs`

**Analysis**: Current tests cover:
- Basic state transitions (Calling → Connected, Calling → Failed)
- Invalid transitions
- Concurrent call limits
- Legacy SDP/ICE methods (with deprecated annotation)

**Gaps**: Need tests for:
- QUIC-native call initiation (`initiate_quic_call`)
- Capability exchange flow (`exchange_capabilities`)
- Connection confirmation (`confirm_connection`)
- QUIC-specific state transitions (Calling → Connecting → Connected)

**Solution**:
- Add `test_quic_call_state_transitions`
- Add `test_capability_exchange_updates_state`
- Add `test_confirm_connection_validates_capabilities`
- Add `test_quic_call_failure_handling`

**Acceptance Criteria**:
- [ ] All QUIC call flows have corresponding tests
- [ ] State machine behavior fully covered
- [ ] Zero warnings

---

## Task 4: Add QuicMediaTransport Unit Tests

**Files**: `saorsa-webrtc-core/src/quic_media_transport.rs` (add tests module)

**Current Coverage**:
- `track_backend_integration.rs` tests integration with tracks
- Missing: isolated unit tests for QuicMediaTransport itself

**Tests to Add**:
- `test_transport_creation_and_defaults`
- `test_connect_updates_state`
- `test_disconnect_cleans_up`
- `test_open_audio_stream_assigns_type`
- `test_open_video_stream_assigns_type`
- `test_open_screen_stream_assigns_type`
- `test_send_rtp_packet_on_stream`
- `test_get_stats_aggregates_all_streams`

**Acceptance Criteria**:
- [ ] Unit tests cover all public methods
- [ ] Edge cases (not connected, stream not open) tested
- [ ] Tests run fast (no real network)

---

## Task 5: Add Stream Multiplexing Tests

**Files**: `saorsa-webrtc-core/tests/` (new file: `stream_multiplexing_tests.rs`)

**Tests**:
- `test_stream_type_to_stream_id_mapping`
- `test_audio_video_screen_concurrent_streams`
- `test_stream_priority_ordering` (audio > video > screen > data)
- `test_rtcp_stream_separate_from_media`
- `test_data_channel_stream_isolation`

**Acceptance Criteria**:
- [ ] Stream type constants tested
- [ ] Priority ordering verified
- [ ] Multiple concurrent streams supported

---

## Task 6: Verify 100% Test Pass Rate

**Actions**:
- Run `cargo test --all-features --all-targets`
- Ensure zero ignored tests (except acceptable doctests)
- Run `cargo clippy --all-features --all-targets -- -D warnings`
- Run `cargo fmt --all -- --check`

**Acceptance Criteria**:
- [ ] All tests pass
- [ ] Zero clippy warnings
- [ ] Code formatted

---

## Files Summary

| Task | Files to Modify/Create |
|------|----------------------|
| 1 | tests/quic_transport_tests.rs |
| 2 | tests/rtp_bridge_tests.rs |
| 3 | tests/call_state_machine_tests.rs |
| 4 | src/quic_media_transport.rs |
| 5 | tests/stream_multiplexing_tests.rs (new) |
| 6 | (validation only) |

---

## Quality Gates

- Zero compilation errors
- Zero compilation warnings
- Zero test failures
- Zero clippy violations
- All public methods tested
