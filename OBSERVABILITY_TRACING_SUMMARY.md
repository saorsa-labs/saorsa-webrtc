# Observability Tracing Summary

Comprehensive tracing spans have been added to `saorsa-webrtc-core` to enable observability of major operations.

## Tracing Strategy

- **Structured Logging**: All spans use structured fields for consistent log parsing
- **Hierarchical Spans**: Operations use `#[tracing::instrument]` to create parent-child relationships
- **Appropriate Levels**: Operations use correct severity levels (info/debug/trace)
- **Performance**: Skip large parameters to avoid overhead

## Files Modified

### 1. [service.rs](file:///Users/davidirvine/Desktop/Devel/projects/saorsa-webrtc/saorsa-webrtc-core/src/service.rs) - WebRtcService Operations

Added `info` level spans with structured fields:

- **`start()`** - Service lifecycle
  - Span: `#[tracing::instrument(skip(self))]`
  - Logs: service startup and completion
  
- **`initiate_call()`** - Call initiation
  - Span: `#[tracing::instrument(skip(self), fields(peer = %callee.to_string_repr()))]`
  - Fields: `peer`, `call_id` (on success)
  
- **`accept_call()`** - Call acceptance
  - Span: `#[tracing::instrument(skip(self), fields(call_id = %call_id))]`
  - Fields: `call_id`
  
- **`reject_call()`** - Call rejection
  - Span: `#[tracing::instrument(skip(self), fields(call_id = %call_id))]`
  - Fields: `call_id`
  
- **`end_call()`** - Call termination
  - Span: `#[tracing::instrument(skip(self), fields(call_id = %call_id))]`
  - Fields: `call_id`

### 2. [call.rs](file:///Users/davidirvine/Desktop/Devel/projects/saorsa-webrtc/saorsa-webrtc-core/src/call.rs) - Call Lifecycle

Added spans for state transitions and protocol operations:

- **State Transitions** - `debug` level
  - `accept_call()`: Logs `old_state` → `new_state` transition
  - `reject_call()`: Logs `old_state` → `new_state` transition
  - Fields: `call_id`, `old_state`, `new_state`

- **SDP Operations** - `debug` level
  - `create_offer()`: Creating and setting local SDP
    - Span: `#[tracing::instrument(skip(self), fields(call_id = %call_id))]`
  - `handle_answer()`: Processing remote SDP
    - Span: `#[tracing::instrument(skip(self, sdp), fields(call_id = %call_id, sdp_len = sdp.len()))]`
    - Fields: `call_id`, `sdp_len`

- **ICE Candidate Handling** - `trace` level
  - `add_ice_candidate()`: High-frequency operation
    - Span: `#[tracing::instrument(skip(self, candidate), fields(call_id = %call_id))]`
    - Fields: `call_id`

### 3. [media.rs](file:///Users/davidirvine/Desktop/Devel/projects/saorsa-webrtc/saorsa-webrtc-core/src/media.rs) - Media Operations

Added spans for device and stream management:

- **Device Enumeration** - `debug` level
  - `initialize()`: Device discovery
    - Span: `#[tracing::instrument(skip(self))]`
    - Fields: `audio_devices`, `video_devices`

- **Stream Creation** - `info` level
  - `create_audio_track()`: Audio track creation
    - Fields: `track_id`, `codec`, `clock_rate`
  - `create_video_track()`: Video track creation
    - Fields: `track_id`, `codec`, `clock_rate`

- **Stream Removal** - `info` level
  - `remove_track()`: Track cleanup
    - Fields: `track_id`, `track_type`

- **Codec Setup** - `debug` level
  - Codec configuration logs for each track

### 4. [signaling.rs](file:///Users/davidirvine/Desktop/Devel/projects/saorsa-webrtc/saorsa-webrtc-core/src/signaling.rs) - SignalingHandler

Added spans for message operations:

- **Message Send** - `debug` level
  - `send_message()`: Outgoing signaling
    - Span: `#[tracing::instrument(skip(self, message), fields(peer = %peer, message_type = ?message_type(&message)))]`
    - Fields: `peer`, `message_type` (Offer/Answer/IceCandidate/IceComplete/Bye)

- **Message Receive** - `debug` level
  - `receive_message()`: Incoming signaling
    - Span: `#[tracing::instrument(skip(self))]`
    - Fields: `peer`, `message_type` (logged after receipt)

- **Connection Establishment** - `info` level
  - `discover_peer_endpoint()`: Peer discovery
    - Span: `#[tracing::instrument(skip(self), fields(peer = %peer))]`
    - Fields: `peer`, `endpoint` (on success)

## Helper Functions

Added `message_type()` helper in [signaling.rs](file:///Users/davidirvine/Desktop/Devel/projects/saorsa-webrtc/saorsa-webrtc-core/src/signaling.rs) to extract message types for consistent tracing.

## Usage Example

```rust
// Initialize tracing subscriber (application setup)
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

tracing_subscriber::registry()
    .with(tracing_subscriber::fmt::layer())
    .with(tracing_subscriber::EnvFilter::from_default_env())
    .init();

// All operations automatically emit structured spans
let call_id = service.initiate_call(peer, constraints).await?;
// INFO initiate_call{peer="peer123"}: saorsa_webrtc_core::service: Initiating call
// INFO initiate_call{peer="peer123"}: saorsa_webrtc_core::service: Call initiated successfully call_id=...
```

## Log Levels Guide

- **`TRACE`**: High-frequency operations (ICE candidates)
- **`DEBUG`**: Technical details (state transitions, SDP operations, codec setup)
- **`INFO`**: Important operations (service start, call lifecycle, stream creation)
- **`WARN`**: Unexpected conditions (already logged in existing code)
- **`ERROR`**: Failures (already logged in existing code)

## Environment Variables

Control tracing verbosity:

```bash
# Show all logs
RUST_LOG=trace

# Show info and above
RUST_LOG=info

# Per-module control
RUST_LOG=saorsa_webrtc_core::call=debug,saorsa_webrtc_core::signaling=trace

# Specific module only
RUST_LOG=saorsa_webrtc_core::service=info
```

## Verification

All changes verified with:
- ✅ `cargo clippy --package saorsa-webrtc-core --all-features -- -D clippy::panic -D clippy::unwrap_used -D clippy::expect_used`
- ✅ 52/53 tests passing (1 pre-existing failure unrelated to tracing)
- ✅ No performance-impacting data copied into spans (all large params skipped)
- ✅ Consistent structured logging across all modules
