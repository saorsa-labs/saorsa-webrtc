//! Mono resampling to 48 kHz via `rubato`'s polynomial `FastFixedIn`.
//!
//! Only constructed when the capture device does not natively offer 48 kHz;
//! at 48 kHz the pipeline bypasses this module entirely.

use crate::{AudioError, Result};
use rubato::{FastFixedIn, PolynomialDegree, Resampler as _};

/// Process input in 10 ms blocks (at the device rate) for bounded latency.
const BLOCK_DIVISOR: u32 = 100;

/// Streaming mono i16 resampler: arbitrary input rate → 48 kHz.
///
/// Accepts arbitrarily sized pushes; buffers internally and processes in
/// fixed 10 ms blocks (a `FastFixedIn` requirement). Output is drained via
/// [`MonoResampler::pop_output`].
pub struct MonoResampler {
    inner: FastFixedIn<f32>,
    block: usize,
    pending: Vec<f32>,
    out: Vec<i16>,
}

impl std::fmt::Debug for MonoResampler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MonoResampler")
            .field("block", &self.block)
            .field("pending", &self.pending.len())
            .field("out", &self.out.len())
            .finish()
    }
}

impl MonoResampler {
    /// Build a resampler from `input_rate` Hz to 48 kHz.
    pub fn new(input_rate: u32) -> Result<Self> {
        if input_rate == 0 {
            return Err(AudioError::Resampler("input rate 0".into()));
        }
        let block = (input_rate / BLOCK_DIVISOR).max(1) as usize;
        let ratio = 48_000.0 / f64::from(input_rate);
        let inner = FastFixedIn::<f32>::new(ratio, 1.0, PolynomialDegree::Septic, block, 1)
            .map_err(|e| AudioError::Resampler(e.to_string()))?;
        Ok(Self {
            inner,
            block,
            pending: Vec::with_capacity(block * 2),
            out: Vec::with_capacity(block * 2),
        })
    }

    /// Push mono samples at the input rate; complete blocks are resampled
    /// immediately, the remainder is buffered.
    pub fn push(&mut self, samples: &[i16]) -> Result<()> {
        self.pending
            .extend(samples.iter().map(|&s| crate::convert::i16_to_f32(s)));
        while self.pending.len() >= self.block {
            let rest = self.pending.split_off(self.block);
            let input = std::mem::replace(&mut self.pending, rest);
            let resampled = self
                .inner
                .process(&[input], None)
                .map_err(|e| AudioError::Resampler(e.to_string()))?;
            if let Some(ch0) = resampled.first() {
                self.out
                    .extend(ch0.iter().map(|&s| crate::convert::f32_to_i16(s)));
            }
        }
        Ok(())
    }

    /// Take all 48 kHz output produced so far.
    pub fn pop_output(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Push a 440 Hz tone at 44.1 kHz, expect ≈48 kHz worth of samples out
    /// with the tone energy intact (coarse spectral sanity via zero-crossing
    /// rate, which is frequency-proportional and resampler-invariant).
    #[test]
    fn ratio_and_tone_survive_44100_to_48000() {
        let in_rate = 44_100u32;
        let secs = 1.0f32;
        let n = (in_rate as f32 * secs) as usize;
        let tone: Vec<i16> = (0..n)
            .map(|i| {
                let t = i as f32 / in_rate as f32;
                crate::convert::f32_to_i16(0.5 * (2.0 * std::f32::consts::PI * 440.0 * t).sin())
            })
            .collect();

        let mut rs = MonoResampler::new(in_rate).expect("resampler");
        rs.push(&tone).expect("push");
        let out = rs.pop_output();

        let expected = 48_000.0 * secs;
        let got = out.len() as f32;
        assert!(
            (got - expected).abs() / expected < 0.02,
            "output length {got} not within 2% of {expected}"
        );

        let zc = |s: &[i16]| s.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count() as f32;
        let in_rate_zc = zc(&tone) / secs;
        let out_secs = got / 48_000.0;
        let out_zc = zc(&out) / out_secs;
        assert!(
            (out_zc - in_rate_zc).abs() / in_rate_zc < 0.05,
            "zero-crossing rate drifted: in {in_rate_zc}/s out {out_zc}/s"
        );
    }

    #[test]
    fn arbitrary_push_sizes_lose_no_blocks() {
        let mut rs = MonoResampler::new(16_000).expect("resampler");
        for chunk in [3usize, 7, 160, 41, 500, 89] {
            rs.push(&vec![1000i16; chunk]).expect("push");
        }
        // 800 samples pushed; block = 160 → 5 complete blocks processed.
        // FastFixedIn carries a small startup delay, so allow up to one
        // block of slack below the ideal 2400 while requiring all
        // processed blocks to have produced output.
        let out = rs.pop_output();
        let ideal = 2400i64;
        let got = out.len() as i64;
        assert!(
            got > ideal - 480 && got <= ideal + 16,
            "expected within one block of {ideal}, got {got}"
        );
    }

    #[test]
    fn zero_rate_rejected() {
        assert!(MonoResampler::new(0).is_err());
    }
}
