# WebRTC Crate Usage Audit - Saorsa WebRTC

**Generated:** 2026-01-25
**Purpose:** Complete inventory of webrtc dependency usage for replacement planning

---

## Executive Summary

The saorsa-webrtc codebase currently uses the webrtc crate family for:
1. **RTCPeerConnection** - Core WebRTC connection management
2. **TrackLocalStaticSample** - Media track abstraction
3. **RTCRtpCodecCapability** - Codec configuration
4. **ICE candidate handling** - NAT traversal (duplicate of ant-quic)
5. **SDP exchange** - Session description protocol

This creates a **dual-transport architecture** where:
- **Signaling** uses ant-quic (correctly)
- **Media** uses webrtc crate's ICE/UDP (incorrectly)

---

## Current WebRTC Dependencies (Cargo.toml)

```toml
# In saorsa-webrtc-core/Cargo.toml

# Primary QUIC transport (correct)
ant-quic = { version = "0.10.3", features = ["pqc"] }

# WebRTC dependencies (TO BE REMOVED/REPLACED)
webrtc = "0.13"
webrtc-ice = "0.13"         # ← DUPLICATE NAT traversal
webrtc-media = "0.10"
webrtc-sctp = "0.12"
webrtc-srtp = "0.15"
webrtc-dtls = "0.12"
webrtc-data = "0.11"
interceptor = "0.14"
rtcp = "0.13"               # ← KEEP: RTP/RTCP packet parsing
rtp = "0.13"                # ← KEEP: RTP/RTCP packet parsing
```

---

## File-by-File Usage Analysis

### call.rs (MAJOR - 642 lines)

**Imports:**
```rust
use webrtc::peer_connection::RTCPeerConnection;
```

**Usage Locations:**

| Line | Usage | Classification |
|------|-------|----------------|
| 11 | `use webrtc::peer_connection::RTCPeerConnection` | ESSENTIAL |
| 54 | `pub peer_connection: Arc<RTCPeerConnection>` | ESSENTIAL |
| 127-142 | `webrtc::api::APIBuilder::new().build().new_peer_connection()` | ESSENTIAL |
| 157-162 | `peer_connection.add_track()` | ESSENTIAL |
| 172-177 | `peer_connection.add_track()` | ESSENTIAL |
| 304 | `call.peer_connection.close().await` | ESSENTIAL |
| 337 | `call.peer_connection.create_offer(None).await` | ESSENTIAL |
| 341-347 | `call.peer_connection.set_local_description()` | ESSENTIAL |
| 374-385 | `RTCSessionDescription::answer()`, `set_remote_description()` | ESSENTIAL |
| 409-418 | `RTCIceCandidateInit`, `add_ice_candidate()` | ESSENTIAL (TO REMOVE) |

**Summary:** call.rs is the PRIMARY consumer of webrtc crate functionality. It uses:
- RTCPeerConnection for connection management
- SDP offer/answer exchange
- ICE candidate handling (this is what we want to ELIMINATE)

---

### media.rs (MAJOR - 510 lines)

**Imports:**
```rust
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
```

**Usage Locations:**

| Line | Usage | Classification |
|------|-------|----------------|
| 13 | `use RTCRtpCodecCapability` | ESSENTIAL |
| 14 | `use TrackLocalStaticSample` | ESSENTIAL |
| 87 | `pub webrtc_track: Arc<TrackLocalStaticSample>` | ESSENTIAL |
| 166 | `pub track: Arc<TrackLocalStaticSample>` | ESSENTIAL |
| 265-271 | `RTCRtpCodecCapability` for audio codec config | REPLACEABLE |
| 274-278 | `TrackLocalStaticSample::new()` for audio track | REPLACEABLE |
| 303-309 | `RTCRtpCodecCapability` for video codec config | REPLACEABLE |
| 312-316 | `TrackLocalStaticSample::new()` for video track | REPLACEABLE |
| 352-358 | `RTCRtpCodecCapability` for codec tracks | REPLACEABLE |
| 360-364 | `TrackLocalStaticSample::new()` | REPLACEABLE |

