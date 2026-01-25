# Phase 3.2: SDP/ICE Removal

**Objective**: Replace SDP/ICE signaling methods with QUIC-native capability exchange and remove dependency on WebRTC negotiation protocol.

## Summary

Phase 3.1 refactored the Call struct to support QUIC-native transport. Phase 3.2 removes the legacy SDP/ICE signaling paths entirely, replacing them with:
- Capability exchange (instead of SDP offer/answer)
- Direct QUIC connection confirmation (instead of ICE)
- Simplified SignalingMessage enum (no ICE fields)

This completes the transition from WebRTC signaling to QUIC-native signaling.

## Current State Analysis

**Methods to be replaced/removed**:
- `create_offer()` - Creates SDP offer (WebRTC only)
- `handle_answer()` - Sets remote SDP (WebRTC only)
- `add_ice_candidate()` - Adds ICE candidates (WebRTC only)
- `start_ice_gathering()` - Starts ICE candidate gathering (WebRTC only)

**Methods to be added**:
- `exchange_capabilities()` - Exchange media capabilities without SDP
- `confirm_connection()` - Confirm QUIC connection is ready
- `validate_capabilities()` - Validate remote capabilities

**Types to simplify**:
- `SignalingMessage` - Remove `Offer`, `Answer`, `IceCandidate` variants
- Add `CapabilityExchange`, `ConnectionConfirmation` variants

**References**:
- Phase 3.1 completed Call struct refactoring
- QuicMediaTransport available from Milestone 2
- State machine in place (Task 6)

---

## Tasks

### Task 1: Replace create_offer with capability exchange
**Files**: `saorsa-webrtc-core/src/call.rs`, `saorsa-webrtc-core/src/types.rs`
**~60 lines**

Replace `create_offer()` with capability exchange mechanism:

```rust
/// Exchange media capabilities with peer (QUIC-native)
/// 
/// Sends our capabilities instead of SDP offer.
/// Returns capabilities in JSON format for transmission.
pub async fn exchange_capabilities(
    &self,
    call_id: CallId,
) -> Result<MediaCapabilities, CallError>
```

Add `MediaCapabilities` struct:
```rust
pub struct MediaCapabilities {
    pub audio: bool,
    pub video: bool,
    pub data_channel: bool,
    pub max_bandwidth: u32,
}
```

**Dependencies**: Call struct, CallError, CallState

**Tests**: Unit tests for capability exchange and serialization.

---

### Task 2: Replace handle_answer with connection confirmation
**Files**: `saorsa-webrtc-core/src/call.rs`
**~50 lines**

Replace `handle_answer()` with connection confirmation:

```rust
/// Confirm peer capabilities and activate connection (QUIC-native)
///
/// Called after exchanging capabilities. Verifies peer capabilities
/// match our constraints and confirms QUIC connection is ready.
pub async fn confirm_connection(
    &self,
    call_id: CallId,
    peer_capabilities: MediaCapabilities,
) -> Result<(), CallError>
```

Implementation:
1. Validate peer capabilities match call constraints
2. Confirm QuicMediaTransport is connected
3. Update call state to Connected
4. Emit ConnectionEstablished event

**Tests**: Validation tests, state transition tests.

---

### Task 3: Remove ICE candidate methods
**Files**: `saorsa-webrtc-core/src/call.rs`
**~0 lines (removal)**

