//! JS8 free-text decode (JS8Call `varicode.cpp`): the Huffman table + `huffDecode`, and the
//! `unpackDataMessage` frame path that carries directed-message free text.
//!
//! A data frame is one of the 72-bit payloads whose top bit is set (`FrameData`/`FrameDataCompressed`,
//! plan §2.4). Its 72 bits are `[data_flag:1][compressed:1][content..][0][1..pad]`: the leading `1`
//! marks a data frame, the next bit picks the coder, the content runs until a single `0` pad marker
//! followed by all-ones. `compressed = 0` is Huffman (this module); `compressed = 1` is the JSC
//! word-dictionary coder ([`crate::jsc`]). Both decode to free text.
//!
//! Ported and validated against the verbatim upstream `Varicode::unpackDataMessage` /
//! `Varicode::huffDecode` compiled against real Qt5 (see the `tests` module ground-truth vectors).

/// The default JS8 Huffman table (JS8Call `varicode.cpp` `hufftable`): character → prefix-free code.
/// Codes are stored as `'0'`/`'1'` strings exactly as upstream lists them.
pub const HUFF_TABLE: &[(char, &str)] = &[
    (' ', "01"),
    ('E', "100"),
    ('T', "1101"),
    ('A', "0011"),
    ('O', "11111"),
    ('I', "11100"),
    ('N', "10111"),
    ('S', "10100"),
    ('H', "00011"),
    ('R', "00000"),
    ('D', "111011"),
    ('L', "110011"),
    ('C', "110001"),
    ('U', "101101"),
    ('M', "101011"),
    ('W', "001011"),
    ('F', "001001"),
    ('G', "000101"),
    ('Y', "000011"),
    ('P', "1111011"),
    ('B', "1111001"),
    ('.', "1110100"),
    ('V', "1100101"),
    ('K', "1100100"),
    ('-', "1100001"),
    ('+', "1100000"),
    ('?', "1011001"),
    ('!', "1011000"),
    ('"', "1010101"),
    ('X', "1010100"),
    ('0', "0010101"),
    ('J', "0010100"),
    ('1', "0010001"),
    ('Q', "0010000"),
    ('2', "0001001"),
    ('Z', "0001000"),
    ('3', "0000101"),
    ('5', "0000100"),
    ('4', "11110101"),
    ('9', "11110100"),
    ('8', "11110001"),
    ('6', "11110000"),
    ('7', "11101011"),
    ('/', "11101010"),
];

/// Expand a 9-byte payload into its 72 bits, MSB-first within each byte.
fn bits72(p: &[u8; 9]) -> [bool; 72] {
    let mut bits = [false; 72];
    for (i, slot) in bits.iter_mut().enumerate() {
        *slot = (p[i / 8] >> (7 - i % 8)) & 1 == 1;
    }
    bits
}

/// Greedily decode a prefix-free Huffman bit sequence with [`HUFF_TABLE`] (JS8Call `huffDecode`).
/// Stops at the first position no code matches (the upstream `!found` break).
pub fn huff_decode(bits: &[bool]) -> String {
    let mut out = String::new();
    let mut pos = 0;
    'outer: while pos < bits.len() {
        for &(ch, code) in HUFF_TABLE {
            let cb = code.as_bytes();
            if cb.len() <= bits.len() - pos
                && cb
                    .iter()
                    .enumerate()
                    .all(|(i, &b)| (b == b'1') == bits[pos + i])
            {
                out.push(ch);
                pos += cb.len();
                continue 'outer;
            }
        }
        break;
    }
    out
}

/// Decode a 72-bit data-frame payload to its free text (JS8Call `Varicode::unpackDataMessage`).
/// Returns `None` if the payload is not a data frame (top bit clear), else the Huffman- or
/// JSC-decoded text per the `compressed` bit.
pub fn unpack_data_message(p: &[u8; 9]) -> Option<String> {
    let bits = bits72(p);
    if !bits[0] {
        return None; // not a data frame (the `isData` gate)
    }
    // Work in the 71-bit vector after dropping the data flag: b71[k] == bits[1 + k].
    let compressed = bits[1]; // b71[0]

    // `n` = lastIndexOf(0) in b71 — the single `0` pad marker before the all-ones tail.
    let n = (0..71).rev().find(|&k| !bits[1 + k])?;
    // content = b71.mid(1, n-1) == b71[1..n] == bits[2..=n].
    let content: Vec<bool> = (1..n).map(|k| bits[1 + k]).collect();
    Some(if compressed {
        crate::jsc::jsc_decompress(&content)
    } else {
        huff_decode(&content)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex9(s: &str) -> [u8; 9] {
        let mut p = [0u8; 9];
        for (i, b) in p.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
        }
        p
    }

    #[test]
    fn huffman_data_frames_match_upstream() {
        // (payload9, decoded) from verbatim upstream packDataMessage/unpackDataMessage (comp=0 cases).
        for (hex, want) in [
            ("bfec64914c8f9138bf", "OPHF1 A1B2C"),
            ("bfec64914408102043", "OPHF1 ZZZZZ"),
            ("bfec64914a952a54ab", "OPHF1 00000"),
            ("8a911217d427875f8b", "012345678"),
            ("bfec64916a06208897", "OPHF1 XYZ12"),
        ] {
            assert_eq!(
                unpack_data_message(&hex9(hex)),
                Some(want.to_string()),
                "huffman decode of {hex}"
            );
        }
    }

    #[test]
    fn non_data_frame_returns_none() {
        // A heartbeat payload (top bit clear) is not a data frame.
        assert_eq!(unpack_data_message(&hex9("0a2fb3a3ee2ee2ea58")), None);
    }

    #[test]
    fn huff_decode_is_prefix_free_and_stops_cleanly() {
        // " " = 01, "E" = 100 → "01" + "100" decodes to " E".
        assert_eq!(huff_decode(&[false, true, true, false, false]), " E");
        // Empty input decodes to empty.
        assert_eq!(huff_decode(&[]), "");
    }
}
