//! OpenH264 video codec.
//!
//! With the `h264` feature enabled (**off by default** — video is opt-in,
//! see `docs/design/revival-v0-v1.md` and `LICENSING-H264.md`),
//! [`OpenH264Encoder`] and [`OpenH264Decoder`] wrap the real Cisco OpenH264
//! codec via the `openh264` crate (compiled from vendored source by
//! `openh264-sys2`): baseline-profile H.264, Annex-B NAL units, RGB8 frames
//! converted to/from I420 internally.
//!
//! The H.264 bitstream carries no wall-clock timestamps at this layer;
//! [`VideoFrame::timestamp`] travels out-of-band (RTP or the QUIC media
//! framing), so decoded frames report `timestamp = 0` and callers restamp
//! from their transport — the same contract as the Opus codec.
//!
//! A legacy pass-through simulation is kept for tests only, behind
//! `#[cfg(any(test, feature = "stub-codecs"))]` in [`stub`]. It performs no
//! real video coding and must never be used for production video.

#[cfg(feature = "h264")]
use crate::{CodecError, Result, VideoDecoder, VideoEncoder, VideoFrame};
#[cfg(feature = "h264")]
use bytes::Bytes;

#[cfg(feature = "h264")]
use crate::{MAX_HEIGHT, MAX_RGB_SIZE, MAX_WIDTH};

/// OpenH264 encoder configuration.
///
/// `width`/`height` fix the frame geometry (frames with other dimensions
/// are rejected with `CodecError::DimensionMismatch`); `bitrate_bps`
/// drives the encoder's rate control; `max_fps` is a rate-control hint.
#[derive(Debug, Clone)]
pub struct OpenH264EncoderConfig {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Target bitrate in bits per second.
    pub bitrate_bps: u32,
    /// Maximum frame rate hint for rate control.
    pub max_fps: f32,
}

impl Default for OpenH264EncoderConfig {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
            bitrate_bps: 512_000,
            max_fps: 30.0,
        }
    }
}

/// Validate dimensions against the crate-wide safety bounds and return the
/// exact RGB8 byte length a frame of this geometry must have.
#[cfg(feature = "h264")]
fn checked_rgb_len(width: u32, height: u32) -> crate::Result<usize> {
    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err(crate::CodecError::InvalidDimensions(width, height));
    }
    let rgb_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(3))
        .ok_or(crate::CodecError::Overflow)?;
    if rgb_len > MAX_RGB_SIZE {
        return Err(crate::CodecError::SizeExceeded {
            actual: rgb_len,
            max: MAX_RGB_SIZE,
        });
    }
    Ok(rgb_len)
}

/// OpenH264 video encoder (real Cisco OpenH264 via the `openh264` crate).
///
/// Frames are RGB8 (`width * height * 3` bytes) and are converted to I420
/// internally. Output is an Annex-B H.264 access unit; the first encoded
/// frame (and the next frame after
/// [`request_keyframe`](VideoEncoder::request_keyframe)) contains
/// SPS/PPS + IDR NAL units.
#[cfg(feature = "h264")]
pub struct OpenH264Encoder {
    inner: Option<openh264::encoder::Encoder>,
    config: OpenH264EncoderConfig,
    rgb_len: usize,
    pending_keyframe: bool,
}

#[cfg(feature = "h264")]
impl OpenH264Encoder {
    /// Create an encoder with the default 640×480 geometry.
    pub fn new() -> Result<Self> {
        Self::with_config(OpenH264EncoderConfig::default())
    }

    /// Create an encoder for the given frame geometry with default rate settings.
    pub fn with_dimensions(width: u32, height: u32) -> Result<Self> {
        Self::with_config(OpenH264EncoderConfig {
            width,
            height,
            ..OpenH264EncoderConfig::default()
        })
    }

    /// Create an encoder from a full configuration.
    pub fn with_config(config: OpenH264EncoderConfig) -> Result<Self> {
        let rgb_len = checked_rgb_len(config.width, config.height)?;
        let inner = Self::build_inner(&config)?;
        Ok(Self {
            inner: Some(inner),
            config,
            rgb_len,
            pending_keyframe: false,
        })
    }

