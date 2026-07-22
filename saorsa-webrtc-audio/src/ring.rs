//! Bounded playout ring: async producer, real-time consumer, silence on
//! underrun, drop-oldest on overrun. Never blocks the audio callback beyond
//! a short mutex-guarded copy.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Shared state between the playout task (producer) and the cpal output
/// callback (consumer).
#[derive(Debug)]
pub struct PlayoutRing {
    buf: Mutex<VecDeque<i16>>,
    capacity: usize,
    underrun_samples: AtomicU64,
    overrun_samples: AtomicU64,
}

impl PlayoutRing {
    /// Ring holding at most `capacity` samples (drop-oldest beyond it).
    pub fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            buf: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
            underrun_samples: AtomicU64::new(0),
            overrun_samples: AtomicU64::new(0),
        })
    }

    /// Producer side: append samples, evicting oldest on overflow.
    pub fn push(&self, samples: &[i16]) {
        let mut buf = match self.buf.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let incoming = samples.len();
        let total = buf.len() + incoming;
        if total > self.capacity {
            let evict = (total - self.capacity).min(buf.len());
            buf.drain(..evict);
            self.overrun_samples
                .fetch_add(evict as u64, Ordering::Relaxed);
        }
        // If a single push exceeds capacity, keep its tail.
        let start = incoming.saturating_sub(self.capacity);
        buf.extend(&samples[start..]);
    }

    /// Consumer side (audio callback): fill `out` from the ring; missing
    /// samples become silence and are counted as underrun.
    pub fn pop_into(&self, out: &mut [i16]) {
        let mut buf = match self.buf.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let n = out.len().min(buf.len());
        for slot in out.iter_mut().take(n) {
            // len checked above; drain-free pop keeps this allocation-free.
            *slot = buf.pop_front().unwrap_or(0);
        }
        if n < out.len() {
            for slot in out.iter_mut().skip(n) {
                *slot = 0;
            }
            self.underrun_samples
                .fetch_add((out.len() - n) as u64, Ordering::Relaxed);
        }
    }

    /// Total samples replaced by silence due to underrun.
    pub fn underrun_samples(&self) -> u64 {
        self.underrun_samples.load(Ordering::Relaxed)
    }

    /// Total samples evicted due to overrun (producer ahead of consumer).
    pub fn overrun_samples(&self) -> u64 {
        self.overrun_samples.load(Ordering::Relaxed)
    }

    /// Samples currently buffered.
    pub fn len(&self) -> usize {
        match self.buf.lock() {
            Ok(g) => g.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
    }

    /// True when no samples are buffered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underrun_inserts_silence_and_counts() {
        let ring = PlayoutRing::new(1000);
        ring.push(&[5i16; 100]);
        let mut out = [1i16; 300];
        ring.pop_into(&mut out);
        assert!(out[..100].iter().all(|&s| s == 5));
        assert!(out[100..].iter().all(|&s| s == 0));
        assert_eq!(ring.underrun_samples(), 200);
    }

    #[test]
    fn overrun_drops_oldest_keeps_newest() {
        let ring = PlayoutRing::new(100);
        ring.push(&[1i16; 80]);
        ring.push(&[2i16; 50]); // 130 > 100 → evict 30 oldest
        assert_eq!(ring.len(), 100);
        assert_eq!(ring.overrun_samples(), 30);
        let mut out = [0i16; 100];
        ring.pop_into(&mut out);
        assert!(out[..50].iter().all(|&s| s == 1));
        assert!(out[50..].iter().all(|&s| s == 2));
    }

    #[test]
    fn giant_single_push_keeps_tail() {
        let ring = PlayoutRing::new(10);
        let big: Vec<i16> = (0..25).map(|i| i as i16).collect();
        ring.push(&big);
        let mut out = [0i16; 10];
        ring.pop_into(&mut out);
        let expect: Vec<i16> = (15..25).map(|i| i as i16).collect();
        assert_eq!(out.to_vec(), expect);
    }
}
