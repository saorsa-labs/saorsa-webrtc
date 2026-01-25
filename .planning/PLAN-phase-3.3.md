# Phase 3.3: Media Track Adaptation

**Milestone 3**: Call Manager Rewrite
**Phase 3.3**: Media Track Adaptation
**Status**: Planning
**Created**: 2026-01-25

## Phase Objective

Decouple `VideoTrack`/`AudioTrack` from webrtc's `TrackLocalStaticSample`, create a `TrackBackend` abstraction that supports both QUIC and legacy WebRTC backends, and update `MediaStreamManager` to create QUIC-backed tracks.

## Current State Analysis

### Existing Architecture (from `media.rs`)

1. **VideoTrack** (lines 86-163):
   - Tightly coupled to `Arc<TrackLocalStaticSample>`
   - Contains encoder/decoder as `Option<Box<dyn VideoEncoder/Decoder>>`
   - Has `id`, `width`, `height`, and codec integration

2. **AudioTrack** (lines 78-83):
   - Minimal struct with just `id: String`
   - No actual audio track implementation

3. **WebRtcTrack** (lines 165-174):
   - Wrapper around `Arc<TrackLocalStaticSample>`
   - Contains `track_type: MediaType` and `id: String`

4. **MediaStreamManager** (lines 183-415):
   - Creates tracks using `TrackLocalStaticSample::new()`
   - Uses `RTCRtpCodecCapability` for codec configuration
   - Stores tracks in `webrtc_tracks: Vec<WebRtcTrack>`

### Target Architecture

1. **TrackBackend trait** - Abstraction for send/receive operations
2. **QuicTrackBackend** - Uses `QuicMediaTransport` for RTP over QUIC
3. **LegacyWebRtcBackend** - Wraps existing `TrackLocalStaticSample`
4. **Updated VideoTrack/AudioTrack** - Use backend abstraction
5. **Updated MediaStreamManager** - Factory pattern for backend selection

## Dependencies

- `QuicMediaTransport` (from Phase 2.3) - already implemented
- `StreamType` enum (Audio, Video, Screen, Data, RtcpFeedback)
- `MediaType` enum (Audio, Video, ScreenShare, DataChannel)
- `framing` module for RTP packet framing

## Tasks

### Task 1: Define TrackBackend Trait

**Description**: Create the `TrackBackend` trait that abstracts the underlying transport mechanism for media tracks.

**Files to modify**:
- `saorsa-webrtc-core/src/media.rs`

**Specification**:
```rust
/// Backend abstraction for media track transport
///
/// This trait defines the interface for sending and receiving media data,
/// allowing tracks to work with either QUIC streams or legacy WebRTC.
#[async_trait::async_trait]
pub trait TrackBackend: Send + Sync {
    /// Send RTP packet data
    async fn send(&self, data: &[u8]) -> Result<(), MediaError>;

    /// Receive RTP packet data (blocks until data available)
    async fn recv(&self) -> Result<Vec<u8>, MediaError>;

    /// Check if the backend is connected and ready
    fn is_connected(&self) -> bool;

    /// Get the backend type (for debugging/logging)
    fn backend_type(&self) -> &'static str;

    /// Get track statistics
    fn stats(&self) -> TrackStats;
}

/// Statistics for a track backend
#[derive(Debug, Clone, Default)]
pub struct TrackStats {
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
}
```

**Tests**:
- Trait must be object-safe
- `dyn TrackBackend` must be Send + Sync

---

### Task 2: Implement QuicTrackBackend

**Description**: Create `QuicTrackBackend` that wraps `QuicMediaTransport` and implements `TrackBackend`.

**Files to modify**:
- `saorsa-webrtc-core/src/media.rs`

**Specification**:
```rust
/// QUIC-native backend for media tracks
///
/// Uses QuicMediaTransport to send/receive RTP packets over QUIC streams.
pub struct QuicTrackBackend {
    transport: Arc<QuicMediaTransport>,
    stream_type: StreamType,
    stats: Arc<RwLock<TrackStats>>,
}

impl QuicTrackBackend {
    /// Create a new QUIC track backend
    pub fn new(transport: Arc<QuicMediaTransport>, media_type: MediaType) -> Self;

    /// Map MediaType to StreamType
    fn media_type_to_stream_type(media_type: MediaType) -> StreamType;
}
```

**Implementation notes**:
- Map `MediaType::Audio` → `StreamType::Audio`
- Map `MediaType::Video` → `StreamType::Video`
- Map `MediaType::ScreenShare` → `StreamType::Screen`
- Use `QuicMediaTransport::send_rtp()` and `recv_rtp()`
- Update stats on each send/recv

**Tests**:
- `test_quic_backend_send_connected`
- `test_quic_backend_send_disconnected`
- `test_quic_backend_stats_tracking`
- `test_media_type_to_stream_type_mapping`

---

### Task 3: Implement LegacyWebRtcBackend

**Description**: Create `LegacyWebRtcBackend` that wraps `TrackLocalStaticSample` for backward compatibility.

