//! Jitter buffer for the audio datagram lane (WP-V1.3).
//!
//! MANDATORY per the R0 two-node finding: ant-quic's per-message send path
//! does not preserve cross-message ordering (~4–5% reorder observed on
//! loopback), and the datagram lane additionally permits loss. This buffer
//! restores playout order within a bounded window and surfaces gaps
//! explicitly so a packet-loss-concealment (PLC) stage can act on them.
//!
//! Semantics:
//! - Frames are keyed by the 32-bit lane sequence number, extended to 64
//!   bits internally (RTP-style wrap handling).
//! - In-order frames drain immediately on [`JitterBuffer::poll_ready`].
//! - A missing sequence is declared a [`JitterEvent::Gap`] once the buffer
//!   holds at least [`JitterConfig::reorder_window_frames`] newer frames —
//!   a deterministic, count-based trigger (the time-based trigger in
//!   [`JitterConfig::reorder_window_ms`] additionally bounds latency when
//!   traffic stalls).
//! - Duplicates of buffered frames and frames older than the playout
//!   cursor are dropped and counted, never re-emitted.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::datagram_lane::AudioDatagram;

/// Configuration for [`JitterBuffer`].
#[derive(Debug, Clone)]
pub struct JitterConfig {
    /// Declare a gap once this many frames newer than the missing one are
    /// buffered. Default 3 (≈60 ms at 20 ms frames).
    pub reorder_window_frames: usize,
    /// Also declare a gap once the oldest buffered frame has waited this
    /// long, even if fewer than `reorder_window_frames` are buffered.
    /// Default 60 ms.
    pub reorder_window_ms: u64,
    /// Reserved for adaptive window depth; currently fixed-depth only.
    pub adaptive: bool,
}

impl Default for JitterConfig {
    fn default() -> Self {
        Self {
            reorder_window_frames: 3,
            reorder_window_ms: 60,
            adaptive: false,
        }
    }
}

/// Counters exposed for diagnostics.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct JitterCounters {
    /// Frames delivered in playout order.
    pub delivered: u64,
    /// Frames that arrived out of order but were reordered successfully.
    pub reordered: u64,
    /// Frames dropped because they arrived behind the playout cursor.
    pub late_dropped: u64,
    /// Frames dropped because an identical sequence was already buffered.
    pub duplicates_dropped: u64,
    /// Gaps declared (each is one missing sequence surfaced to PLC).
    pub gaps_emitted: u64,
}

/// Output of [`JitterBuffer::poll_ready`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JitterEvent {
    /// A frame ready for playout, in order.
    Frame(AudioDatagram),
    /// A sequence declared lost — hook for packet-loss concealment.
    Gap {
        /// The missing lane sequence number (unextended, as on the wire).
        seq: u32,
    },
}

/// Reorder/loss-absorbing buffer between the datagram lane and playout.
#[derive(Debug)]
pub struct JitterBuffer {
    config: JitterConfig,
    /// Next extended sequence expected for playout. `None` until the first
    /// frame arrives (the first frame anchors the cursor).
    next_ext: Option<u64>,
    /// Highest extended sequence observed (drives wrap extension).
    highest_ext: u64,
    /// Buffered out-of-order frames keyed by extended sequence.
    buffered: BTreeMap<u64, (AudioDatagram, Instant)>,
    counters: JitterCounters,
}

impl JitterBuffer {
    /// Create a buffer with the given configuration.
    #[must_use]
    pub fn new(config: JitterConfig) -> Self {
        Self {
            config,
            next_ext: None,
            highest_ext: 0,
            buffered: BTreeMap::new(),
            counters: JitterCounters::default(),
        }
    }

    /// Current counter snapshot.
    #[must_use]
    pub fn counters(&self) -> JitterCounters {
        self.counters
    }

    /// Number of frames currently buffered out of order.
    #[must_use]
    pub fn buffered_len(&self) -> usize {
        self.buffered.len()
    }

    /// Extend a 32-bit wire sequence to 64 bits relative to the highest
    /// sequence seen (RTP-style: pick the candidate closest to the current
    /// position, allowing wrap in either direction).
    fn extend_seq(&self, seq: u32) -> u64 {
        let cycle = self.highest_ext & !u64::from(u32::MAX);
        let base = cycle | u64::from(seq);
        let half = u64::from(u32::MAX / 2);
        if base + half < self.highest_ext {
            base + (1u64 << 32)
        } else if base > self.highest_ext + half && cycle > 0 {
            base - (1u64 << 32)
        } else {
            base
        }
    }

