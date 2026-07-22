//! Opus audio codec.
//!
//! With the default `opus` feature enabled, [`OpusEncoder`] and
//! [`OpusDecoder`] wrap the real libopus (via the `opus` crate):
//! 48 kHz mono, 20 ms frames and a configurable bitrate by default.
//!
//! The Opus bitstream does not carry timestamps; [`AudioFrame::timestamp`]
//! travels out-of-band (e.g. RTP or the QUIC media framing), so decoded
//! frames report `timestamp = 0` and callers restamp from their transport.
//!
//! A legacy pass-through simulation is kept for tests only, behind
//! `#[cfg(any(test, feature = "stub-codecs"))]` in [`stub`]. It performs no
//! compression and must never be used for production audio.

#[cfg(feature = "opus")]
use crate::CodecError;
#[cfg(feature = "opus")]
use crate::Result;
#[cfg(feature = "opus")]
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
    /// The sample rate in hertz.
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
    /// The number of channels.
    pub fn count(&self) -> usize {
        *self as usize
    }
}

/// Audio frame for encoding/decoding.
///
/// `data` is interleaved 16-bit signed PCM. `timestamp` is carried
/// out-of-band by the transport layer, not inside the Opus bitstream.
#[derive(Debug, Clone)]
pub struct AudioFrame {
    /// PCM audio data (16-bit signed samples, interleaved when stereo)
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

/// Legal Opus frame durations in microseconds (2.5, 5, 10, 20, 40, 60 ms).
pub const OPUS_FRAME_DURATIONS_US: [u32; 6] = [2_500, 5_000, 10_000, 20_000, 40_000, 60_000];

/// Samples per channel for a 20 ms frame at the given rate — the default
/// frame duration used across the crate.
pub fn samples_per_20ms(sample_rate: SampleRate) -> usize {
    (sample_rate.as_hz() as usize) / 50
}

#[cfg(feature = "opus")]
fn map_channels(channels: Channels) -> opus::Channels {
    match channels {
        Channels::Mono => opus::Channels::Mono,
        Channels::Stereo => opus::Channels::Stereo,
    }
}

#[cfg(feature = "opus")]
fn is_legal_frame(samples_per_channel: usize, sample_rate: SampleRate) -> bool {
    let hz = sample_rate.as_hz() as u64;
    OPUS_FRAME_DURATIONS_US
        .iter()
        .any(|us| (hz * u64::from(*us)) / 1_000_000 == samples_per_channel as u64)
}

/// Opus audio encoder backed by libopus (VoIP application profile).
#[cfg(feature = "opus")]
pub struct OpusEncoder {
    config: OpusEncoderConfig,
    inner: opus::Encoder,
}

#[cfg(feature = "opus")]
impl OpusEncoder {
    /// Create an encoder for the given configuration.
    pub fn new(config: OpusEncoderConfig) -> Result<Self> {
        if config.bitrate < 6000 || config.bitrate > 510_000 {
            return Err(CodecError::InvalidData(
                "bitrate out of range (6000-510000)",
            ));
        }
        let mut inner = opus::Encoder::new(
            config.sample_rate.as_hz(),
            map_channels(config.channels),
            opus::Application::Voip,
        )
        .map_err(|e| CodecError::InitFailed(format!("opus encoder: {e}")))?;
        inner
            .set_bitrate(opus::Bitrate::Bits(config.bitrate as i32))
            .map_err(|e| CodecError::InitFailed(format!("opus bitrate: {e}")))?;
        Ok(Self { config, inner })
    }

    /// Encode one PCM frame to an Opus packet.
    ///
    /// The frame must contain a legal Opus frame duration of samples
    /// (2.5/5/10/20/40/60 ms) matching the encoder's sample rate and
    /// channel count; 20 ms is the crate default
    /// (see [`samples_per_20ms`]).
    pub fn encode(&mut self, frame: &AudioFrame) -> Result<Bytes> {
        if frame.sample_rate != self.config.sample_rate {
            return Err(CodecError::InvalidData("sample rate mismatch"));
        }
        if frame.channels != self.config.channels {
            return Err(CodecError::InvalidData("channel count mismatch"));
        }
        if frame.data.is_empty() {
            return Err(CodecError::InvalidData("empty audio frame"));
        }
        let per_channel = frame.data.len() / self.config.channels.count();
        if !frame
            .data
            .len()
            .is_multiple_of(self.config.channels.count())
            || !is_legal_frame(per_channel, self.config.sample_rate)
        {
            return Err(CodecError::InvalidData(
                "frame length is not a legal Opus frame duration",
            ));
        }
        // Worst-case packet bound; real VoIP packets are far smaller.
        let max_size = 4000;
        let packet = self
            .inner
            .encode_vec(&frame.data, max_size)
            .map_err(|e| CodecError::InitFailed(format!("opus encode: {e}")))?;
        Ok(Bytes::from(packet))
    }
}

/// Opus audio decoder backed by libopus.
#[cfg(feature = "opus")]
pub struct OpusDecoder {
    sample_rate: SampleRate,
    channels: Channels,
    inner: opus::Decoder,
}

#[cfg(feature = "opus")]
impl OpusDecoder {
    /// Create a decoder; the sample rate and channel count must match the
    /// stream's negotiated parameters.
    pub fn new(sample_rate: SampleRate, channels: Channels) -> Result<Self> {
        let inner = opus::Decoder::new(sample_rate.as_hz(), map_channels(channels))
            .map_err(|e| CodecError::InitFailed(format!("opus decoder: {e}")))?;
        Ok(Self {
            sample_rate,
            channels,
            inner,
        })
    }

