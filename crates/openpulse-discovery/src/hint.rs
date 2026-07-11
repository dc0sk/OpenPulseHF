//! The `@OPULSE` capability hint (plan §3.2 / §5.4).
//!
//! An OpenPulse station marks itself among ordinary JS8 traffic with a directed free-text message to
//! the custom group `@OPULSE` beginning with the standalone token `OPHF<version>` followed by 8
//! base-36 chars encoding a 40-bit payload:
//!
//! ```text
//! DC0SK: @OPULSE OPHF1 XXXXXXXX
//! ```
//!
//! 40-bit payload: `caps:16 | pref_channel:6 | listen_submode:3 | reserved:7 | check:8`, where
//! `check` is a CRC-8 over the low 32 bits **salted with the sender callsign** — so a random text can
//! not collide and a copy-pasted payload from another station fails to verify. This is enough to
//! *filter and initiate*; peer authentication happens after QSY via the signed CONREQ/CONACK.

/// Hint magic prefix; the full token is `OPHF<version>`.
pub const HINT_MAGIC: &str = "OPHF";
/// Hint format version this codec emits.
pub const HINT_VERSION: u8 = 1;
/// The JS8 custom group the hint is addressed to.
pub const OPULSE_GROUP: &str = "OPULSE";

/// Base-36 alphabet (uppercase; survives JS8's normalization, cheap in varicode).
const BASE36: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
/// Payload length in base-36 chars (40 bits ≤ 36^8).
const PAYLOAD_CHARS: usize = 8;

/// The decoded hint capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HintPayload {
    /// Low 16 bits of the peer's `capability_mask` (speaks-HPX / QSY / PQ / relay …).
    pub caps: u16,
    /// Preferred rendezvous channel index (0..=63).
    pub pref_channel: u8,
    /// Preferred listen submode (0..=7).
    pub listen_submode: u8,
}

/// CRC-8/SMBUS (poly 0x07, init 0) over `data` then the upper-cased `salt` bytes.
fn crc8_salted(data: &[u8], salt: &str) -> u8 {
    let mut crc = 0u8;
    let feed = |b: u8, crc: &mut u8| {
        *crc ^= b;
        for _ in 0..8 {
            *crc = if *crc & 0x80 != 0 {
                (*crc << 1) ^ 0x07
            } else {
                *crc << 1
            };
        }
    };
    for &b in data {
        feed(b, &mut crc);
    }
    for b in salt.trim().to_ascii_uppercase().bytes() {
        feed(b, &mut crc);
    }
    crc
}

/// Pack the 40-bit payload (low 32 bits = body, high 8 = salted CRC).
fn payload_value(p: &HintPayload, callsign: &str) -> u64 {
    let body: u32 = (p.caps as u32)
        | ((p.pref_channel as u32 & 0x3f) << 16)
        | ((p.listen_submode as u32 & 0x7) << 22);
    let check = crc8_salted(&body.to_le_bytes(), callsign);
    body as u64 | ((check as u64) << 32)
}

/// Encode a 40-bit value as `PAYLOAD_CHARS` base-36 chars (most-significant char first).
fn to_base36(mut value: u64) -> String {
    let mut out = [b'0'; PAYLOAD_CHARS];
    for slot in out.iter_mut().rev() {
        *slot = BASE36[(value % 36) as usize];
        value /= 36;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Decode `PAYLOAD_CHARS` base-36 chars back to a value, or `None` on a bad char / length.
fn from_base36(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != PAYLOAD_CHARS {
        return None;
    }
    let mut value = 0u64;
    for &c in b {
        let d = BASE36.iter().position(|&x| x == c.to_ascii_uppercase())?;
        value = value * 36 + d as u64;
    }
    Some(value)
}

/// Build the hint free text a station sends: `OPHF<version> XXXXXXXX` (the `@OPULSE` group addressing
/// is applied by the JS8 message layer).
pub fn encode_hint(p: &HintPayload, callsign: &str) -> String {
    format!(
        "{HINT_MAGIC}{HINT_VERSION} {}",
        to_base36(payload_value(p, callsign))
    )
}

/// Detect and decode a hint from a station's free text. Requires the standalone `OPHF<version>` token
/// (this codec's version), a valid 8-char base-36 payload, and a CRC that verifies against
/// `callsign` — all three must hold, so organic JS8 text can not be mistaken for a hint. Returns the
/// capabilities, or `None`.
pub fn decode_hint(text: &str, callsign: &str) -> Option<HintPayload> {
    let mut tokens = text.split_whitespace();
    let tok = tokens.next()?;
    let payload = tokens.next()?;
    // Token must be exactly OPHF<our-version>.
    let ver = tok.strip_prefix(HINT_MAGIC)?;
    if ver.parse::<u8>().ok()? != HINT_VERSION {
        return None;
    }
    let value = from_base36(payload)?;
    let body = value as u32;
    let check = (value >> 32) as u8;
    if crc8_salted(&body.to_le_bytes(), callsign) != check {
        return None;
    }
    Some(HintPayload {
        caps: body as u16,
        pref_channel: ((body >> 16) & 0x3f) as u8,
        listen_submode: ((body >> 22) & 0x7) as u8,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint() -> HintPayload {
        HintPayload {
            caps: 0xB105,
            pref_channel: 42,
            listen_submode: 1,
        }
    }

    #[test]
    fn round_trips() {
        let text = encode_hint(&hint(), "DC0SK");
        assert!(text.starts_with("OPHF1 "));
        assert_eq!(text.split_whitespace().nth(1).unwrap().len(), 8);
        assert_eq!(decode_hint(&text, "DC0SK"), Some(hint()));
    }

    #[test]
    fn crc_binds_to_the_sender_callsign() {
        // A payload minted by DC0SK must not verify as if sent by W1AW (kills copy-paste replay).
        let text = encode_hint(&hint(), "DC0SK");
        assert_eq!(decode_hint(&text, "W1AW"), None);
        // Case/whitespace-insensitive on the callsign salt.
        assert_eq!(decode_hint(&text, " dc0sk "), Some(hint()));
    }

    #[test]
    fn rejects_non_hint_text() {
        assert_eq!(decode_hint("HELLO WORLD", "DC0SK"), None); // not the token
        assert_eq!(decode_hint("OPHF1", "DC0SK"), None); // no payload
        assert_eq!(decode_hint("OPHF2 ABCDEFGH", "DC0SK"), None); // wrong version
        assert_eq!(decode_hint("OPHF1 SHORT", "DC0SK"), None); // bad length
        assert_eq!(decode_hint("OPHF1 ABCDEFG!", "DC0SK"), None); // bad char
                                                                  // A valid-looking but random payload almost never passes the CRC.
        assert_eq!(decode_hint("OPHF1 ZZZZZZZZ", "DC0SK"), None);
    }

    #[test]
    fn all_fields_survive_at_their_bit_widths() {
        let p = HintPayload {
            caps: 0xFFFF,
            pref_channel: 63,
            listen_submode: 7,
        };
        assert_eq!(decode_hint(&encode_hint(&p, "N0CALL"), "N0CALL"), Some(p));
    }
}
