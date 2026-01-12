//! Opus audio codec implementation
//!
//! # ⚠️ STUB IMPLEMENTATION
//!
//! This is currently a **simulation implementation** for development and testing.
//! It validates frame sizes and formats but doesn't perform real audio compression.
//!
//! **Not suitable for production audio calls.**
//!
//! For production use, replace with actual libopus integration using the
//! opus crate or similar library.

use crate::{CodecError, Result};
use bytes::Bytes;

/// Opus audio sample rates (Hz)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleRate {
    Hz8000 = 8000,
    Hz12000 = 12000,
    Hz16000 = 16000,
    Hz24000 = 24000,
    Hz48000 = 48000,
}

impl SampleRate {
    pub fn as_hz(&self) -> u32 {
        *self as u32
    }
}

/// Audio channels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channels {
    Mono = 1,
    Stereo = 2,
}

impl Channels {
    pub fn count(&self) -> usize {
        *self as usize
    }
}

/// Audio frame for encoding/decoding
#[derive(Debug, Clone)]
pub struct AudioFrame {
    /// PCM audio data (16-bit signed samples)
    pub data: Vec<i16>,
    /// Sample rate in Hz
    pub sample_rate: SampleRate,
    /// Number of channels
    pub channels: Channels,
    /// Timestamp in milliseconds
    pub timestamp: u64,
}

/// Opus audio encoder configuration
#[derive(Debug, Clone)]
pub struct OpusEncoderConfig {
    pub sample_rate: SampleRate,
    pub channels: Channels,
    /// Bitrate in bits per second (6000 - 510000)
    pub bitrate: u32,
}

impl Default for OpusEncoderConfig {
    fn default() -> Self {
        Self {
            sample_rate: SampleRate::Hz48000,
            channels: Channels::Mono,
            bitrate: 64000, // 64 kbps
        }
    }
}

/// Opus audio encoder (stub implementation)
pub struct OpusEncoder {
    config: OpusEncoderConfig,
}

impl OpusEncoder {
    pub fn new(config: OpusEncoderConfig) -> Result<Self> {
        // Validate bitrate
        if config.bitrate < 6000 || config.bitrate > 510000 {
            return Err(CodecError::InvalidData(
                "bitrate out of range (6000-510000)",
            ));
        }

        Ok(Self { config })
    }

    /// Encode PCM audio data to Opus
    pub fn encode(&mut self, frame: &AudioFrame) -> Result<Bytes> {
        // Validate frame matches encoder config
        if frame.sample_rate != self.config.sample_rate {
            return Err(CodecError::InvalidData("sample rate mismatch"));
        }
        if frame.channels != self.config.channels {
            return Err(CodecError::InvalidData("channel count mismatch"));
        }

        // Validate frame size (must have data)
        if frame.data.is_empty() {
            return Err(CodecError::InvalidData("empty audio frame"));
        }

        // TODO: Replace with real Opus encoding
        // For now, create a simple compressed representation
        let mut compressed = Vec::new();

        // Header: sample_rate (4 bytes), channels (1 byte), timestamp (8 bytes)
        compressed.extend_from_slice(&self.config.sample_rate.as_hz().to_le_bytes());
        compressed.push(self.config.channels.count() as u8);
        compressed.extend_from_slice(&frame.timestamp.to_le_bytes());

        // Stub compression: store length and simple RLE
        let data_len = frame.data.len() as u32;
        compressed.extend_from_slice(&data_len.to_le_bytes());

        // Simple compression for testing
        let bytes: Vec<u8> = frame.data.iter().flat_map(|s| s.to_le_bytes()).collect();
        compressed.extend_from_slice(&bytes);

        Ok(Bytes::from(compressed))
    }
}

/// Opus audio decoder (stub implementation)
pub struct OpusDecoder {
    #[allow(dead_code)]
    sample_rate: SampleRate,
    #[allow(dead_code)]
    channels: Channels,
}

impl OpusDecoder {
    pub fn new(sample_rate: SampleRate, channels: Channels) -> Result<Self> {
        Ok(Self {
            sample_rate,
            channels,
        })
    }