    /// Decode one Opus packet to PCM.
    ///
    /// The returned frame's `timestamp` is `0`: Opus packets carry no
    /// timestamps — the transport layer restamps frames.
    pub fn decode(&mut self, data: &[u8]) -> Result<AudioFrame> {
        if data.is_empty() {
            return Err(CodecError::InvalidData("empty opus packet"));
        }
        // Largest legal frame: 60 ms.
        let max_per_channel = (self.sample_rate.as_hz() as usize * 60) / 1000;
        let mut pcm = vec![0i16; max_per_channel * self.channels.count()];
        let decoded_per_channel = self
            .inner
            .decode(data, &mut pcm, false)
            .map_err(|e| CodecError::InitFailed(format!("opus decode: {e}")))?;
        pcm.truncate(decoded_per_channel * self.channels.count());
        Ok(AudioFrame {
            data: pcm,
            sample_rate: self.sample_rate,
            channels: self.channels,
            timestamp: 0,
        })
    }
}

/// Test-only pass-through simulation of the Opus interfaces.
///
/// Performs **no compression** — it round-trips PCM verbatim with a small
/// header. Exists so transport-layer tests can run without libopus.
/// Never available in default builds.
#[cfg(any(test, feature = "stub-codecs"))]
pub mod stub {
    use super::{AudioFrame, Channels, OpusEncoderConfig, SampleRate};
    use crate::{CodecError, Result};
    use bytes::Bytes;

    /// Pass-through "encoder" (no compression; tests only).
    pub struct StubOpusEncoder {
        config: OpusEncoderConfig,
    }

    impl StubOpusEncoder {
        /// Create a stub encoder.
        pub fn new(config: OpusEncoderConfig) -> Result<Self> {
            if config.bitrate < 6000 || config.bitrate > 510_000 {
                return Err(CodecError::InvalidData(
                    "bitrate out of range (6000-510000)",
                ));
            }
            Ok(Self { config })
        }

        /// "Encode" by prefixing a header and copying PCM bytes verbatim.
        pub fn encode(&mut self, frame: &AudioFrame) -> Result<Bytes> {
            if frame.sample_rate != self.config.sample_rate {
                return Err(CodecError::InvalidData("sample rate mismatch"));
            }
            if frame.channels != self.config.channels {
                return Err(CodecError::InvalidData("channel count mismatch"));
            }
            if frame.data.is_empty() {
                return Err(CodecError::InvalidData("empty audio frame"));
            }
            let mut out = Vec::new();
            out.extend_from_slice(&self.config.sample_rate.as_hz().to_le_bytes());
            out.push(self.config.channels.count() as u8);
            out.extend_from_slice(&frame.timestamp.to_le_bytes());
            out.extend_from_slice(&(frame.data.len() as u32).to_le_bytes());
            let bytes: Vec<u8> = frame.data.iter().flat_map(|s| s.to_le_bytes()).collect();
            out.extend_from_slice(&bytes);
            Ok(Bytes::from(out))
        }
    }

    /// Pass-through "decoder" (tests only).
    pub struct StubOpusDecoder;

    impl StubOpusDecoder {
        /// Create a stub decoder.
        pub fn new(_sample_rate: SampleRate, _channels: Channels) -> Result<Self> {
            Ok(Self)
        }

