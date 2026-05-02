//! ACK frame taxonomy for HPX sessions.
//!
//! ACK frames carry control information between IRS and ISS.  They are
//! transported over a separate 4FSK waveform (see `fsk4-plugin`) that is
//! decodable at lower SNR than the data modulation, giving ≈ 6 dB headroom.
//!
//! ## Wire layout (5 bytes)
//!
//! ```text
//! byte 0: ACK type [2:0], reserved [7:3]
//! bytes 1–2: session_hash u16 big-endian  (anti-collision)
//! byte 3: reserved (zero)
//! byte 4: CRC-8/SMBUS over bytes 0–3
//! ```

use serde::{Deserialize, Serialize};

use crate::error::AckError;

// ── AckType ───────────────────────────────────────────────────────────────────

/// HPX ACK frame type (3-bit wire code).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum AckType {
    /// Data received correctly; maintain current speed level.
    AckOk = 0b000,
    /// Data received correctly; request one speed-level step up.
    AckUp = 0b001,
    /// Data received correctly; request one speed-level step down.
    AckDown = 0b010,
    /// Data received with uncorrectable errors; retransmit at current rate.
    Nack = 0b011,
    /// Changeover: IRS requests to become ISS.
    Break = 0b100,
    /// ACK lost; repeat last data frame.
    Req = 0b101,
    /// Graceful session end.
    Qrt = 0b110,
    /// Abnormal teardown.
    Abort = 0b111,
}

impl AckType {
    fn from_u8(v: u8) -> Result<Self, AckError> {
        match v {
            0 => Ok(Self::AckOk),
            1 => Ok(Self::AckUp),
            2 => Ok(Self::AckDown),
            3 => Ok(Self::Nack),
            4 => Ok(Self::Break),
            5 => Ok(Self::Req),
            6 => Ok(Self::Qrt),
            7 => Ok(Self::Abort),
            _ => Err(AckError::InvalidAckType(v)),
        }
    }
}

// ── AckFrame ──────────────────────────────────────────────────────────────────

/// Five-byte ACK frame payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckFrame {
    /// Frame type.
    pub ack_type: AckType,
    /// 16-bit FNV-1a hash of the session ID for anti-collision filtering.
    pub session_hash: u16,
}

impl AckFrame {
    /// Create a frame with the session hash computed from `session_id`.
    pub fn new(ack_type: AckType, session_id: &str) -> Self {
        Self {
            ack_type,
            session_hash: Self::hash_session_id(session_id),
        }
    }

    /// Encode to the 5-byte wire representation.
    pub fn encode(&self) -> [u8; 5] {
        let mut b = [0u8; 5];
        b[0] = self.ack_type as u8;
        b[1] = (self.session_hash >> 8) as u8;
        b[2] = self.session_hash as u8;
        b[3] = 0;
        b[4] = crc8(&b[..4]);
        b
    }

    /// Decode from the 5-byte wire representation.
    pub fn decode(b: &[u8; 5]) -> Result<Self, AckError> {
        let expected = crc8(&b[..4]);
        if b[4] != expected {
            return Err(AckError::CrcMismatch {
                expected,
                got: b[4],
            });
        }
        let ack_type = AckType::from_u8(b[0] & 0x07)?;
        let session_hash = ((b[1] as u16) << 8) | b[2] as u16;
        Ok(Self {
            ack_type,
            session_hash,
        })
    }

    /// Compute a 16-bit FNV-1a hash of `session_id` for anti-collision use.
    pub fn hash_session_id(session_id: &str) -> u16 {
        let mut hash: u32 = 2_166_136_261;
        for byte in session_id.bytes() {
            hash ^= byte as u32;
            hash = hash.wrapping_mul(16_777_619);
        }
        (hash ^ (hash >> 16)) as u16
    }
}

// ── CRC-8/SMBUS ───────────────────────────────────────────────────────────────

fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ack_frame_round_trip_all_types() {
        for t in [
            AckType::AckOk,
            AckType::AckUp,
            AckType::AckDown,
            AckType::Nack,
            AckType::Break,
            AckType::Req,
            AckType::Qrt,
            AckType::Abort,
        ] {
            let f = AckFrame {
                ack_type: t,
                session_hash: 0xABCD,
            };
            let b = f.encode();
            assert_eq!(AckFrame::decode(&b).unwrap(), f);
        }
    }

    #[test]
    fn ack_frame_crc_mismatch_detected() {
        let mut b = AckFrame {
            ack_type: AckType::AckOk,
            session_hash: 0,
        }
        .encode();
        b[4] ^= 0xFF;
        assert!(matches!(
            AckFrame::decode(&b),
            Err(AckError::CrcMismatch { .. })
        ));
    }

    #[test]
    fn session_hash_is_deterministic() {
        assert_eq!(
            AckFrame::hash_session_id("sess-abc"),
            AckFrame::hash_session_id("sess-abc")
        );
    }

    #[test]
    fn different_sessions_have_different_hashes() {
        assert_ne!(
            AckFrame::hash_session_id("sess-A"),
            AckFrame::hash_session_id("sess-B")
        );
    }

    #[test]
    fn new_constructor_hashes_session_id() {
        let f = AckFrame::new(AckType::AckUp, "session-xyz");
        assert_eq!(f.session_hash, AckFrame::hash_session_id("session-xyz"));
    }
}