    fn build_inner(config: &OpenH264EncoderConfig) -> Result<openh264::encoder::Encoder> {
        let api = openh264::OpenH264API::from_source();
        let enc_config = openh264::encoder::EncoderConfig::new()
            .bitrate(openh264::encoder::BitRate::from_bps(config.bitrate_bps))
            .max_frame_rate(openh264::encoder::FrameRate::from_hz(config.max_fps));
        openh264::encoder::Encoder::with_api_config(api, enc_config)
            .map_err(|e| CodecError::InitFailed(e.to_string()))
    }
}

#[cfg(feature = "h264")]
impl VideoEncoder for OpenH264Encoder {
    fn encode(&mut self, frame: &VideoFrame) -> Result<Bytes> {
        if frame.width != self.config.width || frame.height != self.config.height {
            return Err(CodecError::DimensionMismatch {
                frame_width: frame.width,
                frame_height: frame.height,
                cfg_width: self.config.width,
                cfg_height: self.config.height,
            });
        }
        if frame.data.len() != self.rgb_len {
            return Err(CodecError::InvalidData(
                "frame data length is not width * height * 3 (RGB8)",
            ));
        }

        // A keyframe request is honoured by rebuilding the encoder: the
        // openh264 crate exposes ForceIntraFrame only through its unsafe raw
        // API (this crate denies `unsafe_code`), and a fresh encoder's first
        // output is always SPS/PPS + IDR. Keyframe requests are rare, so the
        // rebuild cost is acceptable.
        if self.pending_keyframe || self.inner.is_none() {
            self.inner = Some(Self::build_inner(&self.config)?);
            self.pending_keyframe = false;
        }

        let rgb = openh264::formats::RgbSliceU8::new(
            &frame.data,
            (self.config.width as usize, self.config.height as usize),
        );
        let yuv = openh264::formats::YUVBuffer::from_rgb8_source(rgb);

        let encoder = self
            .inner
            .as_mut()
            .ok_or(CodecError::InvalidData("encoder not initialized"))?;
        let bitstream = encoder
            .encode(&yuv)
            .map_err(|e| CodecError::EncodeFailed(e.to_string()))?;
        Ok(Bytes::from(bitstream.to_vec()))
    }

    fn request_keyframe(&mut self) {
        self.pending_keyframe = true;
    }
}

/// OpenH264 video decoder (real Cisco OpenH264 via the `openh264` crate).
///
/// Accepts Annex-B H.264 access units and yields RGB8 [`VideoFrame`]s with
/// `timestamp = 0` (timestamps travel out-of-band; see the module docs).
#[cfg(feature = "h264")]
pub struct OpenH264Decoder {
    inner: openh264::decoder::Decoder,
}

#[cfg(feature = "h264")]
impl OpenH264Decoder {
    /// Create a decoder.
    pub fn new() -> Result<Self> {
        let api = openh264::OpenH264API::from_source();
        let inner = openh264::decoder::Decoder::with_api_config(
            api,
            openh264::decoder::DecoderConfig::new(),
        )
        .map_err(|e| CodecError::InitFailed(e.to_string()))?;
        Ok(Self { inner })
    }
}

#[cfg(feature = "h264")]
impl VideoDecoder for OpenH264Decoder {
    fn decode(&mut self, data: &[u8]) -> Result<VideoFrame> {
        use openh264::formats::YUVSource;
        let decoded = self
            .inner
            .decode(data)
            .map_err(|e| CodecError::DecodeFailed(e.to_string()))?;
        let yuv = decoded.ok_or_else(|| {
            CodecError::DecodeFailed("no frame ready — decoder needs more NAL data".to_string())
        })?;

        let (width, height) = yuv.dimensions();
        let rgb_len = checked_rgb_len(width as u32, height as u32)?;
        let mut rgb = vec![0u8; rgb_len];
        yuv.write_rgb8(&mut rgb);

        Ok(VideoFrame {
            data: rgb,
            width: width as u32,
            height: height as u32,
            timestamp: 0,
        })
    }
}

/// Test-only pass-through simulation (the pre-revival "codec").
///
/// Performs RLE-ish size reduction and carries dimensions + timestamp in a
/// 16-byte header. **It is not H.264** and produces bitstreams nothing else
/// can decode. Kept solely so transport-layer tests can move frame-shaped
/// bytes without paying for a real codec build.
#[cfg(any(test, feature = "stub-codecs"))]
pub mod stub {
    use crate::{CodecError, Result, VideoDecoder, VideoEncoder, VideoFrame};
    use crate::{MAX_HEIGHT, MAX_RGB_SIZE, MAX_WIDTH};
    use bytes::Bytes;

    const HEADER_SIZE: usize = 16;

