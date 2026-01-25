# Migration Guide: v0.2.1 to v0.3.0

This guide covers migrating from saorsa-webrtc-core v0.2.1 (legacy WebRTC with optional QUIC) to v0.3.0 (unified QUIC-native transport).

## Summary of Changes

v0.3.0 unifies media transport over QUIC streams, eliminating the need for separate WebRTC ICE/UDP connections.

### Breaking Changes

| Area | v0.2.1 | v0.3.0 |
|------|--------|--------|
| Media Transport | WebRTC ICE/UDP | QUIC Streams |
| NAT Traversal | STUN/TURN required | ant-quic native |
| Connection Model | Separate signaling + media | Single multiplexed connection |
| SDP/ICE | Required for calls | Replaced with capability exchange |
| Feature Flags | None | `quic-native` (default), `legacy-webrtc` |

### New APIs

| API | Description |
|-----|-------------|
| `CallManager::initiate_quic_call()` | Start a QUIC-native call |
| `CallManager::confirm_connection()` | Confirm connection with capabilities |
| `QuicMediaTransport` | QUIC-based media transport |
| `MediaCapabilities` | Capability exchange structure |

### Deprecated APIs

| API | Replacement |
|-----|-------------|
| `CallManager::create_offer()` | `initiate_quic_call()` |
| `CallManager::handle_answer()` | `confirm_connection()` |
| `CallManager::add_ice_candidate()` | Not needed (QUIC handles NAT) |

## Migration Steps

### Step 1: Update Cargo.toml

```toml
[dependencies]
saorsa-webrtc-core = "0.3"

# For gradual migration, enable legacy support:
# saorsa-webrtc-core = { version = "0.3", features = ["legacy-webrtc"] }
```

### Step 2: Update Call Initiation

**Before (v0.2.1):**
```rust
// SDP-based call initiation
let call_id = call_manager.initiate_call(peer_id, constraints).await?;
let offer = call_manager.create_offer(call_id).await?;
// Send offer via signaling...
// Receive answer...
call_manager.handle_answer(call_id, answer).await?;
// Exchange ICE candidates...
```

**After (v0.3.0):**
```rust
use saorsa_webrtc_core::link_transport::PeerConnection;

// QUIC-native call initiation
let peer_conn = PeerConnection {
    peer_id: peer_id.to_string(),
    remote_addr: peer_socket_addr,
};

let call_id = call_manager
    .initiate_quic_call(peer_id, constraints, peer_conn)
    .await?;

// Exchange capabilities with peer (send via signaling to peer)
let local_capabilities = call_manager.exchange_capabilities(call_id).await?;
// Send local_capabilities to peer via signaling...
// Receive remote_capabilities from peer...

// Confirm with peer capabilities
call_manager.confirm_connection(call_id, remote_capabilities).await?;
```

### Step 3: Update Signaling Messages

**Before (v0.2.1):**
```rust
enum SignalingMessage {
    Offer(String),        // SDP
    Answer(String),       // SDP
    IceCandidate(String), // ICE candidate
    // ...
}
```

**After (v0.3.0):**
```rust
enum SignalingMessage {
    CallRequest {
        call_id: CallId,
        constraints: MediaConstraints,
    },
    CallResponse {
        call_id: CallId,
        capabilities: MediaCapabilities,
        accepted: bool,
    },
    // SDP/ICE messages deprecated but available with legacy-webrtc feature
}
```

### Step 4: Handle Call State Changes

The call state machine now includes QUIC-specific states:

```rust
match call_state {
    CallState::Calling => { /* Initiated, awaiting response */ }
    CallState::Connecting => { /* QUIC connection establishing */ }
    CallState::Connected => { /* Ready for media */ }
    CallState::Failed => { /* Connection failed */ }
}
```

### Step 5: Update Media Handling

Media tracks work the same way, but transport is now QUIC-backed:

```rust
// Create tracks (unchanged)
let audio_track = media_manager.create_audio_track().await?;
let video_track = media_manager.create_video_track().await?;

// Tracks automatically use QUIC transport when connected
```

## Feature Flags

### `quic-native` (default)
- Uses QUIC streams for all media
- No STUN/TURN servers required
- Recommended for new applications

### `legacy-webrtc`
- Enables deprecated SDP/ICE APIs
- Allows gradual migration
- Will be removed in v0.4.0

```toml
# Use only QUIC (recommended)
saorsa-webrtc-core = "0.3"

# Enable legacy for gradual migration
saorsa-webrtc-core = { version = "0.3", features = ["legacy-webrtc"] }
```

## Stream Multiplexing Architecture

v0.3.0 multiplexes all media over a single QUIC connection:

```
QUIC Connection
├── Stream 0x20: Audio RTP
├── Stream 0x21: Video RTP
├── Stream 0x22: Screen Share RTP
├── Stream 0x23: RTCP Feedback
└── Stream 0x24: Data Channel
```

Note: Signaling uses separate channels outside the multiplexing layer.

### Priority Ordering

| Priority | Stream Type | Latency Target |
|----------|-------------|----------------|
| 1 (High) | Audio | 50ms |
| 1 (High) | RTCP Feedback | 50ms |
| 2 (Medium) | Video | 150ms |
| 3 (Low) | Screen Share | 200ms |
| 4 (Lowest) | Data Channel | Best effort |

## Common Migration Issues

### Issue: ICE candidate exchange failing

**Cause:** QUIC-native calls don't use ICE.

**Solution:** Remove ICE candidate handling for QUIC calls. The connection is established directly via ant-quic's NAT traversal.

### Issue: SDP parsing errors

**Cause:** QUIC-native calls use capability exchange, not SDP.

**Solution:** Use `MediaCapabilities` instead of SDP strings:

```rust
let caps = MediaCapabilities::from_constraints(&constraints);
```

### Issue: Connection timeout

**Cause:** Peer not reachable via QUIC.

**Solution:** Ensure both peers are using the same ant-quic coordinator for NAT traversal. Check firewall settings.

## Testing Your Migration

```bash
# Run all tests
cargo test --all-features

# Test QUIC-native only
cargo test

# Test with legacy support
cargo test --features legacy-webrtc
```

## Getting Help

- GitHub Issues: https://github.com/saorsa-labs/saorsa-webrtc/issues
- Documentation: https://docs.rs/saorsa-webrtc-core
