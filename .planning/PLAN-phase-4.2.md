# Phase 4.2: Integration Testing

**Objective**: Comprehensive integration testing covering end-to-end call flows, multi-peer scenarios, and error handling.

## Context
Phase 4.1 completed all unit test updates. Phase 4.2 focuses on integration-level testing to validate QUIC-native call flows work correctly across components.

## Tasks

### Task 1: Fix ignored integration tests in integration_quic_loopback.rs
**Status**: pending
**Description**:
- Remove #[ignore] from test_quic_loopback_rtp_data_path
- Remove #[ignore] from test_quic_loopback_multiple_packets
- Fix data routing issues that caused tests to be ignored
- Use mocks where necessary to avoid ant-quic connection issues
- Verify tests pass reliably

### Task 2: Add end-to-end QUIC call flow tests
**Status**: pending
**Description**:
- Test complete call lifecycle: initiate → connect → media exchange → end
- Test QUIC-native signaling flow with CapabilityExchange/ConnectionConfirm
- Test media stream creation and teardown
- Test bidirectional media flow
- Verify proper cleanup of resources

### Task 3: Add multi-peer test scenarios
**Status**: pending
**Description**:
- Test simultaneous calls to multiple peers
- Test call waiting and call switching
- Test peer disconnection handling
- Test concurrent media streams
- Verify proper resource isolation between calls

### Task 4: Add error handling integration tests
**Status**: pending
**Description**:
- Test network timeout scenarios
- Test capability mismatch rejection
- Test invalid state transitions
- Test resource exhaustion handling
- Test graceful degradation (e.g., video → audio fallback)

### Task 5: Add connection migration tests
**Status**: pending
**Description**:
- Test QUIC connection migration during active call
- Test network change handling
- Test reconnection after temporary disconnect
- Verify media continuity across migration
- Test NAT rebinding scenarios

### Task 6: Verify 100% integration test pass rate
**Status**: pending
**Description**:
- Run all integration tests: `cargo test --test '*'`
- Run ignored tests: `cargo test -- --ignored`
- Verify zero test failures
- Verify zero flaky tests
- Document any platform-specific test requirements

## Quality Gates
- All integration tests pass
- No #[ignore] tests remaining (unless documented as platform-specific)
- Zero test failures
- Zero flaky tests
- Zero compilation warnings
- Full documentation on test scenarios

## Success Criteria
- ✅ All ignored integration tests fixed and passing
- ✅ Complete end-to-end call flow coverage
- ✅ Multi-peer scenarios validated
- ✅ Error handling thoroughly tested
- ✅ Connection migration scenarios covered
- ✅ 100% integration test pass rate
