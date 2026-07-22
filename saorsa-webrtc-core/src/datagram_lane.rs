//! Datagram audio lane (WP-V1.3): Opus frames over QUIC DATAGRAM frames.
//!
//! Reliable ordered streams head-of-line-block under loss, which is the
//! wrong trade for real-time voice. This lane sends each encoded audio
//! frame as one unreliable QUIC datagram through
//! `ant_quic::link_transport::LinkConn::{send_datagram, recv_datagrams}`
//! (public since ant-quic 0.27; no upstream changes required). The receive
//! side feeds [`crate::jitter::JitterBuffer`], which restores order and
//! surfaces losses as PLC hooks.
//!
//! **Transport note (verified 2026-07-22):** no ant-quic component reads
//! application datagrams on `Node`/`P2pEndpoint`-managed connections (the
//! only `read_datagram` callers are the MASQUE relay and `LinkConn`), so
//! the lane is safe on any connection. Reach the connection either via
//! `ant_quic::P2pLinkTransport` or via
//! `Node::inner_endpoint().get_quic_connection(&peer_id)` wrapped in
//! `ant_quic::P2pLinkConn` — the latter reuses the Node's proven connect
//! orchestration.
//!
//! Wire format — 12-byte header, big-endian, then the Opus payload:
//!
//! ```text
//! [version u8][flags u8][seq u32][timestamp_ms u48]
//! ```

use bytes::{BufMut, Bytes, BytesMut};
use std::sync::atomic::{AtomicU32, Ordering};
use thiserror::Error;

use ant_quic::link_transport::LinkConn;

/// Header length of a lane datagram.
pub const DATAGRAM_HEADER_LEN: usize = 12;
/// Current wire version.
pub const DATAGRAM_VERSION: u8 = 1;
/// Maximum payload accepted by [`AudioDatagram::encode`]. QUIC datagrams
/// must fit one packet; 1200 minus header keeps clear of the common MTU.
pub const MAX_DATAGRAM_PAYLOAD: usize = 1188;

/// Timestamp mask — 48 bits of milliseconds (~8.9 millennia).
const TS_MASK: u64 = (1 << 48) - 1;

/// Errors from the datagram lane.
#[derive(Debug, Error)]
pub enum DatagramLaneError {
    /// Datagram shorter than the fixed header.
    #[error("datagram too short: {len} < {DATAGRAM_HEADER_LEN}")]
    TooShort {
        /// Received length.
        len: usize,
    },
    /// Unknown wire version.
    #[error("unsupported datagram version {0} (expected {DATAGRAM_VERSION})")]
    BadVersion(u8),
    /// Payload exceeds [`MAX_DATAGRAM_PAYLOAD`].
    #[error("payload too large: {len} > {max}")]
    PayloadTooLarge {
        /// Offered payload length.
        len: usize,
        /// Maximum accepted.
        max: usize,
    },
    /// Transport rejected the datagram.
    #[error("datagram send failed: {0}")]
    Send(String),
}

/// Which lane carries encoded audio frames.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AudioLaneMode {
    /// Ordered reliable QUIC stream (`StreamType::Audio`) — the R0 path.
    /// Head-of-line blocking under loss; safe everywhere.
    #[default]
    Reliable,
    /// Unreliable QUIC datagrams + jitter buffer — lower latency under
    /// loss. Requires a `LinkConn`-seam connection (see module docs).
    Datagram,
}

/// One audio frame on the datagram lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDatagram {
    /// Lane sequence number (independent of any RTP sequence).
    pub seq: u32,
    /// Capture timestamp in milliseconds (48 bits on the wire).
    pub timestamp_ms: u64,
    /// Flag bits (bit 0 reserved for “end of talk-spurt”).
    pub flags: u8,
    /// Encoded audio payload (Opus).
    pub payload: Bytes,
}