    /// True until the first frame (or gap) has been played out.
    fn warming_up(&self) -> bool {
        self.counters.delivered == 0 && self.counters.gaps_emitted == 0
    }

    /// Insert a frame. Call [`Self::poll_ready`] afterwards to drain.
    pub fn push(&mut self, frame: AudioDatagram) {
        let ext = self.extend_seq(frame.seq);
        let prev_highest = self.highest_ext;
        let first = self.next_ext.is_none();
        if ext > self.highest_ext {
            self.highest_ext = ext;
        }
        match self.next_ext {
            None => {
                // First frame anchors the cursor (playout is held back by
                // the warm-up window in `poll_ready`).
                self.next_ext = Some(ext);
                self.buffered.insert(ext, (frame, Instant::now()));
            }
            Some(next) if ext < next => {
                // Before anything has played out the cursor is only an
                // anchor guess from the first arrival — an earlier frame
                // re-anchors instead of being dropped.
                if self.warming_up() {
                    self.counters.reordered += 1;
                    self.next_ext = Some(ext);
                    self.buffered.insert(ext, (frame, Instant::now()));
                } else {
                    self.counters.late_dropped += 1;
                }
            }
            Some(_) => {
                if self.buffered.contains_key(&ext) {
                    self.counters.duplicates_dropped += 1;
                    return;
                }
                if !first && ext < prev_highest {
                    self.counters.reordered += 1;
                }
                self.buffered.insert(ext, (frame, Instant::now()));
            }
        }
    }