        /// Reverse [`StubOpusEncoder::encode`].
        pub fn decode(&mut self, data: &[u8]) -> Result<AudioFrame> {
            const HEADER_SIZE: usize = 17;
            if data.len() < HEADER_SIZE {
                return Err(CodecError::InvalidData("opus data too small"));
            }
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
            let channels = match data[4] {
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
            let pcm_bytes = data
                .get(HEADER_SIZE..)
                .ok_or(CodecError::InvalidData("missing pcm data"))?;
            let mut pcm_data = Vec::with_capacity(data_len);
            for chunk in pcm_bytes.chunks_exact(2) {
                if let Ok(bytes) = chunk.try_into() {
                    pcm_data.push(i16::from_le_bytes(bytes));
                }
            }
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
}

#[cfg(all(test, feature = "opus"))]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn tone_frame(samples: usize, timestamp: u64) -> AudioFrame {
        let data: Vec<i16> = (0..samples)
            .map(|i| {
                (((i as f32) * 440.0 * 2.0 * std::f32::consts::PI / 48000.0).sin() * 16000.0) as i16
            })
            .collect();
        AudioFrame {
            data,
            sample_rate: SampleRate::Hz48000,
            channels: Channels::Mono,
            timestamp,
        }
    }

    #[test]
    fn encoder_creation_default_and_custom() {
        assert!(OpusEncoder::new(OpusEncoderConfig::default()).is_ok());
        assert!(OpusEncoder::new(OpusEncoderConfig {
            sample_rate: SampleRate::Hz16000,
            channels: Channels::Stereo,
            bitrate: 96000,
        })
        .is_ok());
    }

    #[test]
    fn encoder_invalid_bitrate() {
        assert!(OpusEncoder::new(OpusEncoderConfig {
            bitrate: 5000,
            ..Default::default()
        })
        .is_err());
        assert!(OpusEncoder::new(OpusEncoderConfig {
            bitrate: 520_000,
            ..Default::default()
        })
        .is_err());
    }

    #[test]
    fn encode_rejects_mismatch_and_illegal_length() {
        let mut enc = OpusEncoder::new(OpusEncoderConfig::default()).unwrap();
        // sample-rate mismatch
        let mut f = tone_frame(960, 0);
        f.sample_rate = SampleRate::Hz16000;
        assert!(enc.encode(&f).is_err());
        // channel mismatch
        let mut f = tone_frame(960, 0);
        f.channels = Channels::Stereo;
        assert!(enc.encode(&f).is_err());
        // empty
        let mut f = tone_frame(960, 0);
        f.data.clear();
        assert!(enc.encode(&f).is_err());
        // illegal duration (1000 samples @48k is not 2.5/5/10/20/40/60 ms)
        let f = tone_frame(1000, 0);
        assert!(enc.encode(&f).is_err());
    }

    #[test]
    fn encode_20ms_roundtrip_and_compression() {
        let mut enc = OpusEncoder::new(OpusEncoderConfig::default()).unwrap();
        let mut dec = OpusDecoder::new(SampleRate::Hz48000, Channels::Mono).unwrap();
        let n = samples_per_20ms(SampleRate::Hz48000);
        assert_eq!(n, 960);
        // The first 1–2 packets carry cold-start transients and may exceed
        // the nominal bitrate; the compression bound applies to steady
        // state. (The pass-through stub emits 1,937-byte "packets" for the
        // same input, so it fails this regardless of warm-up.)
        let mut last_packet = None;
        for idx in 0..4 {
            let frame = tone_frame(n, 42 + idx);
            let packet = enc.encode(&frame).unwrap();
            if idx >= 2 {
                assert!(
                    packet.len() <= 200,
                    "steady-state 20 ms packet expected <=200 bytes, got {}",
                    packet.len()
                );
            }
            last_packet = Some(packet);
        }
        let packet = last_packet.unwrap();
        let decoded = dec.decode(&packet).unwrap();
        assert_eq!(decoded.data.len(), n);
        assert_eq!(decoded.sample_rate, SampleRate::Hz48000);
        assert_eq!(decoded.channels, Channels::Mono);
        // Timestamps travel out-of-band.
        assert_eq!(decoded.timestamp, 0);
    }

    #[test]
    fn decoder_rejects_garbage() {
        let mut dec = OpusDecoder::new(SampleRate::Hz48000, Channels::Mono).unwrap();
        assert!(dec.decode(&[]).is_err());
        // A long run of 0xFF is not a valid TOC/packet for libopus.
        assert!(dec.decode(&[0xFFu8; 3]).is_err());
    }

    #[test]
    fn stub_still_roundtrips_for_transport_tests() {
        use super::stub::{StubOpusDecoder, StubOpusEncoder};
        let mut enc = StubOpusEncoder::new(OpusEncoderConfig::default()).unwrap();
        let mut dec = StubOpusDecoder::new(SampleRate::Hz48000, Channels::Mono).unwrap();
        let frame = tone_frame(1000, 7); // arbitrary length allowed by the stub
        let packet = enc.encode(&frame).unwrap();
        let decoded = dec.decode(&packet).unwrap();
        assert_eq!(decoded.data, frame.data);
        assert_eq!(decoded.timestamp, 7);
    }
}