    /// Stub encoder (pass-through simulation; not H.264).
    pub struct StubOpenH264Encoder {
        width: u32,
        height: u32,
        pending_keyframe: bool,
    }

    impl StubOpenH264Encoder {
        /// Create a stub encoder with the default 640×480 geometry.
        pub fn new() -> Result<Self> {
            Self::with_dimensions(640, 480)
        }

        /// Create a stub encoder for the given geometry.
        pub fn with_dimensions(width: u32, height: u32) -> Result<Self> {
            if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
                return Err(CodecError::InvalidDimensions(width, height));
            }
            let rgb_size = width
                .checked_mul(height)
                .and_then(|px| px.checked_mul(3))
                .ok_or(CodecError::Overflow)?;
            if rgb_size as usize > MAX_RGB_SIZE {
                return Err(CodecError::SizeExceeded {
                    actual: rgb_size as usize,
                    max: MAX_RGB_SIZE,
                });
            }
            Ok(Self {
                width,
                height,
                pending_keyframe: false,
            })
        }

        /// Whether a keyframe request is pending (test observability).
        pub fn pending_keyframe(&self) -> bool {
            self.pending_keyframe
        }
    }

    impl VideoEncoder for StubOpenH264Encoder {
        fn encode(&mut self, frame: &VideoFrame) -> Result<Bytes> {
            if frame.width != self.width || frame.height != self.height {
                return Err(CodecError::DimensionMismatch {
                    frame_width: frame.width,
                    frame_height: frame.height,
                    cfg_width: self.width,
                    cfg_height: self.height,
                });
            }

            let original_size = frame.data.len();
            let compressed_size = original_size / 4;

            let mut compressed = Vec::with_capacity(compressed_size + HEADER_SIZE);
            compressed.extend_from_slice(&frame.width.to_le_bytes());
            compressed.extend_from_slice(&frame.height.to_le_bytes());
            compressed.extend_from_slice(&frame.timestamp.to_le_bytes());

            let mut i = 0;
            while i < frame.data.len() && compressed.len() < compressed_size {
                let mut count = 1;
                while i + count < frame.data.len()
                    && frame.data[i + count] == frame.data[i]
                    && count < 255
                {
                    count += 1;
                }
                compressed.push(count as u8);
                compressed.push(frame.data[i]);
                i += count;
            }

            self.pending_keyframe = false;
            Ok(Bytes::from(compressed))
        }

        fn request_keyframe(&mut self) {
            self.pending_keyframe = true;
        }
    }

    /// Stub decoder (parses the stub's 16-byte header; not H.264).
    pub struct StubOpenH264Decoder;

    impl StubOpenH264Decoder {
        /// Create a stub decoder.
        pub fn new() -> Result<Self> {
            Ok(Self)
        }
    }

    impl VideoDecoder for StubOpenH264Decoder {
        fn decode(&mut self, data: &[u8]) -> Result<VideoFrame> {
            if data.len() < HEADER_SIZE {
                return Err(CodecError::InvalidData("data too small for header"));
            }

            let width = u32::from_le_bytes(
                data.get(0..4)
                    .and_then(|s| s.try_into().ok())
                    .ok_or(CodecError::InvalidData("bad width bytes"))?,
            );
            let height = u32::from_le_bytes(
                data.get(4..8)
                    .and_then(|s| s.try_into().ok())
                    .ok_or(CodecError::InvalidData("bad height bytes"))?,
            );
            let timestamp = u64::from_le_bytes(
                data.get(8..16)
                    .and_then(|s| s.try_into().ok())
                    .ok_or(CodecError::InvalidData("bad timestamp bytes"))?,
            );

            if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
                return Err(CodecError::InvalidDimensions(width, height));
            }
            let rgb_size = (width as usize)
                .checked_mul(height as usize)
                .and_then(|px| px.checked_mul(3))
                .ok_or(CodecError::Overflow)?;
            if rgb_size > MAX_RGB_SIZE {
                return Err(CodecError::SizeExceeded {
                    actual: rgb_size,
                    max: MAX_RGB_SIZE,
                });
            }

            let mut rgb = Vec::with_capacity(rgb_size);
            let mut i = HEADER_SIZE;
            while i + 1 < data.len() && rgb.len() < rgb_size {
                let count = data[i] as usize;
                let value = data[i + 1];
                let take = count.min(rgb_size - rgb.len());
                rgb.extend(std::iter::repeat_n(value, take));
                i += 2;
            }
            rgb.resize(rgb_size, 0);

            Ok(VideoFrame {
                data: rgb,
                width,
                height,
                timestamp,
            })
        }
    }

    #[cfg(test)]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    mod tests {
        use super::*;

        fn frame(width: u32, height: u32, fill: u8, timestamp: u64) -> VideoFrame {
            VideoFrame {
                data: vec![fill; width as usize * height as usize * 3],
                width,
                height,
                timestamp,
            }
        }

        #[test]
        fn stub_round_trip_preserves_metadata() {
            let mut encoder = StubOpenH264Encoder::with_dimensions(320, 240).unwrap();
            let mut decoder = StubOpenH264Decoder::new().unwrap();
            let f = frame(320, 240, 128, 9_876_543_210);
            let compressed = encoder.encode(&f).unwrap();
            let decoded = decoder.decode(&compressed).unwrap();
            assert_eq!(decoded.width, 320);
            assert_eq!(decoded.height, 240);
            assert_eq!(decoded.timestamp, 9_876_543_210);
            assert_eq!(decoded.data.len(), f.data.len());
        }

        #[test]
        fn stub_encoder_dimension_mismatch() {
            let mut encoder = StubOpenH264Encoder::new().unwrap();
            let f = frame(320, 240, 0, 0);
            assert!(matches!(
                encoder.encode(&f),
                Err(CodecError::DimensionMismatch { .. })
            ));
        }

        #[test]
        fn stub_decoder_rejects_short_and_bad_headers() {
            let mut decoder = StubOpenH264Decoder::new().unwrap();
            assert!(decoder.decode(&[0u8; 10]).is_err());

            let mut zero_width = Vec::new();
            zero_width.extend_from_slice(&0u32.to_le_bytes());
            zero_width.extend_from_slice(&480u32.to_le_bytes());
            zero_width.extend_from_slice(&1234u64.to_le_bytes());
            assert!(decoder.decode(&zero_width).is_err());

            let mut oversized = Vec::new();
            oversized.extend_from_slice(&(MAX_WIDTH + 1).to_le_bytes());
            oversized.extend_from_slice(&1080u32.to_le_bytes());
            oversized.extend_from_slice(&1234u64.to_le_bytes());
            assert!(decoder.decode(&oversized).is_err());
        }

        #[test]
        fn stub_keyframe_flag_lifecycle() {
            let mut encoder = StubOpenH264Encoder::new().unwrap();
            assert!(!encoder.pending_keyframe());
            encoder.request_keyframe();
            assert!(encoder.pending_keyframe());
            let f = frame(640, 480, 128, 0);
            encoder.encode(&f).unwrap();
            assert!(!encoder.pending_keyframe());
        }

        #[test]
        fn stub_compresses_uniform_frames() {
            let mut encoder = StubOpenH264Encoder::new().unwrap();
            let f = frame(640, 480, 128, 12345);
            let compressed = encoder.encode(&f).unwrap();
            assert!(!compressed.is_empty());
            assert!(compressed.len() < f.data.len());
        }
    }
}

