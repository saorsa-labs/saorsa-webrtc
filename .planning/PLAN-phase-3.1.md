# Phase 3.1: Call Structure Refactoring

**Objective**: Replace RTCPeerConnection usage in Call struct with QuicMediaTransport, maintaining all current functionality.

## Summary

The current `Call` struct uses `webrtc::peer_connection::RTCPeerConnection` which requires SDP/ICE negotiation. This phase refactors to use `QuicMediaTransport` from Milestone 2, eliminating WebRTC ICE dependencies.

## Current State Analysis

**Current Call struct** (`saorsa-webrtc-core/src/call.rs:51`):
```rust
pub struct Call<I: PeerIdentity> {
    pub id: CallId,
    pub remote_peer: I,
    pub peer_connection: Arc<RTCPeerConnection>,  // <- TO REMOVE
    pub state: CallState,
    pub constraints: MediaConstraints,
    pub tracks: Vec<WebRtcTrack>,
}
```

**Methods using peer_connection**:
- `initiate_call()` - Creates RTCPeerConnection, adds tracks
- `end_call()` - Calls `peer_connection.close()`
- `create_offer()` - Creates SDP offer
- `handle_answer()` - Sets remote SDP
- `add_ice_candidate()` - Adds ICE candidates
- `start_ice_gathering()` - No-op currently

**Target**:
Replace `Arc<RTCPeerConnection>` with `Arc<QuicMediaTransport>`, which already has:
- Connection state management (Connected/Disconnected/Failed)
- Stream management per media type
- Statistics tracking
- Health monitoring

---

## Tasks

### Task 1: Add QuicMediaTransport to Call struct
**Files**: `saorsa-webrtc-core/src/call.rs`
**~30 lines**

Add `QuicMediaTransport` field alongside `peer_connection` (for gradual migration):
```rust
pub struct Call<I: PeerIdentity> {
    pub id: CallId,
    pub remote_peer: I,
    pub peer_connection: Arc<RTCPeerConnection>,
    pub media_transport: Option<Arc<QuicMediaTransport>>,  // NEW
    pub state: CallState,
    pub constraints: MediaConstraints,
    pub tracks: Vec<WebRtcTrack>,
}
```

Add import for `QuicMediaTransport` and required types.

**Tests**: Existing tests should pass unchanged.

---

### Task 2: Update initiate_call to create QuicMediaTransport
**Files**: `saorsa-webrtc-core/src/call.rs`
**~40 lines**

Modify `initiate_call()` to:
1. Create `QuicMediaTransport::new()`
2. Store in Call struct
3. Keep RTCPeerConnection creation for now (legacy path)

**Tests**: Verify `media_transport` field is set on new calls.

---

### Task 3: Add QUIC-based call methods
**Files**: `saorsa-webrtc-core/src/call.rs`
**~80 lines**

Add new methods to `CallManager`:
- `initiate_quic_call(callee, constraints, peer: PeerConnection)` - Uses QuicMediaTransport only
- `connect_quic_transport(call_id, peer: PeerConnection)` - Connect transport to a peer

These methods bypass SDP/ICE entirely using existing QuicMediaTransport API.

**Tests**: Unit tests for new methods.

---

### Task 4: Refactor end_call to handle both transports
**Files**: `saorsa-webrtc-core/src/call.rs`
**~30 lines**

Modify `end_call()` to:
1. Disconnect `QuicMediaTransport` if present
2. Close `RTCPeerConnection` (legacy)
3. Clean up tracks from media manager

**Tests**: Verify both cleanup paths work.

---

### Task 5: Add QuicCallState mapping
**Files**: `saorsa-webrtc-core/src/types.rs`, `saorsa-webrtc-core/src/call.rs`
**~50 lines**

Add mapping between `MediaTransportState` and `CallState`:
- `Disconnected` -> `Idle` or `Ending`
- `Connecting` -> `Connecting`
- `Connected` -> `Connected`
- `Failed` -> `Failed`

Add `call_state_from_transport()` helper function.

**Tests**: Unit tests for state mapping.

---

### Task 6: Update CallState transitions for QUIC flow
**Files**: `saorsa-webrtc-core/src/call.rs`
**~60 lines**

Update state machine for QUIC-native calls:
- `Idle` -> `Calling` (when initiating QUIC call)
- `Calling` -> `Connecting` (when QuicMediaTransport connects)
- `Connecting` -> `Connected` (when transport state is Connected)
- `Connected` -> `Ending` (when ending call)
- `Ending` -> `Idle` (when transport disconnected)
- Any -> `Failed` (on transport error)

Add `update_state_from_transport()` method.

**Tests**: Call state machine tests for QUIC flow.

---

### Task 7: Maintain PeerIdentity type safety
**Files**: `saorsa-webrtc-core/src/call.rs`
**~20 lines**

Ensure all new methods maintain generic type parameter `I: PeerIdentity`:
- `initiate_quic_call` returns `Result<CallId, CallError>`
- `connect_quic_transport` uses call's `remote_peer`
- Events include peer identity

**Tests**: Verify type inference works correctly.

---

## Quality Gates

- [ ] `cargo check --all-features --all-targets` - Zero errors
- [ ] `cargo clippy --all-features --all-targets -- -D warnings` - Zero warnings
- [ ] `cargo fmt --all -- --check` - Formatting OK
- [ ] `cargo test --all-features --all-targets` - All pass
- [ ] `cargo doc --all-features --no-deps` - No doc warnings

## Dependencies

- **Milestone 2 Complete**: QuicMediaTransport available
- **PeerConnection type**: From `link_transport` module

## Notes

- This phase adds QUIC transport alongside existing WebRTC path
- Phase 3.2 will remove SDP/ICE methods
- Phase 3.3 will replace WebRtcTrack with QUIC-backed tracks
