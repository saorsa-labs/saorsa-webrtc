# Saorsa WebRTC

A unified QUIC-native WebRTC implementation over ant-quic transport.

[![Crates.io](https://img.shields.io/crates/v/saorsa-webrtc-core.svg)](https://crates.io/crates/saorsa-webrtc-core)
[![Documentation](https://docs.rs/saorsa-webrtc-core/badge.svg)](https://docs.rs/saorsa-webrtc-core)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)

## Overview

`saorsa-webrtc` provides a **unified QUIC-native** WebRTC implementation where **all media flows over QUIC streams** instead of traditional ICE/UDP connections. This eliminates the need for STUN/TURN servers by leveraging ant-quic's built-in NAT traversal.

### **Latest Release: v0.3.0**

Major architecture update: Unified QUIC media transport, single-connection model, no STUN/TURN required!

## Key Features

- **Unified QUIC Transport**: All signaling AND media over a single QUIC connection
- **No STUN/TURN Required**: ant-quic handles NAT traversal natively
- **Stream Multiplexing**: Dedicated streams per media type with priority ordering
- **Post-Quantum Cryptography**: Built-in PQC support via ant-quic (ML-DSA/ML-KEM)
- **Generic Peer Identity**: Abstracted peer identification via `PeerIdentity` trait
- **High Performance**: Low-latency with stream-level QoS (Audio: 50ms, Video: 150ms)
- **Multi-Platform**: CLI, mobile (Swift/Kotlin), and desktop (Tauri) support
- **Backward Compatible**: Legacy WebRTC support via `legacy-webrtc` feature flag

## Architecture (v0.3.0)

```
QUIC Connection (single)
├── Stream 0x20: Signaling
├── Stream 0x21: Audio RTP
├── Stream 0x22: Video RTP
├── Stream 0x23: Screen Share RTP
├── Stream 0x24: RTCP Feedback
└── Stream 0x25: Data Channel

NAT Traversal: Built-in (coordinator-based)
Crypto: TLS 1.3 + Post-Quantum (ML-DSA/ML-KEM)
```

### Benefits

- **Single NAT binding** - One connection for all traffic
- **No duplicate NAT traversal** - Eliminates ICE + QUIC parallel paths
- **Simpler deployment** - No STUN/TURN infrastructure needed
- **Better security** - TLS 1.3 for all streams

## Installation

### Library Packages
```toml
# Core library (default: QUIC-native only)
saorsa-webrtc-core = "0.3.0"

# With legacy WebRTC support for gradual migration
saorsa-webrtc-core = { version = "0.3.0", features = ["legacy-webrtc"] }
```

### Feature Flags

| Flag | Description | Default |
|------|-------------|---------|
| `quic-native` | QUIC-based media transport | Yes |
| `legacy-webrtc` | Include traditional WebRTC support | Yes (for compatibility) |

## Usage

### QUIC-Native Call (Recommended)

```rust
use saorsa_webrtc_core::prelude::*;
use saorsa_webrtc_core::link_transport::PeerConnection;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create WebRTC service
    let service = WebRtcService::<PeerIdentityString, AntQuicTransport>::builder()
        .with_identity("alice-bob-charlie-david")
        .build()
        .await?;

    // Start the service
    service.start().await?;

    // Get call manager
    let call_manager = service.call_manager();

    // Initiate QUIC-native call
    let peer_conn = PeerConnection {
        peer_id: "eve-frank-grace-henry".to_string(),
        remote_addr: peer_socket_addr,
    };

    let call_id = call_manager
        .initiate_quic_call("eve-frank-grace-henry", MediaConstraints::video_call(), peer_conn)
        .await?;

    // Exchange capabilities (replaces SDP)
    let local_caps = call_manager.exchange_capabilities(call_id).await?;
    // Send local_caps to peer via signaling...

    // Confirm connection with peer capabilities
    let peer_caps = MediaCapabilities {
        audio: true,
        video: true,
        data_channel: false,
        max_bandwidth_kbps: 2500,
    };
    call_manager.confirm_connection(call_id, peer_caps).await?;

    Ok(())
}
```

### Migration from v0.2.1

See [MIGRATION_GUIDE.md](docs/MIGRATION_GUIDE.md) for detailed migration instructions.

**Quick summary:**

| v0.2.1 (Legacy) | v0.3.0 (QUIC-native) |
|-----------------|---------------------|
| `create_offer()` | `exchange_capabilities()` |
| `handle_answer()` | `confirm_connection()` |
| `add_ice_candidate()` | Not needed |
| SDP strings | `MediaCapabilities` struct |
| STUN/TURN servers | Not needed |

## Stream Multiplexing

Media streams are prioritized for optimal real-time performance:

| Priority | Stream Type | Target Latency |
|----------|-------------|----------------|
| High (1) | Audio, RTCP | 50ms |
| Medium (2) | Video | 150ms |
| Low (3) | Screen Share | 200ms |
| Best Effort (4) | Data Channel | N/A |

See [STREAM_MULTIPLEXING.md](docs/STREAM_MULTIPLEXING.md) for architecture details.

## Project Structure

```
saorsa-webrtc/
├── saorsa-webrtc-core/     # Core WebRTC implementation
│   └── src/
│       ├── lib.rs              # Public API
│       ├── transport.rs        # QuicMediaTransport, stream multiplexing
│       ├── quic_bridge.rs      # RTP packet framing
│       ├── quic_streams.rs     # Stream management with QoS
│       ├── call.rs             # Call state machine (QUIC-native + legacy)
│       ├── media.rs            # TrackBackend abstraction
│       └── signaling.rs        # Signaling protocol
├── saorsa-webrtc-cli/      # Terminal-based video calling
├── saorsa-webrtc-codecs/   # Video/audio codec support
├── saorsa-webrtc-ffi/      # Mobile platform bindings
└── saorsa-webrtc-tauri/    # Desktop integration
```

## Documentation

- [Migration Guide](docs/MIGRATION_GUIDE.md) - Migrating from v0.2.1 to v0.3.0
- [Stream Multiplexing](docs/STREAM_MULTIPLEXING.md) - QUIC stream architecture
- [API Documentation](https://docs.rs/saorsa-webrtc-core) - Rust docs

## Status

**v0.3.0 - Production Ready**

- Unified QUIC media transport
- Stream multiplexing with QoS
- Capability exchange (replaces SDP)
- 224 tests passing
- Zero warnings
- Comprehensive documentation

## Contributing

This is part of the Saorsa project ecosystem. For contribution guidelines, see the main Saorsa project.

## License

This project is licensed under the [GNU Affero General Public License v3.0](LICENSE) (AGPL-3.0).

Copyright (C) 2024-2026 Saorsa Labs Limited and David Irvine

## Related Projects

- **saorsa-core**: Core P2P networking with DHT-based peer discovery
- **ant-quic**: QUIC implementation with NAT traversal and PQC support
- **communitas**: Application using gossip-based signaling

## Contact

Part of the Saorsa Labs ecosystem.
