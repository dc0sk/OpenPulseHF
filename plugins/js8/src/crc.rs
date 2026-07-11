//! CRC-12 primitive (JS8Call `lib/crc12.cpp` = `boost::augmented_crc<12, 0xc06>`).
//!
//! JS8 protects each 75-bit message with a 12-bit CRC (giving the 87 info bits the LDPC(174,87) code
//! encodes). Upstream uses Boost's *augmented* CRC — an unreflected, MSB-first modulo-2 division with
//! the check bits augmented into the message, so appending the CRC makes the whole word divide to
//! zero. This port is validated **bit-exactly against ground-truth vectors emitted by the real
//! `boost::augmented_crc<12, 0xc06>`** (see the `tests` module).
//!
//! The JS8 message CRC additionally XORs the result with [`JS8_CRC12_XOR`] and is computed over an
//! 11-byte buffer assembled by the frame layer (`genjs8.f90`); that assembly + XOR lives with frame
//! packing so it is tested alongside the payload/flag layout it depends on.

/// Truncated CRC-12 polynomial (`0xc06`, JS8Call `crc12.cpp` `POLY`).
pub const CRC12_POLY: u16 = 0xc06;

/// JS8 post-CRC XOR (`genjs8.f90`: `icrc12 = xor(icrc12, 42)`).
pub const JS8_CRC12_XOR: u16 = 42;

/// Boost-equivalent augmented CRC-12 over `data`, MSB-first, initial remainder 0.
///
/// Matches `boost::augmented_crc<12, 0xc06>` (JS8Call `crc12.cpp`). Appending the returned 12-bit CRC
/// into a buffer's trailing 12 bits makes this function return 0 for the whole buffer (the augmented
/// self-check `crc12_check` relies on).
pub fn augmented_crc12(data: &[u8]) -> u16 {
    let poly = CRC12_POLY as u32;
    let mut rem: u32 = 0;
    for &byte in data {
        for i in (0..8).rev() {
            let bit = ((byte >> i) & 1) as u32;
            let quotient = (rem >> 11) & 1;
            rem = ((rem << 1) | bit) & 0xfff;
            if quotient != 0 {
                rem ^= poly;
            }
        }
    }
    (rem & 0xfff) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hexb(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// (buffer hex, CRC-12) pairs emitted by the real `boost::augmented_crc<12, 0xc06>`. A single
    /// wrong bit in the port fails at least one of these.
    const VECTORS: &[(&str, u16)] = &[
        ("4d4a6595cef16af7d278bd", 0x00fb),
        ("14bf1b0c1c24b3dc8ed1e1", 0x0b63),
        ("59535f51044f0dcd990f8a", 0x0da6),
        ("9494b228cebe85f010d83e", 0x09ea),
        ("b7fa3a5857dc1e1bfaa232", 0x0b98),
        ("19c993b71cfa3d02dc7450", 0x050a),
        ("0baa1af48c469bf29e0207", 0x0529),
        ("a37b9054067358804a4126", 0x044c),
        ("53ecea8f16fa329bbcdbcc", 0x0ff6),
        ("569e8ebddf7a89d2a1af78", 0x01ae),
        ("78a9556ca7832f1a2f4194", 0x0c86),
        ("8af6a58f52e7ee8416b8ad", 0x0e61),
        ("e3", 0x00e3),
        ("e65a", 0x066a),
        ("531514", 0x0694),
        ("94f2c5a910", 0x032c),
        ("5f1072cb2d795ed5", 0x0039),
        ("515821c8b96b5ff79e0d", 0x074d),
        ("d4ff4ec8564fa2232f750d944eba0d9b", 0x0d51),
        ("50eb0a2ca1443833b73a70e05df5a2e3bd1596dccab3", 0x035b),
    ];

    #[test]
    fn matches_boost_ground_truth_vectors() {
        for (hex, want) in VECTORS {
            assert_eq!(augmented_crc12(&hexb(hex)), *want, "buffer {hex}");
        }
    }

    #[test]
    fn empty_and_zero_inputs() {
        assert_eq!(augmented_crc12(&[]), 0);
        assert_eq!(augmented_crc12(&[0u8; 11]), 0);
    }

    #[test]
    fn appending_the_crc_makes_the_word_divide_to_zero() {
        // The augmented self-check: put the 12-bit CRC in the trailing 12 bits (byte 9 low nibble +
        // byte 10) of an 11-byte buffer whose augmentation space started zero → the CRC of the whole
        // buffer is 0. This is the property JS8Call's `crc12_check` uses.
        let mut buf = [
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x00, 0x00,
        ];
        buf[9] &= 0xf0; // clear the augmentation space
        buf[10] = 0;
        let c = augmented_crc12(&buf);
        buf[9] = (buf[9] & 0xf0) | ((c >> 8) as u8 & 0x0f);
        buf[10] = (c & 0xff) as u8;
        assert_eq!(augmented_crc12(&buf), 0, "appended CRC must divide to zero");
    }
}
