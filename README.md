# Saorsa WebRTC

A WebRTC implementation over ant-quic transport with pluggable signaling mechanisms.

[![Crates.io](https://img.shields.io/crates/v/saorsa-webrtc-core.svg)](https://crates.io/crates/saorsa-webrtc-core)
[![Documentation](https://docs.rs/saorsa-webrtc-core/badge.svg)](https://docs.rs/saorsa-webrtc-core)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)

## Overview

`saorsa-webrtc` provides a WebRTC implementation that uses **ant-quic as the transport layer** instead of traditional ICE/STUN/TURN protocols. This approach leverages QUIC's built-in NAT traversal, post-quantum cryptography, and multiplexing capabilities while maintaining WebRTC's media streaming features.

### **Latest Release: v0.2.1** 🚀

Production readiness improvements with security hardening, QUIC data path implementation, and comprehensive observability!

## Key Features

- **Native QUIC Transport**: Uses ant-quic for reliable, encrypted connections with automatic NAT traversal
- **Transport-Agnostic Signaling**: Pluggable signaling layer supporting multiple backends:
  - DHT-based signaling (for saorsa-core integration)
  - Gossip-based rendezvous (for communitas integration)
  - Custom transport implementations via `SignalingTransport` trait
- **Post-Quantum Cryptography**: Built-in PQC support via ant-quic
- **Generic Peer Identity**: Abstracted peer identification via `PeerIdentity` trait
- **High Performance**: Low-latency media streaming with configurable QoS parameters
- **Multi-Platform**: CLI, mobile (Swift/Kotlin), and desktop (Tauri) support
- **Professional CLI**: Terminal-based video calling with real-time UI
- **Codec Support**: OpenH264 video encoding/decoding with compression
- **Type-Safe**: Generic over identity and transport types with full type safety

## Architecture

### Core Components

1. **Signaling Layer** (`signaling.rs`)
   - `SignalingTransport` trait for pluggable transport mechanisms
   - SDP offer/answer exchange
   - ICE candidate negotiation
   - Connection state management

2. **Media Management** (`media.rs`)
   - Audio/video device enumeration
   - Media stream management
   - Track handling
   - Device event notifications

3. **Call Management** (`call.rs`)
   - Call state machine (Idle → Calling → Connecting → Connected → Ending)
   - Call initiation and acceptance
   - Call lifecycle management
   - Event broadcasting

4. **Transport Integration** (`transport.rs`)
   - ant-quic transport adapter
   - Endpoint discovery
   - Message routing

5. **QUIC Bridge** (`quic_bridge.rs`, `quic_streams.rs`)
   - RTP packet to QUIC stream translation
   - QoS parameter management (audio: 50ms, video: 150ms, screen share: 200ms)
   - Media prioritization

### Generic Architecture

The library is generic over two key traits:

```rust
pub trait PeerIdentity:
    Clone + Debug + Display + Serialize + Deserialize + Send + Sync + 'static
{
    fn to_string_repr(&self) -> String;
    fn from_string_repr(s: &str) -> anyhow::Result<Self>;
    fn unique_id(&self) -> String;
}

pub trait SignalingTransport: Send + Sync {
    type PeerId: Clone + Send + Sync + Debug + Display + FromStr;
    type Error: std::error::Error + Send + Sync + 'static;

    async fn send_message(&self, peer: &Self::PeerId, message: SignalingMessage)
        -> Result<(), Self::Error>;
    async fn receive_message(&self)
        -> Result<(Self::PeerId, SignalingMessage), Self::Error>;
    async fn discover_peer_endpoint(&self, peer: &Self::PeerId)
        -> Result<Option<SocketAddr>, Self::Error>;
}
```

This allows the library to work with different peer identity schemes (e.g., FourWordAddress in saorsa-core, gossip IDs in communitas) and different signaling mechanisms (DHT, gossip, centralized servers).

## Installation

### **CLI Tool** (Recommended for getting started)
```bash
cargo install saorsa-webrtc-cli
saorsa --help
```

### **Library Packages**
```toml
# Core library
saorsa-webrtc-core = "0.2.1"

# Codec support (stub implementations for development)
saorsa-webrtc-codecs = "0.2.1"

# CLI interface
saorsa-webrtc-cli = "0.2.1"

# Mobile bindings (iOS/Android)
saorsa-webrtc-ffi = "0.2.1"

# Desktop integration (Tauri)
saorsa-webrtc-tauri = "0.2.1"
```

## Usage

### Basic Example

```rust
use saorsa_webrtc::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create WebRTC service with string-based peer identity
    let service = WebRtcService::<PeerIdentityString, AntQuicTransport>::builder()
        .with_identity("alice-bob-charlie-david")
        .build()
        .await?;

    // Start the service
    service.start().await?;

    // Subscribe to WebRTC events
    let mut events = service.subscribe_events();

    // Initiate a video call
    let call_id = service.initiate_call(
        "eve-frank-grace-henry",
        MediaConstraints::video_call()
    ).await?;

    // Handle events
    while let Ok(event) = events.recv().await {
        match event {
            WebRtcEvent::IncomingCall { from, call_id, constraints } => {
                println!("Incoming call from {}: {:?}", from, constraints);
                service.accept_call(&call_id).await?;
            }
            WebRtcEvent::CallConnected { call_id } => {
                println!("Call {} connected", call_id);
            }
            WebRtcEvent::CallEnded { call_id, reason } => {
                println!("Call {} ended: {:?}", call_id, reason);
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
```

### CLI Usage

The CLI provides a professional terminal-based video calling experience:

```bash
# Initiate a call
saorsa call alice-bob-charlie-david --video --audio --display sixel

# Listen for incoming calls
saorsa listen --auto-accept --display ascii

# Show status
saorsa status

# Get help
saorsa --help
```

**CLI Features:**
- Real-time terminal UI with live statistics (RTT, bitrate, FPS)
- Interactive controls: mute (m), toggle video (v), quit (q)
- Multiple display modes: Sixel graphics, ASCII art, none
- Auto-accept mode for daemon operation

### Codec Usage

The codec system provides efficient video compression:

```rust
use saorsa_webrtc_codecs::{OpenH264Encoder, OpenH264Decoder, VideoFrame};

// Create encoder and decoder
let mut encoder = OpenH264Encoder::new()?;
let mut decoder = OpenH264Decoder::new()?;

// Create a video frame
let frame = VideoFrame {
    data: rgb_pixel_data,
    width: 640,
    height: 480,
    timestamp: 12345,
};

// Encode frame (compresses to ~25% of original size)
let compressed = encoder.encode(&frame)?;

// Decode frame (reconstructs original data)
let reconstructed = decoder.decode(&compressed)?;
```

### Integration with saorsa-core (DHT Signaling)

```rust
use saorsa_webrtc::prelude::*;
use saorsa_core::FourWordAddress;

// Use DHT-based signaling transport
let service = WebRtcService::<FourWordAddress, DhtSignalingTransport>::builder()
    .with_identity(my_four_word_address)
    .with_transport(dht_transport)
    .build()
    .await?;
```

### Integration with communitas (Gossip Signaling)

```rust
use saorsa_webrtc::prelude::*;
use communitas::GossipIdentity;

// Use gossip-based rendezvous signaling
let service = WebRtcService::<GossipIdentity, GossipSignalingTransport>::builder()
    .with_identity(my_gossip_identity)
    .with_transport(gossip_transport)
    .build()
    .await?;
```

### Custom Signaling Transport

Implement the `SignalingTransport` trait for your own signaling mechanism:

```rust
use saorsa_webrtc::{SignalingTransport, SignalingMessage};
use async_trait::async_trait;

pub struct MyCustomTransport {
    // Your transport state
}

#[async_trait]
impl SignalingTransport for MyCustomTransport {
    type PeerId = String;
    type Error = MyError;

    async fn send_message(&self, peer: &String, message: SignalingMessage)
        -> Result<(), MyError>
    {
        // Your implementation
        Ok(())
    }

    async fn receive_message(&self)
        -> Result<(String, SignalingMessage), MyError>
    {
        // Your implementation
        todo!()
    }

    async fn discover_peer_endpoint(&self, peer: &String)
        -> Result<Option<SocketAddr>, MyError>
    {
        // Your implementation
        Ok(None)
    }
}
```

## Dependencies

This library depends on:

- **ant-quic**: QUIC transport with NAT traversal and PQC support (path: `../ant-quic`)
- **tokio**: Async runtime
- **webrtc**: WebRTC protocol implementation
- **serde**: Serialization/deserialization
- **async-trait**: Async trait support

## Differences from Traditional WebRTC

Traditional WebRTC uses:
- ICE for connectivity establishment
- STUN/TURN for NAT traversal
- DTLS for encryption
- Centralized or P2P signaling servers

Saorsa WebRTC uses:
- **ant-quic for connectivity** (built-in NAT traversal via hole punching)
- **No STUN/TURN required** (QUIC handles NAT traversal)
- **QUIC encryption** (with optional post-quantum cryptography)
- **Pluggable signaling** (DHT, gossip, or custom)

This approach provides:
- Simpler deployment (no STUN/TURN infrastructure)
- Better security (PQC support, modern crypto)
- More flexibility (pluggable signaling and identity)
- Improved performance (QUIC's congestion control and multiplexing)

## Project Structure

### **Workspace Architecture (v0.2.0)**

```
saorsa-webrtc/
├── saorsa-webrtc-core/     # Core WebRTC implementation
│   └── src/
│       ├── lib.rs              # Public API and module exports
│       ├── identity.rs         # PeerIdentity trait and implementations
│       ├── types.rs            # Core data structures (CallId, MediaConstraints, etc.)
│       ├── signaling.rs        # Signaling protocol and transport abstraction
│       ├── media.rs            # Media stream management with codecs
│       ├── call.rs             # Call state management
│       ├── service.rs          # WebRtcService and builder
│       ├── transport.rs        # ant-quic transport adapter
│       ├── quic_bridge.rs      # WebRTC to QUIC bridge
│       ├── quic_streams.rs     # QUIC media stream management with QoS
│       └── signaling_gossip_example.rs  # Gossip integration example
├── saorsa-webrtc-cli/      # Terminal-based video calling
│   └── src/
│       ├── main.rs         # CLI entry point
│       └── terminal_ui.rs  # Ratatui-based TUI
├── saorsa-webrtc-codecs/   # Video/audio codec support
│   └── src/
│       ├── lib.rs         # Codec traits and types
│       └── openh264.rs    # OpenH264 implementation
├── saorsa-webrtc-ffi/      # Mobile platform bindings
│   └── src/
│       └── lib.rs         # C API for Swift/Kotlin
└── saorsa-webrtc-tauri/    # Desktop integration
    └── src/
        └── lib.rs         # Tauri plugin commands
```

## Status

**Current Status**: Core structure implemented, stub implementations in place.

**Completed**:
- ✅ Generic architecture with `PeerIdentity` and `SignalingTransport` traits
- ✅ Signaling protocol definition with pluggable transports
- ✅ Type-safe data structures
- ✅ Module organization and workspace structure
- ✅ CLI interface with terminal UI
- ✅ Codec simulation stubs for development (OpenH264/Opus)
- ✅ FFI bindings for mobile platforms
- ✅ Tauri plugin integrated with core library
- ✅ QUIC media data path implementation
- ✅ Security hardening (DoS protection, size limits, graceful shutdown)
- ✅ Comprehensive observability with structured tracing
- ✅ Strict clippy policy enforcement (panic/unwrap/expect forbidden)
- ✅ Compilation verified (zero errors, zero warnings)

**In Progress**:
- Real OpenH264/Opus integration (currently using simulation stubs - see codec documentation)
- Sixel video display in terminal
- Integration testing with communitas gossip network
- End-to-end integration tests with ant-quic loopback

**Planned**:
- Performance benchmarks
- Usage examples
- Full communitas integration
- Documentation improvements

## Contributing

This is part of the Saorsa project ecosystem. For contribution guidelines, see the main Saorsa project.

## License

This project is licensed under the [GNU Affero General Public License v3.0](LICENSE) (AGPL-3.0).

Copyright (C) 2024 Saorsa Labs Limited and David Irvine

## Related Projects

- **saorsa-core**: Core P2P networking with DHT-based peer discovery
- **ant-quic**: QUIC implementation with NAT traversal and PQC support
- **communitas**: Application using gossip-based signaling

## Contact

Part of the Saorsa Labs ecosystem.
