//! Majority-protected little-endian length prefix for block-modem frames.
//!
//! OFDM and SC-FDMA frames carry their payload length in-band.  A bare 2-byte
//! prefix has no integrity protection: a single bit error silently truncates
//! or inflates the frame *before* FEC sees it.  The prefix is therefore
//! transmitted as three identical LE copies (6 bytes) and decoded by per-bit
//! majority vote — any single-copy corruption of each bit position is
//! corrected, and the soft decoder combines the three copies' LLRs for an
//! additional ~4.8 dB of prefix protection.

/// Encoded prefix size in bytes (three 2-byte LE copies).
pub const LEN_PREFIX_BYTES: usize = 6;
/// Encoded prefix size in bits.
pub const LEN_PREFIX_BITS: usize = LEN_PREFIX_BYTES * 8;

/// Whitening masks applied to the three copies before transmission.
///
/// Bare repetition makes the prefix bits periodic, which on DFT-spread
/// waveforms (SC-FDMA) concentrates the first symbol's energy on a few
/// subcarriers and raises its PAPR by ~2 dB.  XOR-whitening the second and
/// third copies breaks the periodicity; the masks are undone before voting.
const COPY_MASKS: [u16; 3] = [0x0000, 0x5AA5, 0xC33C];

/// Encode `len` as three whitened little-endian copies.
pub fn encode_len_prefix(len: u16) -> [u8; LEN_PREFIX_BYTES] {
    let mut out = [0u8; LEN_PREFIX_BYTES];
    for (k, mask) in COPY_MASKS.iter().enumerate() {
        let le = (len ^ mask).to_le_bytes();
        out[2 * k] = le[0];
        out[2 * k + 1] = le[1];
    }
    out
}

/// Decode a majority-voted length from at least [`LEN_PREFIX_BYTES`] bytes.
///
/// Each of the 16 length bits is taken as the majority of its three
/// (de-whitened) copies.  Returns `None` when fewer than
/// [`LEN_PREFIX_BYTES`] bytes are available.
pub fn decode_len_prefix(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < LEN_PREFIX_BYTES {
        return None;
    }
    let copy = |k: usize| u16::from_le_bytes([bytes[2 * k], bytes[2 * k + 1]]) ^ COPY_MASKS[k];
    let (a, b, c) = (copy(0), copy(1), copy(2));
    // Bitwise majority of three words.
    Some((a & b) | (a & c) | (b & c))
}

/// Decode the length from at least [`LEN_PREFIX_BITS`] LLRs (LSB-first per
/// byte, positive = bit 0), soft-combining the three copies per bit position.
///
/// Whitened bits are sign-flipped before combining (mask bit 1 inverts the
/// transmitted bit, which negates its LLR contribution).
pub fn decode_len_prefix_llrs(llrs: &[f32]) -> Option<u16> {
    if llrs.len() < LEN_PREFIX_BITS {
        return None;
    }
    let mut len = 0u16;
    for bit in 0..16 {
        let combined: f32 = COPY_MASKS
            .iter()
            .enumerate()
            .map(|(k, mask)| {
                let sign = if (mask >> bit) & 1 == 1 { -1.0 } else { 1.0 };
                sign * llrs[16 * k + bit]
            })
            .sum();
        if combined < 0.0 {
            len |= 1 << bit;
        }
    }
    Some(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_clean() {
        for len in [0u16, 1, 64, 255, 256, u16::MAX] {
            let enc = encode_len_prefix(len);
            assert_eq!(decode_len_prefix(&enc), Some(len));
        }
    }

    #[test]
    fn corrects_any_single_copy_corruption() {
        let len = 0x1234u16;
        for copy in 0..3 {
            let mut enc = encode_len_prefix(len);
            // Trash one full copy.
            enc[2 * copy] ^= 0xFF;
            enc[2 * copy + 1] ^= 0xFF;
            assert_eq!(decode_len_prefix(&enc), Some(len), "copy {copy}");
        }
    }

    #[test]
    fn llr_decode_combines_copies() {
        let len = 0x00C8u16; // 200
        let enc = encode_len_prefix(len);
        // Strong correct LLRs for copies 1 and 2; copy 0 inverted but weak.
        let mut llrs = Vec::with_capacity(LEN_PREFIX_BITS);
        for (byte_idx, &b) in enc.iter().enumerate() {
            for bit in 0..8 {
                let is_one = (b >> bit) & 1 == 1;
                let mag = if byte_idx < 2 { 0.4 } else { 2.0 };
                let sign = if is_one { -1.0 } else { 1.0 };
                let flip = if byte_idx < 2 { -1.0 } else { 1.0 };
                llrs.push(sign * mag * flip);
            }
        }
        assert_eq!(decode_len_prefix_llrs(&llrs), Some(len));
    }

    #[test]
    fn short_input_returns_none() {
        assert_eq!(decode_len_prefix(&[0u8; 5]), None);
        assert_eq!(decode_len_prefix_llrs(&[0.0; 47]), None);
    }
}
