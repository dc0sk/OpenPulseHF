//! KISS framing: byte-stuffed frame encode/decode.

/// Frame start/end delimiter.
pub const FEND: u8 = 0xC0;
/// Escape character.
const FESC: u8 = 0xDB;
/// Escaped FEND.
const TFEND: u8 = 0xDC;
/// Escaped FESC.
const TFESC: u8 = 0xDD;

/// Type byte for a data frame on port 0.
pub const KISS_DATA: u8 = 0x00;

#[derive(Debug, thiserror::Error)]
pub enum KissError {
    #[error("empty frame")]
    EmptyFrame,
    #[error("invalid escape sequence")]
    InvalidEscape,
}

/// Encode a KISS frame: `FEND | type_byte | escaped(payload) | FEND`.
pub fn encode(type_byte: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 4);
    out.push(FEND);
    out.push(type_byte);
    for &b in payload {
        match b {
            FEND => {
                out.push(FESC);
                out.push(TFEND);
            }
            FESC => {
                out.push(FESC);
                out.push(TFESC);
            }
            _ => out.push(b),
        }
    }
    out.push(FEND);
    out
}

/// Decode a KISS frame body (bytes between FENDs).
///
/// Returns `(type_byte, unescaped_payload)`.
pub fn decode(frame: &[u8]) -> Result<(u8, Vec<u8>), KissError> {
    if frame.is_empty() {
        return Err(KissError::EmptyFrame);
    }
    let type_byte = frame[0];
    let mut payload = Vec::with_capacity(frame.len().saturating_sub(1));
    let mut i = 1;
    while i < frame.len() {
        match frame[i] {
            FESC if i + 1 < frame.len() => match frame[i + 1] {
                TFEND => {
                    payload.push(FEND);
                    i += 2;
                }
                TFESC => {
                    payload.push(FESC);
                    i += 2;
                }
                _ => return Err(KissError::InvalidEscape),
            },
            FESC => return Err(KissError::InvalidEscape),
            b => {
                payload.push(b);
                i += 1;
            }
        }
    }
    Ok((type_byte, payload))
}