**Summary:** media.rs uses webrtc types for:
- Codec capability definitions (can be replaced with custom types)
- Track abstraction (can be replaced with QUIC-backed tracks)

---

### Other Files (No Direct webrtc Usage)

| File | webrtc Usage | Notes |
|------|--------------|-------|
| transport.rs | None | Uses ant-quic only |
| quic_bridge.rs | None | Custom RTP bridge |
| quic_streams.rs | None | Custom stream management |
| protocol_handler.rs | None | Uses custom types |
| signaling.rs | None | Transport-agnostic |
| service.rs | None | Orchestration only |
| types.rs | None | Custom type definitions |
| identity.rs | None | Identity abstraction |
| lib.rs | None | Public API |

---

## Classification Summary

### ESSENTIAL (Must Replace)
These components are core to the dual-transport problem:

| Component | Location | Replacement Strategy |
|-----------|----------|---------------------|
| RTCPeerConnection | call.rs | Replace with QuicMediaTransport |
| ICE candidate handling | call.rs | REMOVE - use ant-quic NAT traversal |
| SDP offer/answer | call.rs | Simplify to capability exchange |

### REPLACEABLE (Can Use Custom Types)
These use webrtc types but functionality is duplicated in QUIC layer:

| Component | Location | Replacement Strategy |
|-----------|----------|---------------------|
| TrackLocalStaticSample | media.rs | Create QuicMediaTrack |
| RTCRtpCodecCapability | media.rs | Create custom CodecCapability |

### KEEP (RTP/RTCP Parsing Only)
These crates provide packet parsing without transport:

| Crate | Purpose | Action |
|-------|---------|--------|
| rtp | RTP packet serialization | KEEP |
| rtcp | RTCP packet serialization | KEEP |

---

## Dependency Removal Plan

### Phase 1: Make Optional (Feature Flags)
```toml
[features]
default = ["quic-native"]
quic-native = []
legacy-webrtc = ["webrtc", "webrtc-ice", "webrtc-media", ...]
```

### Phase 2: Remove Transport Dependencies
```toml
# REMOVE ENTIRELY (duplicate NAT traversal)
webrtc-ice = "0.13"

# REMOVE (not needed for QUIC transport)
webrtc-dtls = "0.12"
webrtc-srtp = "0.15"
webrtc-sctp = "0.12"
webrtc-data = "0.11"
interceptor = "0.14"
```

### Phase 3: Replace Core Types
```toml
# REMOVE after implementing QuicMediaTransport
webrtc = "0.13"
webrtc-media = "0.10"
```

### Phase 4: Final Dependencies
```toml
# KEEP - packet parsing only, no transport
rtp = "0.13"
rtcp = "0.13"

# UPGRADE - primary transport
ant-quic = "0.20.0"
```

---

## API Changes Required

### call.rs Changes

**Before (webrtc-based):**
```rust
pub struct Call<I: PeerIdentity> {
    pub peer_connection: Arc<RTCPeerConnection>,
    // ...
}
```

**After (QUIC-native):**
```rust
pub struct Call<I: PeerIdentity> {
    pub media_transport: Arc<QuicMediaTransport>,
    // RTCPeerConnection REMOVED
}
```

### media.rs Changes

**Before (webrtc-based):**
```rust
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

pub struct VideoTrack {
    pub webrtc_track: Arc<TrackLocalStaticSample>,
}
```

**After (QUIC-native):**
```rust
pub struct VideoTrack {
    pub quic_stream: QuicMediaStream,
    // TrackLocalStaticSample REMOVED
}
```

---

## Conclusion

The webrtc crate usage is concentrated in two files:
1. **call.rs** - Connection management and ICE (REMOVE ICE)
2. **media.rs** - Track abstractions (REPLACE with QUIC)

The fix requires:
1. Removing RTCPeerConnection in favor of direct QUIC streams
2. Removing ICE candidate handling (ant-quic handles NAT)
3. Creating custom track types backed by QUIC streams
4. Keeping only rtp/rtcp crates for packet parsing

Total webrtc API touchpoints: **~25 locations across 2 files**
