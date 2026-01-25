//! OpenH264 codec implementation
//!
//! # ⚠️ STUB IMPLEMENTATION
//!
//! This is currently a **simulation implementation** for development and testing.
//! It uses simple compression to simulate codec behavior (~25% size reduction).
//!
//! **Not suitable for production video calls.**
//!
//! For production use, replace with actual openh264 integration using the
//! openh264-sys bindings or similar library.

use crate::{CodecError, Result, VideoDecoder, VideoEncoder, VideoFrame};
use crate::{MAX_HEIGHT, MAX_RGB_SIZE, MAX_WIDTH};
use bytes::Bytes;

const HEADER_SIZE: usize = 16;

/// OpenH264 video encoder (stub/simulation implementation)
pub struct OpenH264Encoder {
    width: u32,
    height: u32,
    pending_keyframe: bool,
}

impl OpenH264Encoder {
    pub fn new() -> Result<Self> {
        Self::with_dimensions(640, 480)
    }

    pub fn with_dimensions(width: u32, height: u32) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(CodecError::InvalidDimensions(width, height));
        }
        if width > MAX_WIDTH || height > MAX_HEIGHT {
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
}

impl VideoEncoder for OpenH264Encoder {
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
                && frame.data[i] == frame.data[i + count]
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

/// OpenH264 video decoder (stub implementation for now)
pub struct OpenH264Decoder;

impl OpenH264Decoder {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

impl VideoDecoder for OpenH264Decoder {
    fn decode(&mut self, data: &[u8]) -> Result<VideoFrame> {
        if data.len() < HEADER_SIZE {
            return Err(CodecError::InvalidData("data too small for header"));
        }

        let width_bytes: [u8; 4] = data
            .get(0..4)
            .and_then(|s| s.try_into().ok())
            .ok_or(CodecError::InvalidData("missing width"))?;
        let width = u32::from_le_bytes(width_bytes);

        let height_bytes: [u8; 4] = data
            .get(4..8)
            .and_then(|s| s.try_into().ok())
            .ok_or(CodecError::InvalidData("missing height"))?;
        let height = u32::from_le_bytes(height_bytes);

        let timestamp_bytes: [u8; 8] = data
            .get(8..16)
            .and_then(|s| s.try_into().ok())
            .ok_or(CodecError::InvalidData("missing timestamp"))?;
        let timestamp = u64::from_le_bytes(timestamp_bytes);

        if width == 0 || height == 0 {
            return Err(CodecError::InvalidDimensions(width, height));
        }
        if width > MAX_WIDTH || height > MAX_HEIGHT {
            return Err(CodecError::InvalidDimensions(width, height));
        }

        let expected_rgb_size = width
            .checked_mul(height)
            .and_then(|px| px.checked_mul(3))
            .ok_or(CodecError::Overflow)?;

        if expected_rgb_size as usize > MAX_RGB_SIZE {
            return Err(CodecError::SizeExceeded {
                actual: expected_rgb_size as usize,
                max: MAX_RGB_SIZE,
            });
        }

        let mut rgb_data = Vec::with_capacity(expected_rgb_size as usize);

        let mut i = HEADER_SIZE;
        while i < data.len() && rgb_data.len() < expected_rgb_size as usize {
            if i + 1 >= data.len() {
                break;
            }
            let count = data[i] as usize;
            let value = data[i + 1];
            for _ in 0..count {
                if rgb_data.len() < expected_rgb_size as usize {
                    rgb_data.push(value);
                }
            }
            i += 2;
        }

        while rgb_data.len() < expected_rgb_size as usize {
            rgb_data.push(0);
        }

        Ok(VideoFrame {
            data: rgb_data,
            width,
            height,
            timestamp,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_encoder_creation_default() {
        let result = OpenH264Encoder::new();
        assert!(result.is_ok());
        let encoder = result.unwrap();
        assert_eq!(encoder.width, 640);
        assert_eq!(encoder.height, 480);
    }

    #[test]
    fn test_encoder_creation_custom() {
        let result = OpenH264Encoder::with_dimensions(1920, 1080);
        assert!(result.is_ok());
        let encoder = result.unwrap();
        assert_eq!(encoder.width, 1920);
        assert_eq!(encoder.height, 1080);
    }

    #[test]
    fn test_encoder_zero_dimensions() {
        assert!(OpenH264Encoder::with_dimensions(0, 480).is_err());
        assert!(OpenH264Encoder::with_dimensions(640, 0).is_err());
        assert!(OpenH264Encoder::with_dimensions(0, 0).is_err());
    }

    #[test]
    fn test_encoder_oversized_dimensions() {
        assert!(OpenH264Encoder::with_dimensions(MAX_WIDTH + 1, 480).is_err());
        assert!(OpenH264Encoder::with_dimensions(640, MAX_HEIGHT + 1).is_err());
    }

    #[test]
    fn test_decoder_creation() {
        let result = OpenH264Decoder::new();
        assert!(result.is_ok());
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let mut encoder = OpenH264Encoder::new().unwrap();
        let mut decoder = OpenH264Decoder::new().unwrap();

        let original_frame = VideoFrame {
            data: vec![200; 640 * 480 * 3],
            width: 640,
            height: 480,
            timestamp: 67890,
        };

        let compressed = encoder.encode(&original_frame).unwrap();
        let decoded_frame = decoder.decode(&compressed).unwrap();

        assert_eq!(decoded_frame.width, original_frame.width);
        assert_eq!(decoded_frame.height, original_frame.height);
        assert_eq!(decoded_frame.timestamp, original_frame.timestamp);
        assert_eq!(decoded_frame.data.len(), original_frame.data.len());
    }

    #[test]
    fn test_timestamp_full_u64_roundtrip() {
        let mut encoder = OpenH264Encoder::new().unwrap();
        let mut decoder = OpenH264Decoder::new().unwrap();

        let large_timestamp = u64::MAX - 1000;
        let frame = VideoFrame {
            data: vec![128; 640 * 480 * 3],
            width: 640,
            height: 480,
            timestamp: large_timestamp,
        };

        let compressed = encoder.encode(&frame).unwrap();
        let decoded = decoder.decode(&compressed).unwrap();

        assert_eq!(decoded.timestamp, large_timestamp);
    }

    #[test]
    fn test_encoder_dimension_mismatch() {
        let mut encoder = OpenH264Encoder::new().unwrap();

        let frame = VideoFrame {
            data: vec![0; 320 * 240 * 3],
            width: 320,
            height: 240,
            timestamp: 0,
        };

        let result = encoder.encode(&frame);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CodecError::DimensionMismatch { .. }
        ));
    }

    #[test]
    fn test_decoder_corrupted_header_too_small() {
        let mut decoder = OpenH264Decoder::new().unwrap();
        let corrupted_data = vec![0u8; 10];
        let result = decoder.decode(&corrupted_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decoder_invalid_dimensions() {
        let mut decoder = OpenH264Decoder::new().unwrap();

        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&480u32.to_le_bytes());
        data.extend_from_slice(&1234u64.to_le_bytes());

        let result = decoder.decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decoder_oversized_dimensions() {
        let mut decoder = OpenH264Decoder::new().unwrap();

        let mut data = Vec::new();
        data.extend_from_slice(&(MAX_WIDTH + 1).to_le_bytes());
        data.extend_from_slice(&1080u32.to_le_bytes());
        data.extend_from_slice(&1234u64.to_le_bytes());

        let result = decoder.decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decoder_random_noise() {
        let mut decoder = OpenH264Decoder::new().unwrap();

        let mut data = Vec::new();
        data.extend_from_slice(&640u32.to_le_bytes());
        data.extend_from_slice(&480u32.to_le_bytes());
        data.extend_from_slice(&1234u64.to_le_bytes());
        for i in 0..100 {
            data.push(i as u8);
        }

        let result = decoder.decode(&data);
        assert!(result.is_ok());
    }

    #[test]
    fn test_encoder_compression() {
        let mut encoder = OpenH264Encoder::new().unwrap();

        let frame = VideoFrame {
            data: vec![128; 640 * 480 * 3],
            width: 640,
            height: 480,
            timestamp: 12345,
        };

        let compressed = encoder.encode(&frame).unwrap();
        assert!(!compressed.is_empty());
        assert!(compressed.len() < frame.data.len());
    }

    #[test]
    fn test_keyframe_request() {
        let mut encoder = OpenH264Encoder::new().unwrap();
        assert!(!encoder.pending_keyframe);

        encoder.request_keyframe();
        assert!(encoder.pending_keyframe);

        let frame = VideoFrame {
            data: vec![128; 640 * 480 * 3],
            width: 640,
            height: 480,
            timestamp: 0,
        };

        let _ = encoder.encode(&frame).unwrap();
        assert!(!encoder.pending_keyframe);
    }

    #[test]
    fn test_encode_varied_content() {
        let mut encoder = OpenH264Encoder::new().unwrap();
        let mut decoder = OpenH264Decoder::new().unwrap();

        let mut varied_data = Vec::new();
        for i in 0..(640 * 480 * 3) {
            varied_data.push((i % 256) as u8);
        }

        let frame = VideoFrame {
            data: varied_data,
            width: 640,
            height: 480,
            timestamp: 99999,
        };

        let compressed = encoder.encode(&frame).unwrap();
        let decoded = decoder.decode(&compressed).unwrap();

        assert_eq!(decoded.width, frame.width);
        assert_eq!(decoded.height, frame.height);
        assert_eq!(decoded.timestamp, frame.timestamp);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_encode_decode_preserves_metadata(
            width in 1u32..=1920,
            height in 1u32..=1080,
            timestamp in any::<u64>(),
            seed in 0u8..=255,
        ) {
            let width = width.clamp(1, MAX_WIDTH);
            let height = height.clamp(1, MAX_HEIGHT);

            let pixel_count = (width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(3));

            if let Some(size) = pixel_count {
                if size <= MAX_RGB_SIZE {
                    let data = vec![seed; size];
                    let frame = VideoFrame { data, width, height, timestamp };

                    let mut encoder = OpenH264Encoder::with_dimensions(width, height)?;
                    let mut decoder = OpenH264Decoder::new()?;

                    let compressed = encoder.encode(&frame)?;
                    let decoded = decoder.decode(&compressed)?;

                    prop_assert_eq!(decoded.width, width);
                    prop_assert_eq!(decoded.height, height);
                    prop_assert_eq!(decoded.timestamp, timestamp);
                    prop_assert_eq!(decoded.data.len(), size);
                }
            }
        }

        #[test]
        fn prop_decoder_handles_arbitrary_compressed_data(
            width in 1u32..=1920,
            height in 1u32..=1080,
            timestamp in any::<u64>(),
            data_len in 0usize..=1000,
            seed in any::<u64>(),
        ) {
            let width = width.clamp(1, MAX_WIDTH);
            let height = height.clamp(1, MAX_HEIGHT);

            let mut data = Vec::new();
            data.extend_from_slice(&width.to_le_bytes());
            data.extend_from_slice(&height.to_le_bytes());
            data.extend_from_slice(&timestamp.to_le_bytes());

            let mut rng_val = seed;
            for _ in 0..data_len {
                rng_val = rng_val.wrapping_mul(1103515245).wrapping_add(12345);
                data.push((rng_val >> 16) as u8);
            }

            if let Ok(mut decoder) = OpenH264Decoder::new() {
                let _ = decoder.decode(&data);
            }
        }

        #[test]
        fn prop_encoder_rejects_mismatched_dimensions(
            cfg_w in 1u32..=640,
            cfg_h in 1u32..=480,
            frame_w in 1u32..=640,
            frame_h in 1u32..=480,
        ) {
            if cfg_w != frame_w || cfg_h != frame_h {
                let size = (frame_w as usize * frame_h as usize * 3).min(MAX_RGB_SIZE);
                let frame = VideoFrame {
                    data: vec![128; size],
                    width: frame_w,
                    height: frame_h,
                    timestamp: 0,
                };

                let mut encoder = OpenH264Encoder::with_dimensions(cfg_w, cfg_h)?;
                let result = encoder.encode(&frame);
                prop_assert!(result.is_err());
            }
        }

        #[test]
        fn prop_keyframe_flag_cleared_after_encode(
            width in 1u32..=640,
            height in 1u32..=480,
        ) {
            let size = width as usize * height as usize * 3;
            let frame = VideoFrame {
                data: vec![128; size],
                width,
                height,
                timestamp: 0,
            };

            let mut encoder = OpenH264Encoder::with_dimensions(width, height)?;
            encoder.request_keyframe();
            prop_assert!(encoder.pending_keyframe);

            let _ = encoder.encode(&frame)?;
            prop_assert!(!encoder.pending_keyframe);
        }
    }
}