impl AudioDatagram {
    /// Encode to wire bytes.
    ///
    /// # Errors
    /// [`DatagramLaneError::PayloadTooLarge`] if the payload exceeds
    /// [`MAX_DATAGRAM_PAYLOAD`].
    pub fn encode(&self) -> Result<Bytes, DatagramLaneError> {
        if self.payload.len() > MAX_DATAGRAM_PAYLOAD {
            return Err(DatagramLaneError::PayloadTooLarge {
                len: self.payload.len(),
                max: MAX_DATAGRAM_PAYLOAD,
            });
        }
        let mut buf = BytesMut::with_capacity(DATAGRAM_HEADER_LEN + self.payload.len());
        buf.put_u8(DATAGRAM_VERSION);
        buf.put_u8(self.flags);
        buf.put_u32(self.seq);
        let ts = self.timestamp_ms & TS_MASK;
        buf.put_uint(ts, 6);
        buf.extend_from_slice(&self.payload);
        Ok(buf.freeze())
    }

    /// Decode from wire bytes.
    ///
    /// # Errors
    /// [`DatagramLaneError::TooShort`] / [`DatagramLaneError::BadVersion`].
    pub fn decode(mut data: Bytes) -> Result<Self, DatagramLaneError> {
        use bytes::Buf;
        if data.len() < DATAGRAM_HEADER_LEN {
            return Err(DatagramLaneError::TooShort { len: data.len() });
        }
        let version = data.get_u8();
        if version != DATAGRAM_VERSION {
            return Err(DatagramLaneError::BadVersion(version));
        }
        let flags = data.get_u8();
        let seq = data.get_u32();
        let timestamp_ms = data.get_uint(6);
        Ok(Self {
            seq,
            timestamp_ms,
            flags,
            payload: data,
        })
    }
}

/// Sender half of the lane: stamps sequence numbers and pushes datagrams
/// into a [`LinkConn`].
#[derive(Debug, Default)]
pub struct DatagramAudioLane {
    next_seq: AtomicU32,
}

impl DatagramAudioLane {
    /// Create a lane starting at sequence 0.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stamp the next frame. Sequence numbers wrap at `u32::MAX`.
    pub fn stamp(&self, payload: Bytes, timestamp_ms: u64, flags: u8) -> AudioDatagram {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        AudioDatagram {
            seq,
            timestamp_ms,
            flags,
            payload,
        }
    }

    /// Stamp and send one frame over `conn`.
    ///
    /// # Errors
    /// Encode errors, or [`DatagramLaneError::Send`] from the transport.
    pub fn send_frame<C: LinkConn + ?Sized>(
        &self,
        conn: &C,
        payload: Bytes,
        timestamp_ms: u64,
        flags: u8,
    ) -> Result<u32, DatagramLaneError> {
        let frame = self.stamp(payload, timestamp_ms, flags);
        let seq = frame.seq;
        let wire = frame.encode()?;
        conn.send_datagram(wire)
            .map_err(|e| DatagramLaneError::Send(e.to_string()))?;
        Ok(seq)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn header_round_trip() {
        let d = AudioDatagram {
            seq: 0xDEAD_BEEF,
            timestamp_ms: (1 << 47) + 12345,
            flags: 0b1,
            payload: Bytes::from_static(b"opus-frame"),
        };
        let wire = d.encode().unwrap();
        assert_eq!(wire.len(), DATAGRAM_HEADER_LEN + 10);
        let back = AudioDatagram::decode(wire).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn short_and_bad_version_rejected() {
        assert!(matches!(
            AudioDatagram::decode(Bytes::from_static(b"tiny")),
            Err(DatagramLaneError::TooShort { len: 4 })
        ));
        let mut wire = AudioDatagram {
            seq: 1,
            timestamp_ms: 2,
            flags: 0,
            payload: Bytes::new(),
        }
        .encode()
        .unwrap()
        .to_vec();
        wire[0] = 9;
        assert!(matches!(
            AudioDatagram::decode(Bytes::from(wire)),
            Err(DatagramLaneError::BadVersion(9))
        ));
    }

    #[test]
    fn oversize_payload_rejected() {
        let d = AudioDatagram {
            seq: 0,
            timestamp_ms: 0,
            flags: 0,
            payload: Bytes::from(vec![0u8; MAX_DATAGRAM_PAYLOAD + 1]),
        };
        assert!(matches!(
            d.encode(),
            Err(DatagramLaneError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn lane_stamps_monotonic_sequences() {
        let lane = DatagramAudioLane::new();
        let a = lane.stamp(Bytes::new(), 0, 0);
        let b = lane.stamp(Bytes::new(), 20, 0);
        assert_eq!(a.seq + 1, b.seq);
    }
}