Remove these methods entirely (they're WebRTC-specific):
- `add_ice_candidate()` - No longer needed
- `start_ice_gathering()` - No longer needed

These are replaced by QUIC connection management (automatic).

**Tests**: Remove related tests, add deprecation notes if needed.

---

### Task 4: Simplify SignalingMessage enum
**Files**: `saorsa-webrtc-core/src/types.rs`
**~40 lines**

Current SignalingMessage (example):
```rust
pub enum SignalingMessage {
    Offer(String),          // SDP offer
    Answer(String),         // SDP answer
    IceCandidate(String),   // ICE candidate
    // ...
}
```

Update to:
```rust
pub enum SignalingMessage {
    CapabilityExchange(MediaCapabilities),
    ConnectionConfirmation { call_id: CallId, peer_capabilities: MediaCapabilities },
    ConnectionReady(CallId),
    // ... other variants
}
```

Add new types:
- `MediaCapabilities` struct (from Task 1)
- Update serialization

**Tests**: Enum variant tests, serialization tests.

---

### Task 5: Add capability validation
**Files**: `saorsa-webrtc-core/src/call.rs`
**~30 lines**

Add helper method for capability validation:

```rust
/// Validate remote capabilities match call constraints
fn validate_remote_capabilities(
    constraints: &MediaConstraints,
    remote_caps: &MediaCapabilities,
) -> Result<(), CallError>
```

Validation rules:
- If audio required, remote must support audio
- If video required, remote must support video
- Bandwidth requirements met
- Data channel requirements (if applicable)

**Tests**: Various constraint/capability combinations.

---

### Task 6: Update call state transitions
**Files**: `saorsa-webrtc-core/src/call.rs`
**~40 lines**

Update state transitions for QUIC-native calls:
- `Calling` → `Connecting` (when exchanging capabilities)
- `Connecting` → `Connected` (when confirming connection)
- Remove ICE-related states

Add documentation of new flow:
```
    Idle
     ↓
  Calling (initiate_quic_call)
     ↓
  Connecting (exchange_capabilities)
     ↓
  Connected (confirm_connection)
     ↓
  Ending (end_call)
     ↓
   Idle
```

**Tests**: Call flow tests for new sequence.

---

### Task 7: Maintain backward compatibility for legacy path
**Files**: `saorsa-webrtc-core/src/call.rs`
**~20 lines**

Keep legacy SDP/ICE methods for calls without QuicMediaTransport:
- `create_offer()` - Still works for legacy RTCPeerConnection
- `handle_answer()` - Still works for legacy RTCPeerConnection
- Add check: if no media_transport, use legacy path

Document deprecation path:
- Phase 3.2: Both paths available
- Phase 3.3: Legacy path removed

**Tests**: Legacy path still works for non-QUIC calls.

---

### Task 8: Add comprehensive integration tests
**Files**: `saorsa-webrtc-core/src/call.rs` (tests module)
**~150 lines**

Test QUIC-native call flow end-to-end:
1. Initiate QUIC call
2. Exchange capabilities
3. Validate capabilities
4. Confirm connection
5. Verify Connected state
6. End call
7. Verify Idle state

Test error cases:
- Incompatible capabilities
- Connection confirmation failures
- Transport not ready
- Invalid call ID

**Tests**: 
- test_quic_call_flow_success
- test_capability_exchange_serialization
- test_capability_validation
- test_confirm_connection_incompatible_caps
- test_legacy_fallback_still_works

---

## Quality Gates

- [ ] `cargo check --all-features --all-targets` - Zero errors
- [ ] `cargo clippy --all-features --all-targets -- -D warnings` - Zero warnings
- [ ] `cargo fmt --all -- --check` - Formatting OK
- [ ] `cargo test --all-features --all-targets` - All pass
- [ ] `cargo doc --all-features --no-deps` - No doc warnings

## Dependencies

- **Phase 3.1 Complete**: Call struct refactored
- **MediaConstraints**: Available in types module
- **CallState**: Available in types module
- **QuicMediaTransport**: Available from Milestone 2

## Notes

- Phase 3.2 removes SDP/ICE, replacing with QUIC-native exchange
- Phase 3.3 will adapt media tracks to QUIC
- After Phase 3.2, calls use QUIC signaling exclusively for new calls
- Legacy WebRTC path deprecated but maintained until Phase 3.3
- No changes to codec layer (OpenH264, Opus)

## Success Criteria

- All create_offer/handle_answer/add_ice_candidate calls removed (except legacy fallback)
- SignalingMessage enum simplified with new variants
- New capability exchange working end-to-end
- All tests passing
- Zero warnings or errors
- Legacy path still works for non-QUIC calls
- Type safety maintained

---

## Integration Notes

This phase enables:
- QUIC-native signaling without WebRTC overhead
- Removal of ICE gathering complexity
- Simplified capability negotiation
- Cleaner connection flow

After Phase 3.2:
- Calls can be QUIC-native from start to finish
- Phase 3.3 adapts media tracks to QUIC
- Milestone 3 (Call Manager Rewrite) complete