#[cfg(all(test, feature = "h264"))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::{VideoEncoder, VideoFrame};

    #[test]
    fn real_encoder_rejects_mismatched_dimensions() {
        let mut encoder = OpenH264Encoder::with_dimensions(320, 240).unwrap();
        let frame = VideoFrame {
            data: vec![0; 640 * 480 * 3],
            width: 640,
            height: 480,
            timestamp: 0,
        };
        assert!(matches!(
            encoder.encode(&frame),
            Err(CodecError::DimensionMismatch { .. })
        ));
    }

    #[test]
    fn real_encoder_rejects_wrong_buffer_length() {
        let mut encoder = OpenH264Encoder::with_dimensions(320, 240).unwrap();
        let frame = VideoFrame {
            data: vec![0; 100],
            width: 320,
            height: 240,
            timestamp: 0,
        };
        assert!(matches!(
            encoder.encode(&frame),
            Err(CodecError::InvalidData(_))
        ));
    }

    #[test]
    fn real_config_validation() {
        assert!(matches!(
            OpenH264Encoder::with_dimensions(0, 480),
            Err(CodecError::InvalidDimensions(_, _))
        ));
        assert!(matches!(
            OpenH264Encoder::with_dimensions(MAX_WIDTH + 1, 480),
            Err(CodecError::InvalidDimensions(_, _))
        ));
    }
}
