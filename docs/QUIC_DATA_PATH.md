# QUIC Media Data Path Implementation

## Overview

This document describes the QUIC media data path implementation for the WebRTC bridge, which enables transmission of RTP packets and media streams over QUIC transport using the ant-quic library.

## Architecture

The QUIC data path consists of three main components:

### 1. Transport Layer (`transport.rs`)

The `AntQuicTransport` provides the low-level QUIC connectivity using ant-quic's `QuicP2PNode`:

- **send_bytes()**: Sends raw bytes to the default peer
- **receive_bytes()**: Receives raw bytes from any connected peer
- Uses `node.send_to_peer(peer_id, data)` from ant-quic
- Uses `node.receive()` from ant-quic

### 2. QUIC Bridge (`quic_bridge.rs`)

The `WebRtcQuicBridge` handles RTP packet serialization and transmission:

- **send_rtp_packet()**: Serializes and sends RTP packets over QUIC
  - Validates packet size limits
  - Serializes using `RtpPacket::to_bytes()`
  - Adds tracing spans with stream type, priority, and sequence number
  
- **receive_rtp_packet()**: Receives and deserializes RTP packets from QUIC
  - Receives raw bytes from transport
  - Deserializes using `RtpPacket::from_bytes()` (includes size validation)
  - Logs stream type and sequence information

### 3. Stream Manager (`quic_streams.rs`)

The `QuicMediaStreamManager` manages multiple media streams with QoS parameters:

- **send_data()**: Sends data on a specific stream
  - Maps stream ID to QoS parameters
  - Adds tracing with stream type and priority
  - Delegates to transport layer
  
- **receive_data()**: Receives data from a specific stream
  - Validates stream exists
  - Adds tracing with stream type and priority
  - Delegates to transport layer

## Stream Priority Mapping

Stream priorities are mapped from WebRTC stream types to QUIC priorities:

| Stream Type   | Priority | Target Latency | Use Case       |
|--------------|----------|----------------|----------------|
| Audio        | 1 (High) | 50ms          | Voice calls    |
| Video        | 2        | 150ms         | Video calls    |
| ScreenShare  | 3        | 200ms         | Screen sharing |
| Data         | 4 (Low)  | Best-effort   | Data channels  |

## Usage Example

```rust
use saorsa_webrtc_core::{
    transport::{AntQuicTransport, TransportConfig},
    quic_bridge::{WebRtcQuicBridge, QuicBridgeConfig, RtpPacket, StreamType},
    quic_streams::{QuicMediaStreamManager, QoSParams, MediaStreamType},
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Create and start transport
    let config = TransportConfig::default();
    let mut transport = AntQuicTransport::new(config);
    transport.start().await?;
    
    // Connect to peer
    let peer_addr = "127.0.0.1:8080".parse()?;
    transport.connect_to_peer(peer_addr).await?;
    
    let transport_arc = Arc::new(transport);
    
    // 2. Create QUIC bridge with transport
    let bridge = WebRtcQuicBridge::with_transport(
        QuicBridgeConfig::default(),
        (*transport_arc).clone()
    );
    
    // 3. Send RTP packet
    let packet = RtpPacket::new(
        96,                      // payload_type
        1000,                    // sequence_number
        12345,                   // timestamp
        0xDEADBEEF,             // ssrc
        vec![1, 2, 3, 4],       // payload
        StreamType::Audio        // stream_type
    )?;
    
    bridge.send_rtp_packet(&packet).await?;
    
    // 4. Receive RTP packet
    let received = bridge.receive_rtp_packet().await?;
    println!("Received packet: seq={}, type={:?}", 
             received.sequence_number, 
             received.stream_type);
    
    // 5. Use stream manager for multiple streams
    let mut stream_manager = QuicMediaStreamManager::with_transport(
        QoSParams::audio(),
        transport_arc.clone()
    );
    
    let audio_stream = stream_manager.create_stream(MediaStreamType::Audio)?;
    let video_stream = stream_manager.create_stream(MediaStreamType::Video)?;
    
    // Send on specific streams
    stream_manager.send_data(audio_stream, &[1, 2, 3]).await?;
    stream_manager.send_data(video_stream, &[4, 5, 6]).await?;
    
    // Receive from stream
    let data = stream_manager.receive_data(audio_stream).await?;
    
    Ok(())
}
```

