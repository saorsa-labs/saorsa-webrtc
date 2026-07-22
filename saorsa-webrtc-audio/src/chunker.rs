//! Accumulate a mono 48 kHz i16 sample stream into 20 ms [`AudioFrame`]s.

use crate::{FRAME_MS, FRAME_SAMPLES_48K_MONO_20MS};
use saorsa_webrtc_codecs::{AudioFrame, Channels, SampleRate};

/// Stateful chunker: push samples in arbitrary block sizes, pull complete
/// 20 ms frames with a monotonically increasing sample-clock timestamp.
///
/// Timestamps are derived from the count of emitted samples (a sample clock),
/// not wall time — the transport layer restamps for network purposes.
#[derive(Debug, Default)]
pub struct FrameChunker {
    buf: Vec<i16>,
    frames_emitted: u64,
}

impl FrameChunker {
    /// New empty chunker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append mono 48 kHz samples.
    pub fn push(&mut self, samples: &[i16]) {
        self.buf.extend_from_slice(samples);
    }

    /// Pop the next complete 20 ms frame, if buffered.
    pub fn pop_frame(&mut self) -> Option<AudioFrame> {
        if self.buf.len() < FRAME_SAMPLES_48K_MONO_20MS {
            return None;
        }
        let rest = self.buf.split_off(FRAME_SAMPLES_48K_MONO_20MS);
        let data = std::mem::replace(&mut self.buf, rest);
        let timestamp = self.frames_emitted * u64::from(FRAME_MS);
        self.frames_emitted += 1;
        Some(AudioFrame {
            data,
            sample_rate: SampleRate::Hz48000,
            channels: Channels::Mono,
            timestamp,
        })
    }

    /// Samples currently buffered (incomplete frame remainder).
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_960_samples_with_20ms_sample_clock() {
        let mut c = FrameChunker::new();
        c.push(&vec![1i16; 2500]);
        let a = c.pop_frame().expect("frame 0");
        let b = c.pop_frame().expect("frame 1");
        assert_eq!(a.data.len(), 960);
        assert_eq!(b.data.len(), 960);
        assert_eq!((a.timestamp, b.timestamp), (0, 20));
        assert!(c.pop_frame().is_none());
        assert_eq!(c.buffered(), 2500 - 1920);
    }

    #[test]
    fn sample_order_is_preserved_across_pushes() {
        let mut c = FrameChunker::new();
        let first: Vec<i16> = (0..600).map(|i| i as i16).collect();
        let second: Vec<i16> = (600..1000).map(|i| i as i16).collect();
        c.push(&first);
        assert!(c.pop_frame().is_none());
        c.push(&second);
        let f = c.pop_frame().expect("frame");
        let expect: Vec<i16> = (0..960).map(|i| i as i16).collect();
        assert_eq!(f.data, expect);
    }
}
