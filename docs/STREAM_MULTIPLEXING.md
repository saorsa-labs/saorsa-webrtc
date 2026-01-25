# Stream Multiplexing Architecture

This document describes the QUIC stream multiplexing strategy used in saorsa-webrtc-core v0.3.0.

## Overview

saorsa-webrtc-core multiplexes all WebRTC media over a single QUIC connection, using dedicated streams for each media type. This eliminates the need for separate ICE/UDP connections and enables efficient NAT traversal through ant-quic.

## Stream Type Assignments

Each media type is assigned a unique stream type tag in the range `0x20-0x2F`:

| Stream Type | Tag | Description |
|-------------|-----|-------------|
| Audio | `0x20` | Audio RTP packets |
| Video | `0x21` | Video RTP packets |
| Screen Share | `0x22` | Screen sharing RTP packets |
| RTCP Feedback | `0x23` | RTCP packets for QoS |
| Data Channel | `0x24` | Application data |

### Tag Constants

From `link_transport.rs`:

```rust
#[repr(u8)]
pub enum StreamType {
    Audio = 0x20,
    Video = 0x21,
    Screen = 0x22,
    RtcpFeedback = 0x23,
    Data = 0x24,
}
```

Signaling is handled through separate signaling channels outside the stream multiplexing layer.

## Priority Ordering

Streams are prioritized to ensure real-time media quality:

| Priority | Value | Stream Types | Rationale |
|----------|-------|--------------|-----------|
| High | 1 | Audio, RTCP | Audio latency directly impacts call quality |
| Medium | 2 | Video | Video can tolerate slightly more latency |
| Low | 3 | Screen Share | Screen share is less latency-sensitive |
| Best Effort | 4 | Data Channel | Data can be buffered and retried |

### QoS Parameters

```rust
pub struct QoSParams {
    pub target_latency_ms: u32,
    pub priority: u8,
}

// Audio: 50ms target, priority 10
// Video: 150ms target, priority 5
// Screen: 200ms target, priority 3
```

## Packet Framing Format

RTP packets are framed with a stream type tag for demultiplexing:

```
+--------+------------------+
| Tag    | RTP Packet       |
| (1B)   | (variable)       |
+--------+------------------+
```

### Tagged Packet Structure

```rust
pub struct RtpPacket {
    pub payload_type: u8,
    pub sequence_number: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub payload: Vec<u8>,
    pub stream_type: StreamType,
}

// Serialization
fn to_tagged_bytes(&self) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.push(self.stream_type.to_tag()); // Stream type tag
    bytes.extend(&self.to_bytes());         // RTP packet
    bytes
}

// Deserialization
fn from_tagged_bytes(data: &[u8]) -> Result<Self> {
    let tag = data[0];
    let stream_type = StreamType::from_tag(tag)?;
    let packet = RtpPacket::from_bytes(&data[1..])?;
    Ok(packet.with_stream_type(stream_type))
}
```

## Connection Sharing Model

A single QUIC connection is shared between signaling and media:

```
┌─────────────────────────────────────────────────┐
│              ant-quic Connection                │
│                                                 │
│  ┌─────────────────────────────────────────┐   │
│  │         SignalingTransport              │   │
│  │  - Capability exchange                  │   │
│  │  - Call control messages                │   │
│  └─────────────────────────────────────────┘   │
│                      │                          │
│                      │ get_quic_connection()    │
│                      ▼                          │
│  ┌─────────────────────────────────────────┐   │
│  │         QuicMediaTransport              │   │
│  │  - Audio stream (0x20)                  │   │
│  │  - Video stream (0x21)                  │   │
│  │  - Screen stream (0x22)                 │   │
│  │  - RTCP stream (0x23)                   │   │
│  │  - Data stream (0x24)                   │   │
│  └─────────────────────────────────────────┘   │
│                                                 │
└─────────────────────────────────────────────────┘
```

### Connection Lifecycle

1. **Signaling connects** - ant-quic establishes QUIC connection
2. **Call initiated** - Capability exchange over signaling stream
3. **Media transport created** - Shares same QUIC connection
4. **Streams opened** - Dedicated streams per media type
5. **Media flows** - RTP packets tagged and multiplexed
6. **Call ends** - Streams closed, connection may persist

## Stream Management

### Opening Streams

```rust
let transport = QuicMediaTransport::new();
transport.connect(peer_connection).await?;

// Open streams as needed
transport.open_stream(StreamType::Audio).await?;
transport.open_stream(StreamType::Video).await?;
```

### Stream Isolation

Each stream is independent:
- Audio issues don't affect video
- Data channel congestion doesn't block media
- RTCP feedback has dedicated path

### Concurrent Streams

Multiple streams can be active simultaneously:

```rust
// All streams open at once
transport.open_all_streams().await?;

// Send on multiple streams concurrently
transport.send_audio(&audio_rtp).await?;
transport.send_video(&video_rtp).await?;
transport.send_rtcp(&rtcp_packet).await?;
```

## RTCP Feedback Handling

RTCP packets flow on a dedicated stream (0x23) for QoS:

- Receiver reports
- Sender reports
- NACK (negative acknowledgment)
- PLI (picture loss indication)
- FIR (full intra request)

This ensures RTCP feedback isn't blocked by media congestion.

## Benefits of Multiplexing

1. **Single NAT binding** - One connection to traverse NAT
2. **Shared congestion control** - QUIC manages all streams
3. **Reduced latency** - No separate ICE negotiation
4. **Simplified firewall** - One port for all traffic
5. **Better security** - TLS 1.3 for all streams

## Implementation Notes

### Thread Safety

`QuicMediaTransport` uses `Arc<RwLock<>>` for thread-safe access:

```rust
pub struct QuicMediaTransport {
    state: Arc<RwLock<MediaTransportState>>,
    streams: Arc<RwLock<HashMap<StreamType, StreamHandle>>>,
    // ...
}
```

### Error Handling

Stream errors are isolated:

```rust
match transport.send_audio(&packet).await {
    Ok(_) => { /* Success */ }
    Err(MediaTransportError::StreamError(e)) => {
        // Audio stream error, video unaffected
    }
    Err(MediaTransportError::NotConnected) => {
        // Connection lost
    }
}
```

## Related Documentation

- [Migration Guide](MIGRATION_GUIDE.md) - Migrating from v0.2.1
- [API Documentation](https://docs.rs/saorsa-webrtc-core) - Rust docs
- [ROADMAP.md](../.planning/ROADMAP.md) - Project roadmap