    /// Drain everything ready for playout: in-order frames, plus gap
    /// declarations once the reorder window is exceeded.
    pub fn poll_ready(&mut self) -> Vec<JitterEvent> {
        let mut out = Vec::new();
        let Some(mut next) = self.next_ext else {
            return out;
        };
        // Warm-up: hold playout until the reorder window fills (by count
        // or by age) so early out-of-order arrivals can settle.
        if self.warming_up() {
            let window = Duration::from_millis(self.config.reorder_window_ms);
            let filled = self.buffered.len() >= self.config.reorder_window_frames.max(1);
            let aged = self
                .buffered
                .values()
                .next()
                .is_some_and(|(_, at)| at.elapsed() >= window);
            if !filled && !aged {
                return out;
            }
            if let Some(lowest) = self.buffered.keys().next().copied() {
                next = lowest;
            }
        }
        let window_frames = self.config.reorder_window_frames.max(1);
        let window = Duration::from_millis(self.config.reorder_window_ms);

        loop {
            if let Some((frame, _)) = self.buffered.remove(&next) {
                out.push(JitterEvent::Frame(frame));
                self.counters.delivered += 1;
                next += 1;
                continue;
            }
            // `next` is missing. Declare a gap only when the window is
            // exceeded by count (deterministic) or by age (latency bound).
            let newer = self.buffered.range(next..).count();
            let oldest_wait = self
                .buffered
                .range(next..)
                .next()
                .map(|(_, (_, at))| at.elapsed());
            let count_exceeded = newer >= window_frames;
            let time_exceeded = oldest_wait.is_some_and(|w| w >= window);
            if newer > 0 && (count_exceeded || time_exceeded) {
                let wire_seq = (next & u64::from(u32::MAX)) as u32;
                out.push(JitterEvent::Gap { seq: wire_seq });
                self.counters.gaps_emitted += 1;
                next += 1;
                continue;
            }
            break;
        }
        self.next_ext = Some(next);
        out
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use bytes::Bytes;

    fn frame(seq: u32) -> AudioDatagram {
        AudioDatagram {
            seq,
            timestamp_ms: u64::from(seq) * 20,
            flags: 0,
            payload: Bytes::from(vec![(seq % 251) as u8; 40]),
        }
    }

    fn drain_frames(events: &[JitterEvent]) -> Vec<u32> {
        events
            .iter()
            .filter_map(|e| match e {
                JitterEvent::Frame(f) => Some(f.seq),
                JitterEvent::Gap { .. } => None,
            })
            .collect()
    }

    #[test]
    fn in_order_delivery_is_immediate() {
        let mut jb = JitterBuffer::new(JitterConfig::default());
        let mut got = Vec::new();
        for s in 0..50u32 {
            jb.push(frame(s));
            got.extend(drain_frames(&jb.poll_ready()));
        }
        assert_eq!(got, (0..50).collect::<Vec<_>>());
        let c = jb.counters();
        assert_eq!(c.delivered, 50);
        assert_eq!(c.gaps_emitted, 0);
        assert_eq!(c.reordered, 0);
    }

    #[test]
    fn five_percent_seeded_reorder_is_fully_recovered() {
        // Deterministic shuffle: swap ~5% of adjacent pairs (seeded by index).
        let mut order: Vec<u32> = (0..200).collect();
        let mut i = 0;
        while i + 1 < order.len() {
            if (i * 2654435761) % 100 < 5 {
                order.swap(i, i + 1);
                i += 2;
            } else {
                i += 1;
            }
        }
        assert_ne!(order, (0..200).collect::<Vec<_>>(), "shuffle must reorder");

        let mut jb = JitterBuffer::new(JitterConfig::default());
        let mut got = Vec::new();
        for s in &order {
            jb.push(frame(*s));
            got.extend(drain_frames(&jb.poll_ready()));
        }
        got.extend(drain_frames(&jb.poll_ready()));
        assert_eq!(got, (0..200).collect::<Vec<_>>(), "adjacent swaps recover");
        let c = jb.counters();
        assert_eq!(c.delivered, 200);
        assert_eq!(c.gaps_emitted, 0);
        assert!(c.reordered > 0);
    }

    #[test]
    fn five_percent_loss_surfaces_gaps_for_plc() {
        let mut jb = JitterBuffer::new(JitterConfig::default());
        let lost: Vec<u32> = (0..200).filter(|s| s % 20 == 7).collect();
        let mut events = Vec::new();
        for s in 0..200u32 {
            if lost.contains(&s) {
                continue;
            }
            jb.push(frame(s));
            events.extend(jb.poll_ready());
        }
        events.extend(jb.poll_ready());

        let delivered = drain_frames(&events);
        let expected: Vec<u32> = (0..200).filter(|s| !lost.contains(s)).collect();
        assert_eq!(delivered, expected);
        let gaps: Vec<u32> = events
            .iter()
            .filter_map(|e| match e {
                JitterEvent::Gap { seq } => Some(*seq),
                JitterEvent::Frame(_) => None,
            })
            .collect();
        assert_eq!(gaps, lost, "every lost seq surfaces exactly one gap");
        assert_eq!(jb.counters().gaps_emitted, lost.len() as u64);
    }

    #[test]
    fn duplicates_and_late_frames_are_dropped_and_counted() {
        let mut jb = JitterBuffer::new(JitterConfig::default());
        // Fill the warm-up window so playout starts.
        for s in 0..3u32 {
            jb.push(frame(s));
        }
        assert_eq!(drain_frames(&jb.poll_ready()), vec![0, 1, 2]); // cursor at 3
        jb.push(frame(5)); // buffered (3, 4 missing)
        jb.push(frame(5)); // duplicate of buffered
        jb.push(frame(0)); // behind cursor → late
        let c = jb.counters();
        assert_eq!(c.duplicates_dropped, 1);
        assert_eq!(c.late_dropped, 1);
        jb.push(frame(3));
        jb.push(frame(4));
        let got = drain_frames(&jb.poll_ready());
        assert_eq!(got, vec![3, 4, 5]);
    }

    #[test]
    fn seq_wrap_is_handled() {
        let mut jb = JitterBuffer::new(JitterConfig::default());
        let start = u32::MAX - 2;
        let mut got = Vec::new();
        for off in 0..6u32 {
            jb.push(frame(start.wrapping_add(off)));
            got.extend(drain_frames(&jb.poll_ready()));
        }
        let expected: Vec<u32> = (0..6).map(|o| start.wrapping_add(o)).collect();
        assert_eq!(got, expected, "delivery continues across u32 wrap");
        assert_eq!(jb.counters().gaps_emitted, 0);
    }
}