**Files to modify**:
- `saorsa-webrtc-core/src/media.rs`

**Specification**:
```rust
/// Legacy WebRTC backend for media tracks
///
/// Wraps TrackLocalStaticSample for backward compatibility during migration.
/// Marked as deprecated - will be removed when legacy-webrtc feature is removed.
#[deprecated(since = "0.3.0", note = "Use QuicTrackBackend for new code")]
pub struct LegacyWebRtcBackend {
    track: Arc<TrackLocalStaticSample>,
    track_type: MediaType,
    stats: Arc<RwLock<TrackStats>>,
}
```

**Implementation notes**:
- `send()` uses `track.write_sample()`
- `recv()` returns error (WebRTC tracks don't support direct receive)
- Mark entire struct as deprecated

**Tests**:
- `test_legacy_backend_creation`
- `test_legacy_backend_type_identification`
- Verify deprecation warnings appear in compilation

---

### Task 4: Refactor VideoTrack to Use TrackBackend

**Description**: Update `VideoTrack` to use `TrackBackend` instead of direct `TrackLocalStaticSample` reference.

**Files to modify**:
- `saorsa-webrtc-core/src/media.rs`

**Current structure (lines 86-99)**:
```rust
pub struct VideoTrack {
    pub id: String,
    pub webrtc_track: Arc<TrackLocalStaticSample>,  // Remove this
    pub encoder: Option<Box<dyn VideoEncoder>>,
    pub decoder: Option<Box<dyn VideoDecoder>>,
    pub width: u32,
    pub height: u32,
}
```

**New structure**:
```rust
pub struct VideoTrack {
    pub id: String,
    backend: Arc<dyn TrackBackend>,
    pub encoder: Option<Box<dyn VideoEncoder>>,
    pub decoder: Option<Box<dyn VideoDecoder>>,
    pub width: u32,
    pub height: u32,
}

impl VideoTrack {
    /// Create a new video track with the specified backend
    pub fn new(id: String, backend: Arc<dyn TrackBackend>, width: u32, height: u32) -> Self;

    /// Create with QUIC backend
    pub fn with_quic(id: String, transport: Arc<QuicMediaTransport>, width: u32, height: u32) -> Self;

    /// Get the underlying backend
    pub fn backend(&self) -> &Arc<dyn TrackBackend>;

    /// Send encoded video frame
    pub async fn send_frame(&mut self, frame_data: &[u8]) -> Result<(), MediaError>;

    /// Receive encoded video frame
    pub async fn recv_frame(&mut self) -> Result<Vec<u8>, MediaError>;
}
```

**Tests**:
- `test_video_track_with_quic_backend`
- `test_video_track_send_frame`
- `test_video_track_with_encoder`
- Ensure existing tests still pass

---

### Task 5: Refactor AudioTrack to Use TrackBackend

**Description**: Update `AudioTrack` to use `TrackBackend` and add proper audio handling.

**Files to modify**:
- `saorsa-webrtc-core/src/media.rs`

**Current structure (lines 78-83)**:
```rust
pub struct AudioTrack {
    pub id: String,
}
```

**New structure**:
```rust
pub struct AudioTrack {
    pub id: String,
    backend: Arc<dyn TrackBackend>,
}

impl AudioTrack {
    /// Create a new audio track with the specified backend
    pub fn new(id: String, backend: Arc<dyn TrackBackend>) -> Self;

    /// Create with QUIC backend
    pub fn with_quic(id: String, transport: Arc<QuicMediaTransport>) -> Self;

    /// Get the underlying backend
    pub fn backend(&self) -> &Arc<dyn TrackBackend>;

    /// Send audio data
    pub async fn send_audio(&self, data: &[u8]) -> Result<(), MediaError>;

    /// Receive audio data
    pub async fn recv_audio(&self) -> Result<Vec<u8>, MediaError>;
}
```

**Tests**:
- `test_audio_track_with_quic_backend`
- `test_audio_track_send_receive`

---

### Task 6: Create GenericTrack Type

**Description**: Create a `GenericTrack` enum that unifies audio, video, and screen tracks with type-safe handling.

**Files to modify**:
- `saorsa-webrtc-core/src/media.rs`

**Specification**:
```rust
/// Unified track type for type-safe track handling
pub enum GenericTrack {
    Audio(AudioTrack),
    Video(VideoTrack),
    Screen(VideoTrack),  // Screen shares use VideoTrack with different stream type
}

impl GenericTrack {
    /// Get the track ID
    pub fn id(&self) -> &str;

    /// Get the media type
    pub fn media_type(&self) -> MediaType;

    /// Get the backend
    pub fn backend(&self) -> &Arc<dyn TrackBackend>;

    /// Check if connected
    pub fn is_connected(&self) -> bool;

    /// Get track stats
    pub fn stats(&self) -> TrackStats;
}
```

**Tests**:
- `test_generic_track_audio`
- `test_generic_track_video`
- `test_generic_track_screen`

---

### Task 7: Update MediaStreamManager for QUIC Tracks

**Description**: Update `MediaStreamManager` to create tracks with QUIC backend when a transport is provided.

**Files to modify**:
- `saorsa-webrtc-core/src/media.rs`

**Changes to MediaStreamManager**:
```rust
pub struct MediaStreamManager {
    event_sender: broadcast::Sender<MediaEvent>,
    audio_devices: Vec<AudioDevice>,
    video_devices: Vec<VideoDevice>,
    tracks: Vec<GenericTrack>,  // Changed from webrtc_tracks
    quic_transport: Option<Arc<QuicMediaTransport>>,  // New field
}

impl MediaStreamManager {
    /// Create with QUIC transport
    pub fn with_quic_transport(transport: Arc<QuicMediaTransport>) -> Self;

    /// Set QUIC transport (for existing managers)
    pub fn set_quic_transport(&mut self, transport: Arc<QuicMediaTransport>);

    /// Create audio track with appropriate backend
    pub async fn create_audio_track(&mut self) -> Result<&AudioTrack, MediaError>;

    /// Create video track with appropriate backend
    pub async fn create_video_track(&mut self) -> Result<&VideoTrack, MediaError>;

    /// Create QUIC-backed audio track (explicit)
    pub async fn create_quic_audio_track(&mut self, transport: Arc<QuicMediaTransport>) -> Result<AudioTrack, MediaError>;

    /// Create QUIC-backed video track (explicit)
    pub async fn create_quic_video_track(
        &mut self,
        transport: Arc<QuicMediaTransport>,
        width: u32,
        height: u32,
    ) -> Result<VideoTrack, MediaError>;

    /// Get all tracks
    pub fn get_tracks(&self) -> &[GenericTrack];
}
```

**Tests**:
- `test_media_manager_with_quic_transport`
- `test_create_quic_audio_track`
- `test_create_quic_video_track`
- `test_create_track_selects_correct_backend`

---

### Task 8: Update Call Struct for QUIC Tracks

**Description**: Update the `Call` struct in `call.rs` to use QUIC-backed tracks.

**Files to modify**:
- `saorsa-webrtc-core/src/call.rs`

**Changes**:
```rust
pub struct Call<I: PeerIdentity> {
    pub id: CallId,
    pub remote_peer: I,
    pub peer_connection: Arc<RTCPeerConnection>,  // Keep for legacy
    pub media_transport: Option<Arc<QuicMediaTransport>>,
    pub state: CallState,
    pub constraints: MediaConstraints,
    // Change from webrtc_tracks to generic tracks
    pub tracks: Vec<GenericTrack>,  // Changed from Vec<WebRtcTrack>
}
```

**Update `initiate_quic_call`**:
- Create tracks using `MediaStreamManager::create_quic_*_track()` methods
- Associate tracks with the `QuicMediaTransport`

**Tests**:
- `test_initiate_quic_call_creates_quic_tracks`
- `test_quic_call_tracks_use_transport`

---

### Task 9: Add Track Stream Binding

**Description**: Add functionality to bind tracks to specific QUIC streams and manage stream lifecycle.

**Files to modify**:
- `saorsa-webrtc-core/src/media.rs`

**Specification**:
```rust
impl QuicTrackBackend {
    /// Ensure the stream is open for this track
    pub async fn ensure_stream(&self) -> Result<(), MediaError>;

    /// Close the stream for this track
    pub async fn close_stream(&self) -> Result<(), MediaError>;

    /// Check if stream is open
    pub async fn is_stream_open(&self) -> bool;
}

/// Extension methods for GenericTrack
impl GenericTrack {
    /// Open the underlying stream (QUIC only)
    pub async fn open(&self) -> Result<(), MediaError>;

    /// Close the underlying stream
    pub async fn close(&self) -> Result<(), MediaError>;
}
```

**Tests**:
- `test_quic_track_stream_lifecycle`
- `test_track_open_close`

---

### Task 10: Integration Tests

**Description**: Add integration tests verifying end-to-end track creation and media flow.

**Files to create/modify**:
- `saorsa-webrtc-core/tests/track_backend_integration.rs`

**Test scenarios**:
1. Create QUIC transport and tracks
2. Send audio data through AudioTrack → QuicTrackBackend → QuicMediaTransport
3. Send video data through VideoTrack → QuicTrackBackend → QuicMediaTransport
4. Verify statistics propagation
5. Test track cleanup on disconnect

---

## Success Criteria

1. All tracks can use either QUIC or legacy WebRTC backend
2. `MediaStreamManager` creates QUIC-backed tracks when transport is available
3. `Call` struct uses `GenericTrack` instead of `WebRtcTrack`
4. Zero compilation errors
5. Zero compilation warnings
6. All existing tests pass
7. New tests for all new functionality
8. 100% test pass rate

## Notes

- Maintain backward compatibility with legacy-webrtc feature
- Keep codec integration (OpenH264/Opus) unchanged
- Ensure `TrackBackend` is object-safe for dynamic dispatch
- Use `async_trait` crate for async trait methods