    /// Decode Opus data to PCM audio
    pub fn decode(&mut self, data: &[u8]) -> Result<AudioFrame> {
        // Minimum size: 4 (sample_rate) + 1 (channels) + 8 (timestamp) + 4 (length)
        const HEADER_SIZE: usize = 17;

        if data.len() < HEADER_SIZE {
            return Err(CodecError::InvalidData("opus data too small"));
        }

        // Parse header
        let sample_rate_hz = u32::from_le_bytes(
            data.get(0..4)
                .and_then(|s| s.try_into().ok())
                .ok_or(CodecError::InvalidData("invalid sample rate"))?,
        );

        let sample_rate = match sample_rate_hz {
            8000 => SampleRate::Hz8000,
            12000 => SampleRate::Hz12000,
            16000 => SampleRate::Hz16000,
            24000 => SampleRate::Hz24000,
            48000 => SampleRate::Hz48000,
            _ => return Err(CodecError::InvalidData("unsupported sample rate")),
        };

        let channel_count = data[4];
        let channels = match channel_count {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            _ => return Err(CodecError::InvalidData("invalid channel count")),
        };

        let timestamp = u64::from_le_bytes(
            data.get(5..13)
                .and_then(|s| s.try_into().ok())
                .ok_or(CodecError::InvalidData("invalid timestamp"))?,
        );

        let data_len = u32::from_le_bytes(
            data.get(13..17)
                .and_then(|s| s.try_into().ok())
                .ok_or(CodecError::InvalidData("invalid data length"))?,
        ) as usize;

        // Parse PCM data
        let pcm_bytes = data
            .get(HEADER_SIZE..)
            .ok_or(CodecError::InvalidData("missing pcm data"))?;

        let mut pcm_data = Vec::with_capacity(data_len);
        for chunk in pcm_bytes.chunks_exact(2) {
            if let Ok(bytes) = chunk.try_into() {
                pcm_data.push(i16::from_le_bytes(bytes));
            }
        }

        // Validate we got the expected amount of data
        if pcm_data.len() != data_len {
            return Err(CodecError::InvalidData("pcm data length mismatch"));
        }

        Ok(AudioFrame {
            data: pcm_data,
            sample_rate,
            channels,
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
        let config = OpusEncoderConfig::default();
        let result = OpusEncoder::new(config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_encoder_creation_custom() {
        let config = OpusEncoderConfig {
            sample_rate: SampleRate::Hz16000,
            channels: Channels::Stereo,
            bitrate: 96000,
        };
        let result = OpusEncoder::new(config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_encoder_invalid_bitrate() {
        let config = OpusEncoderConfig {
            bitrate: 5000, // Too low
            ..Default::default()
        };
        assert!(OpusEncoder::new(config).is_err());

        let config = OpusEncoderConfig {
            bitrate: 520000, // Too high
            ..Default::default()
        };
        assert!(OpusEncoder::new(config).is_err());
    }

    #[test]
    fn test_decoder_creation() {
        let result = OpusDecoder::new(SampleRate::Hz48000, Channels::Mono);
        assert!(result.is_ok());
    }

    #[test]
    fn test_encode_decode_roundtrip_mono() {
        let config = OpusEncoderConfig::default();
        let mut encoder = OpusEncoder::new(config).unwrap();
        let mut decoder = OpusDecoder::new(SampleRate::Hz48000, Channels::Mono).unwrap();

        // Create test audio (1 second at 48kHz, sine wave)
        let samples = 48000;
        let mut audio_data = Vec::with_capacity(samples);
        for i in 0..samples {
            let sample = (((i as f32) * 440.0 * 2.0 * std::f32::consts::PI / 48000.0).sin()
                * 16000.0) as i16;
            audio_data.push(sample);
        }

        let frame = AudioFrame {
            data: audio_data.clone(),
            sample_rate: SampleRate::Hz48000,
            channels: Channels::Mono,
            timestamp: 1000,
        };

        let compressed = encoder.encode(&frame).unwrap();
        let decoded = decoder.decode(&compressed).unwrap();

        assert_eq!(decoded.sample_rate, frame.sample_rate);
        assert_eq!(decoded.channels, frame.channels);
        assert_eq!(decoded.timestamp, frame.timestamp);
        assert_eq!(decoded.data.len(), frame.data.len());
        assert_eq!(decoded.data, frame.data);
    }

    #[test]
    fn test_encode_decode_roundtrip_stereo() {
        let config = OpusEncoderConfig {
            sample_rate: SampleRate::Hz48000,
            channels: Channels::Stereo,
            bitrate: 128000,
        };
        let mut encoder = OpusEncoder::new(config).unwrap();
        let mut decoder = OpusDecoder::new(SampleRate::Hz48000, Channels::Stereo).unwrap();

        // Create stereo test audio (interleaved L/R)
        let samples = 96000; // 1 second stereo at 48kHz (2 channels)
        let audio_data: Vec<i16> = (0..samples).map(|i| (i % 1000) as i16).collect();

        let frame = AudioFrame {
            data: audio_data,
            sample_rate: SampleRate::Hz48000,
            channels: Channels::Stereo,
            timestamp: 2000,
        };

        let compressed = encoder.encode(&frame).unwrap();
        let decoded = decoder.decode(&compressed).unwrap();

        assert_eq!(decoded.sample_rate, frame.sample_rate);
        assert_eq!(decoded.channels, frame.channels);
        assert_eq!(decoded.timestamp, frame.timestamp);
        assert_eq!(decoded.data, frame.data);
    }

    #[test]
    fn test_encoder_sample_rate_mismatch() {
        let config = OpusEncoderConfig {
            sample_rate: SampleRate::Hz48000,
            ..Default::default()
        };
        let mut encoder = OpusEncoder::new(config).unwrap();

        let frame = AudioFrame {
            data: vec![0; 100],
            sample_rate: SampleRate::Hz16000, // Mismatch!
            channels: Channels::Mono,
            timestamp: 0,
        };

        assert!(encoder.encode(&frame).is_err());
    }

    #[test]
    fn test_encoder_channel_mismatch() {
        let config = OpusEncoderConfig {
            channels: Channels::Mono,
            ..Default::default()
        };
        let mut encoder = OpusEncoder::new(config).unwrap();

        let frame = AudioFrame {
            data: vec![0; 100],
            sample_rate: SampleRate::Hz48000,
            channels: Channels::Stereo, // Mismatch!
            timestamp: 0,
        };

        assert!(encoder.encode(&frame).is_err());
    }

    #[test]
    fn test_encoder_empty_frame() {
        let config = OpusEncoderConfig::default();
        let mut encoder = OpusEncoder::new(config).unwrap();

        let frame = AudioFrame {
            data: vec![], // Empty!
            sample_rate: SampleRate::Hz48000,
            channels: Channels::Mono,
            timestamp: 0,
        };

        assert!(encoder.encode(&frame).is_err());
    }

    #[test]
    fn test_decoder_corrupted_data() {
        let mut decoder = OpusDecoder::new(SampleRate::Hz48000, Channels::Mono).unwrap();

        // Data too small
        let corrupted = vec![0u8; 10];
        assert!(decoder.decode(&corrupted).is_err());
    }

    #[test]
    fn test_decoder_invalid_sample_rate() {
        let mut decoder = OpusDecoder::new(SampleRate::Hz48000, Channels::Mono).unwrap();

        let mut data = Vec::new();
        data.extend_from_slice(&99999u32.to_le_bytes()); // Invalid sample rate
        data.push(1); // Mono
        data.extend_from_slice(&1000u64.to_le_bytes()); // Timestamp
        data.extend_from_slice(&100u32.to_le_bytes()); // Length
        data.extend_from_slice(&vec![0u8; 200]); // Data

        assert!(decoder.decode(&data).is_err());
    }

    #[test]
    fn test_decoder_invalid_channels() {
        let mut decoder = OpusDecoder::new(SampleRate::Hz48000, Channels::Mono).unwrap();

        let mut data = Vec::new();
        data.extend_from_slice(&48000u32.to_le_bytes());
        data.push(5); // Invalid channel count
        data.extend_from_slice(&1000u64.to_le_bytes());
        data.extend_from_slice(&100u32.to_le_bytes());
        data.extend_from_slice(&vec![0u8; 200]);

        assert!(decoder.decode(&data).is_err());
    }

    #[test]
    fn test_different_sample_rates() {
        for &sample_rate in &[
            SampleRate::Hz8000,
            SampleRate::Hz12000,
            SampleRate::Hz16000,
            SampleRate::Hz24000,
            SampleRate::Hz48000,
        ] {
            let config = OpusEncoderConfig {
                sample_rate,
                ..Default::default()
            };
            let mut encoder = OpusEncoder::new(config).unwrap();
            let mut decoder = OpusDecoder::new(sample_rate, Channels::Mono).unwrap();

            let frame = AudioFrame {
                data: vec![100; 1000],
                sample_rate,
                channels: Channels::Mono,
                timestamp: 5000,
            };

            let compressed = encoder.encode(&frame).unwrap();
            let decoded = decoder.decode(&compressed).unwrap();

            assert_eq!(decoded.sample_rate, sample_rate);
            assert_eq!(decoded.data, frame.data);
        }
    }

    #[test]
    fn test_timestamp_preservation() {
        let config = OpusEncoderConfig::default();
        let mut encoder = OpusEncoder::new(config).unwrap();
        let mut decoder = OpusDecoder::new(SampleRate::Hz48000, Channels::Mono).unwrap();

        let timestamps = vec![0u64, 1000, u64::MAX / 2, u64::MAX - 1000];

        for ts in timestamps {
            let frame = AudioFrame {
                data: vec![42; 480], // 10ms at 48kHz
                sample_rate: SampleRate::Hz48000,
                channels: Channels::Mono,
                timestamp: ts,
            };

            let compressed = encoder.encode(&frame).unwrap();
            let decoded = decoder.decode(&compressed).unwrap();

            assert_eq!(decoded.timestamp, ts);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn sample_rate_strategy() -> impl Strategy<Value = SampleRate> {
        prop_oneof![
            Just(SampleRate::Hz8000),
            Just(SampleRate::Hz12000),
            Just(SampleRate::Hz16000),
            Just(SampleRate::Hz24000),
            Just(SampleRate::Hz48000),
        ]
    }

    fn channels_strategy() -> impl Strategy<Value = Channels> {
        prop_oneof![Just(Channels::Mono), Just(Channels::Stereo),]
    }

    proptest! {
        #[test]
        fn prop_encode_decode_roundtrip(
            sample_rate in sample_rate_strategy(),
            channels in channels_strategy(),
            bitrate in 6000u32..=510000,
            timestamp in any::<u64>(),
            audio_len in 1usize..=10000,
        ) {
            let config = OpusEncoderConfig { sample_rate, channels, bitrate };
            let mut encoder = OpusEncoder::new(config)?;
            let mut decoder = OpusDecoder::new(sample_rate, channels)?;

            let audio_data: Vec<i16> = (0..audio_len).map(|i| (i % 1000) as i16).collect();
            let frame = AudioFrame {
                data: audio_data.clone(),
                sample_rate,
                channels,
                timestamp,
            };

            let compressed = encoder.encode(&frame)?;
            let decoded = decoder.decode(&compressed)?;

            prop_assert_eq!(decoded.sample_rate, sample_rate);
            prop_assert_eq!(decoded.channels, channels);
            prop_assert_eq!(decoded.timestamp, timestamp);
            prop_assert_eq!(decoded.data, audio_data);
        }

        #[test]
        fn prop_encoder_rejects_mismatched_config(
            encoder_rate in sample_rate_strategy(),
            frame_rate in sample_rate_strategy(),
            encoder_channels in channels_strategy(),
            frame_channels in channels_strategy(),
        ) {
            if encoder_rate != frame_rate || encoder_channels != frame_channels {
                let config = OpusEncoderConfig {
                    sample_rate: encoder_rate,
                    channels: encoder_channels,
                    bitrate: 64000,
                };
                let mut encoder = OpusEncoder::new(config)?;

                let frame = AudioFrame {
                    data: vec![0; 100],
                    sample_rate: frame_rate,
                    channels: frame_channels,
                    timestamp: 0,
                };

                prop_assert!(encoder.encode(&frame).is_err());
            }
        }
    }
}