## Error Handling

All functions follow the strict no-panic policy:

- **No `unwrap()`**, **`expect()`**, or **`panic!()`** in production code
- All errors use `thiserror` for structured error types
- Bridge errors: `BridgeError::ConfigError`, `BridgeError::StreamError`
- Stream errors: `StreamError::ConfigError`, `StreamError::OperationError`
- Transport errors: `TransportError::SendError`, `TransportError::ReceiveError`

## Tracing

All operations include tracing spans for observability:

### Send RTP Packet
```rust
tracing::debug_span!(
    "send_rtp_packet",
    stream_type = ?packet.stream_type,
    priority = packet.stream_type.priority(),
    seq_num = packet.sequence_number
);
```

### Receive RTP Packet
```rust
tracing::debug_span!("receive_rtp_packet");
```

### Stream Send/Receive
```rust
tracing::debug_span!(
    "send_stream_data",
    stream_id = stream_id,
    stream_type = ?stream.stream_type,
    priority = stream.qos_params.priority,
    data_len = data.len()
);
```

### Transport Operations
```rust
tracing::debug_span!(
    "transport_send_bytes",
    data_len = data.len()
);
```

## Integration Points

### 1. WebRTC Track Bridging

To bridge a WebRTC track to QUIC:

```rust
// Extract RTP packets from WebRTC track
// Serialize to RTP packet structure
// Send via bridge
bridge.send_rtp_packet(&rtp_packet).await?;
```

### 2. Peer Management

The transport maintains a peer map and default peer:

- Connect to peers using `transport.connect_to_peer(addr)`
- Data is sent to the default peer (first connected peer)
- Received data can come from any connected peer

### 3. Stream Lifecycle

Streams should be created before use:

```rust
let stream_id = manager.create_stream(MediaStreamType::Audio)?;
// ... use stream ...
manager.close_stream(stream_id)?;
```

## Performance Considerations

1. **Packet Size Limits**: 
   - Maximum packet size: 1200 bytes
   - Maximum payload: 1188 bytes (after 12-byte RTP header)
   
2. **Serialization**:
   - Uses `postcard` for efficient binary serialization
   - Size validation before deserialization prevents DoS
   
3. **Priority Handling**:
   - Stream priorities are metadata only in current implementation
   - Future: Can map to QUIC stream priorities when ant-quic supports it

## Future Enhancements

1. **Multi-peer Support**: 
   - Currently sends to default peer
   - Can extend to send to specific peers by stream ID
   
2. **Stream Priority Enforcement**:
   - Map stream priorities to QUIC stream priorities
   - Requires ant-quic API enhancement
   
3. **Congestion Control**:
   - Integrate with WebRTC bandwidth estimation
   - Adaptive bitrate based on QUIC feedback
   
4. **Packet Loss Recovery**:
   - FEC (Forward Error Correction)
   - Selective retransmission for important frames

## Testing

Tests verify correct behavior without transport:

```rust
#[tokio::test]
async fn test_without_transport() {
    let bridge = WebRtcQuicBridge::default();
    let result = bridge.receive_rtp_packet().await;
    // Should fail with ConfigError
    assert!(matches!(result, Err(BridgeError::ConfigError(_))));
}
```

Integration tests with actual transport require running ant-quic nodes.

## Security

- All data sent over QUIC is encrypted by ant-quic
- Size validation prevents buffer overflow attacks
- No secrets in logs (only packet metadata)
- PQC (Post-Quantum Cryptography) support via ant-quic feature flag
