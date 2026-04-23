use crate::error::ModemError;

// Magic bytes that start every OpenPulse frame on the wire.
const MAGIC: &[u8; 4] = b"OPLS";
const VERSION: u8 = 1;

/// An OpenPulse data frame.
///
/// Frames are the atomic unit transmitted over the air.  The wire format is:
///
/// ```text
/// ┌────────┬─────────┬──────────────────┬─────────────┬─────────┬───────────┐
/// │ magic  │ version │ sequence (16-bit) │ length (8b) │ payload │ CRC-16    │
/// │ 4 B    │ 1 B     │ 2 B              │ 1 B         │ 0–255 B │ 2 B       │
/// └────────┴─────────┴──────────────────┴─────────────┴─────────┴───────────┘
/// ```
///
/// The CRC-16/CCITT covers everything from the magic bytes through the last
/// payload byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Monotonically increasing sequence number (wraps at 65 535).
    pub sequence: u16,
    /// User data – maximum 255 bytes per frame.
    pub payload: Vec<u8>,
}

impl Frame {
    /// Create a new frame.
    ///
    /// # Panics
    /// Panics if `payload.len() > 255`.
    pub fn new(sequence: u16, payload: Vec<u8>) -> Self {
        assert!(payload.len() <= 255, "payload must be ≤ 255 bytes");
        Self { sequence, payload }
    }

    /// Serialise the frame to bytes ready for the modulator.
    pub fn encode(&self) -> Vec<u8> {
        let header_len = MAGIC.len() + 1 + 2 + 1; // magic + ver + seq + len
        let total = header_len + self.payload.len() + 2; // + CRC

        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.push((self.sequence >> 8) as u8);
        out.push(self.sequence as u8);
        out.push(self.payload.len() as u8);
        out.extend_from_slice(&self.payload);

        let crc = crc16(&out);
        out.push((crc >> 8) as u8);
        out.push(crc as u8);
        out
    }

    /// Deserialise a frame from bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, ModemError> {
        let min_len = MAGIC.len() + 1 + 2 + 1 + 2; // header + empty payload + CRC
        if bytes.len() < min_len {
            return Err(ModemError::Frame("frame too short".into()));
        }

        if &bytes[..4] != MAGIC {
            return Err(ModemError::Frame("invalid magic".into()));
        }
        if bytes[4] != VERSION {
            return Err(ModemError::Frame(format!(
                "unsupported version {}",
                bytes[4]
            )));
        }

        let sequence = ((bytes[5] as u16) << 8) | bytes[6] as u16;
        let payload_len = bytes[7] as usize;

        let body_end = 8 + payload_len;
        if bytes.len() < body_end + 2 {
            return Err(ModemError::Frame("frame truncated".into()));
        }

        let expected_crc = crc16(&bytes[..body_end]);
        let actual_crc = ((bytes[body_end] as u16) << 8) | bytes[body_end + 1] as u16;
        if expected_crc != actual_crc {
            return Err(ModemError::Frame(format!(
                "CRC mismatch (expected {expected_crc:#06x}, got {actual_crc:#06x})"
            )));
        }

        Ok(Self {
            sequence,
            payload: bytes[8..body_end].to_vec(),
        })
    }
}

// ── CRC-16/CCITT (polynomial 0x1021, initial value 0xFFFF) ───────────────────

/// Compute CRC-16/CCITT over `data`.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_empty_payload() {
        let f = Frame::new(0, vec![]);
        assert_eq!(Frame::decode(&f.encode()).unwrap(), f);
    }

    #[test]
    fn round_trip_with_payload() {
        let f = Frame::new(1234, b"Hello, OpenPulse!".to_vec());
        let encoded = f.encode();
        let decoded = Frame::decode(&encoded).unwrap();
        assert_eq!(decoded.sequence, 1234);
        assert_eq!(decoded.payload, b"Hello, OpenPulse!");
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut bytes = Frame::new(0, b"test".to_vec()).encode();
        bytes[0] = 0xFF;
        assert!(Frame::decode(&bytes).is_err());
    }

    #[test]
    fn corrupted_crc_is_rejected() {
        let mut bytes = Frame::new(0, b"test".to_vec()).encode();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        assert!(Frame::decode(&bytes).is_err());
    }

    #[test]
    fn sequence_number_round_trips() {
        for seq in [0u16, 1, 0x00FF, 0xFF00, 0xFFFF] {
            let f = Frame::new(seq, b"x".to_vec());
            assert_eq!(Frame::decode(&f.encode()).unwrap().sequence, seq);
        }
    }
}
