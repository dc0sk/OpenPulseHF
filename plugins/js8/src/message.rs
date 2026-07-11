//! JS8 message-bit assembly (JS8Call `genjs8.f90` head): 72-bit payload + 3-bit flags → 87 LDPC info
//! bits, protected by the CRC-12.
//!
//! `genjs8` builds an 11-byte buffer `[payload(72) | i3bit<<5 (top 3 of a byte) | 0]`, CRC-12s it,
//! XORs the result with 42 ([`crate::crc::JS8_CRC12_XOR`]), and lays the 87 info bits out as
//! `[payload(72) | i3bit(3) | crc(12)]`. The semantic packing of a heartbeat/directed message *into*
//! the 72-bit payload (callsign, grid, command) is the message-grammar layer (a later phase); this
//! unit is the waveform-side assembly that feeds LDPC.

use crate::crc::{augmented_crc12, JS8_CRC12_XOR};
use crate::ldpc174::K;

/// The JS8 message CRC-12 over a 72-bit `payload9` (MSB-first) and 3-bit `i3bit` flags: augmented
/// CRC-12 over the 11-byte `genjs8` buffer, XORed with 42.
pub fn js8_message_crc12(payload9: &[u8; 9], i3bit: u8) -> u16 {
    let mut buf = [0u8; 11];
    buf[..9].copy_from_slice(payload9);
    buf[9] = (i3bit << 5) & 0xE0;
    buf[10] = 0;
    (augmented_crc12(&buf) ^ JS8_CRC12_XOR) & 0x0fff
}

/// Assemble the 87 LDPC info bits (each 0/1) from a 72-bit `payload9` (MSB-first) and 3-bit `i3bit`:
/// `[payload(72) | i3bit(3) | crc12(12)]`.
pub fn js8_info_bits(payload9: &[u8; 9], i3bit: u8) -> [u8; K] {
    let crc = js8_message_crc12(payload9, i3bit);
    let mut bits = [0u8; K];
    for (i, slot) in bits.iter_mut().enumerate().take(72) {
        *slot = (payload9[i / 8] >> (7 - i % 8)) & 1;
    }
    for i in 0..3 {
        bits[72 + i] = (i3bit >> (2 - i)) & 1;
    }
    for i in 0..12 {
        bits[75 + i] = ((crc >> (11 - i)) & 1) as u8;
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::costas::CostasKind;
    use crate::ldpc174::{parity_syndrome, M};
    use crate::tones::{message_to_tones, tones_to_codeword};

    fn payload9(hex: &str) -> [u8; 9] {
        let mut p = [0u8; 9];
        for (i, b) in p.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        p
    }

    /// (i3bit, payload hex, CRC) from the `genjs8` buffer layout + real `boost::augmented_crc<12>` ^ 42.
    const CRC_VECTORS: &[(u8, &str, u16)] = &[
        (6, "fdb86791b77a94985b", 0x81a),
        (0, "2353d01c3fefb845b2", 0x8fa),
        (2, "128f0e66d44dedf2ef", 0x18c),
        (7, "a40aaa3303fdfcfb8c", 0x94c),
        (2, "608cb64bba7fbe359c", 0x6d6),
        (7, "26eb55828db31c79fa", 0x08e),
        (7, "5ebe8349d24cfcd0be", 0xef0),
        (1, "bdfdac3958a91f6485", 0xea6),
        (6, "73eab8a4563dfdde4a", 0xcb0),
        (3, "ab904607dbf85daaec", 0xaa6),
        (0, "000000000000000000", 0x02a),
        (5, "000000000000000000", 0x80e),
    ];

    #[test]
    fn message_crc_matches_genjs8_layout_ground_truth() {
        for (i3bit, hex, want) in CRC_VECTORS {
            assert_eq!(
                js8_message_crc12(&payload9(hex), *i3bit),
                *want,
                "payload {hex}"
            );
        }
    }

    #[test]
    fn all_zero_message_crc_is_the_bare_xor() {
        // Empty buffer CRCs to 0, so the JS8 CRC is exactly the XOR constant, 42 = 0x02a.
        assert_eq!(js8_message_crc12(&[0u8; 9], 0), 0x02a);
    }

    #[test]
    fn info_bits_layout_is_payload_then_flags_then_crc() {
        let p = payload9("128f0e66d44dedf2ef");
        let i3bit = 2u8;
        let bits = js8_info_bits(&p, i3bit);
        // payload
        for (i, b) in bits.iter().enumerate().take(72) {
            assert_eq!(*b, (p[i / 8] >> (7 - i % 8)) & 1, "payload bit {i}");
        }
        // flags
        assert_eq!([bits[72], bits[73], bits[74]], [0, 1, 0]); // i3bit = 2 = 0b010
                                                               // crc
        let crc = js8_message_crc12(&p, i3bit);
        for i in 0..12 {
            assert_eq!(bits[75 + i], ((crc >> (11 - i)) & 1) as u8, "crc bit {i}");
        }
    }

    #[test]
    fn assembled_message_encodes_to_a_parity_valid_codeword() {
        let bits = js8_info_bits(&payload9("a40aaa3303fdfcfb8c"), 7);
        let tones = message_to_tones(&bits, CostasKind::Original);
        assert_eq!(parity_syndrome(&tones_to_codeword(&tones)), [0u8; M]);
    }
}
