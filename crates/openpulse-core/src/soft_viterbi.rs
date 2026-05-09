//! K=7, rate-1/2 convolutional codec with soft-decision Viterbi decoder (BL-FEC-5).
//!
//! Uses NASA/3GPP standard generators G0=0o133 (0b1011011), G1=0o171 (0b1111001).
//! The 64-state trellis provides ~5 dB coding gain over the K=3 hard-decision
//! `ConvCodec` when fed true LLRs from `ModulationPlugin::demodulate_soft()`.
//!
//! # Wire format
//!
//! Identical to `ConvCodec`: a 4-byte big-endian original-length prefix is
//! prepended to the payload **before** convolutional encoding.  The prefix is
//! therefore FEC-protected, and the decoder uses it to trim trailing bits left
//! by byte-alignment padding after decoding.
//!
//! # LLR convention
//!
//! **Positive = bit more likely 0**, negative = bit more likely 1.  The branch
//! metric for each trellis branch is `sign(llr) * |llr|` when the expected bit
//! matches the received tendency, negative otherwise.
//!
//! # Bit ordering
//!
//! LSB-first within each byte (bit 0 transmitted first), matching the convention
//! used by the BPSK and QPSK modulator/demodulator plugins.

const K: usize = 7;
const N_STATES: usize = 1 << (K - 1); // 64
const G0: u8 = 0b1011011; // 0o133
const G1: u8 = 0b1111001; // 0o171
const FLUSH: usize = K - 1; // 6 tail bits to drain the shift register

/// K=7, rate-1/2 convolutional codec with a soft-decision Viterbi decoder.
pub struct SoftViterbiCodec;

impl SoftViterbiCodec {
    /// Encode `data` bytes.
    ///
    /// Prepends a 4-byte big-endian original-length prefix (same framing as
    /// `ConvCodec`) then convolves at rate 1/2.  Output is approximately
    /// `2 * (4 + data.len())` bytes.
    pub fn encode(&self, data: &[u8]) -> Vec<u8> {
        // 4-byte BE length prefix — protected by the convolutional code.
        let mut src: Vec<u8> = Vec::with_capacity(4 + data.len());
        src.extend_from_slice(&(data.len() as u32).to_be_bytes());
        src.extend_from_slice(data);

        let bits = unpack_lsb(&src);
        // Append K-1 flush zero bits to drive the encoder to state 0.
        let total = bits.len() + FLUSH;
        let mut all_bits: Vec<u8> = Vec::with_capacity(total);
        all_bits.extend_from_slice(&bits);
        all_bits.extend(std::iter::repeat_n(0u8, FLUSH));

        let mut sr: u8 = 0; // 6-bit shift register (state)
        let mut enc: Vec<u8> = Vec::with_capacity(total * 2);
        for &b in &all_bits {
            // Compute encoder output BEFORE updating the shift register.
            // word[6] = new input bit; word[5..0] = current state.
            let word = (b << (K as u8 - 1)) | sr;
            sr = (sr >> 1) | (b << (K as u8 - 2));
            enc.push(parity(word & G0));
            enc.push(parity(word & G1));
        }
        pack_lsb(&enc)
    }

    /// Soft-decision Viterbi decode.
    ///
    /// `llrs` holds one `f32` per encoded bit (2 × total encoded bits before
    /// packing, LSB-first), with **positive = more likely 0**.
    ///
    /// Returns the decoded payload bytes without the length prefix.  Returns an
    /// empty vec if the LLR slice is too short to hold a valid frame.
    pub fn decode_soft(&self, llrs: &[f32]) -> Vec<u8> {
        let n_pairs = llrs.len() / 2;
        if n_pairs < 32 + FLUSH {
            // Too short to contain even the 4-byte prefix + flush tail.
            return vec![];
        }

        const NEG_INF: f32 = f32::NEG_INFINITY;
        let mut pm = [NEG_INF; N_STATES];
        pm[0] = 0.0; // start from state 0

        // traceback[step][state] = input bit that led here
        let mut tb_bit: Vec<[u8; N_STATES]> = Vec::with_capacity(n_pairs);
        // traceback[step][state] = predecessor state
        let mut tb_from: Vec<[u8; N_STATES]> = Vec::with_capacity(n_pairs);

        for pair in 0..n_pairs {
            let llr0 = llrs[pair * 2];
            let llr1 = llrs[pair * 2 + 1];

            let mut new_pm = [NEG_INF; N_STATES];
            let mut new_bit = [0u8; N_STATES];
            let mut new_from = [0u8; N_STATES];

            for (s, &cur_pm) in pm.iter().enumerate() {
                if cur_pm == NEG_INF {
                    continue;
                }
                for input in 0u8..2 {
                    let next = (s >> 1) | ((input as usize) << (K - 2));
                    let word = (input << (K as u8 - 1)) | s as u8;
                    let c0 = parity(word & G0);
                    let c1 = parity(word & G1);
                    let bm = branch_metric(llr0, c0) + branch_metric(llr1, c1);
                    let candidate = cur_pm + bm;
                    if candidate > new_pm[next] {
                        new_pm[next] = candidate;
                        new_bit[next] = input;
                        new_from[next] = s as u8;
                    }
                }
            }
            pm = new_pm;
            tb_bit.push(new_bit);
            tb_from.push(new_from);
        }

        // Traceback from highest-metric terminal state.
        let mut state = pm
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);

        let mut decoded: Vec<u8> = vec![0u8; n_pairs];
        for step in (0..n_pairs).rev() {
            decoded[step] = tb_bit[step][state];
            state = tb_from[step][state] as usize;
        }

        // Strip the FLUSH tail bits.
        let data_bits = &decoded[..decoded.len().saturating_sub(FLUSH)];

        // Read the 4-byte BE length prefix from the first 32 bits.
        if data_bits.len() < 32 {
            return vec![];
        }
        let orig_len = bits_be_to_u32(&data_bits[..32]) as usize;
        let want_bits = 32 + orig_len * 8;
        if data_bits.len() < want_bits {
            return vec![];
        }
        pack_lsb(&data_bits[32..want_bits])
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

#[inline]
fn branch_metric(llr: f32, encoded_bit: u8) -> f32 {
    // Positive LLR → bit=0 likely; encoded_bit=0 → reward (+llr), else penalise.
    if encoded_bit == 0 {
        llr
    } else {
        -llr
    }
}

#[inline]
fn parity(mut x: u8) -> u8 {
    x ^= x >> 4;
    x ^= x >> 2;
    x ^= x >> 1;
    x & 1
}

/// Unpack bytes into LSB-first bit vector (each element is 0 or 1).
fn unpack_lsb(bytes: &[u8]) -> Vec<u8> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for shift in 0..8u8 {
            bits.push((b >> shift) & 1);
        }
    }
    bits
}

/// Pack LSB-first bit vector into bytes.
fn pack_lsb(bits: &[u8]) -> Vec<u8> {
    bits.chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |acc, (i, &b)| acc | (b << i))
        })
        .collect()
}

/// Interpret the first 32 bits (LSB-first per byte, BE byte order) as a u32 length.
/// The prefix was encoded as 4-byte BE, then unpacked LSB-first per byte, so the
/// first 8 decoded bits are the LSB-first bits of the MSB byte of the length.
fn bits_be_to_u32(bits: &[u8]) -> u32 {
    // Reconstruct 4 bytes from 32 LSB-first bits (8 bits per byte).
    let mut val = 0u32;
    for byte_idx in 0..4 {
        let mut byte_val = 0u32;
        for bit_idx in 0..8 {
            byte_val |= (bits[byte_idx * 8 + bit_idx] as u32) << bit_idx;
        }
        // BE: first byte is most significant.
        val = (val << 8) | byte_val;
    }
    val
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn to_llrs(encoded: &[u8]) -> Vec<f32> {
        unpack_lsb(encoded)
            .iter()
            .map(|&b| if b == 0 { 1.0f32 } else { -1.0f32 })
            .collect()
    }

    #[test]
    fn clean_round_trip_short() {
        let codec = SoftViterbiCodec;
        let payload = b"Test";
        let encoded = codec.encode(payload);
        let llrs = to_llrs(&encoded);
        let decoded = codec.decode_soft(&llrs);
        assert_eq!(decoded.as_slice(), payload.as_ref());
    }

    #[test]
    fn clean_round_trip_32_bytes() {
        let codec = SoftViterbiCodec;
        let payload: Vec<u8> = (0u8..32).collect();
        let encoded = codec.encode(&payload);
        let llrs = to_llrs(&encoded);
        let decoded = codec.decode_soft(&llrs);
        assert_eq!(decoded, payload);
    }

    #[test]
    fn clean_round_trip_empty() {
        let codec = SoftViterbiCodec;
        let encoded = codec.encode(&[]);
        let llrs = to_llrs(&encoded);
        let decoded = codec.decode_soft(&llrs);
        assert_eq!(decoded, &[] as &[u8]);
    }

    #[test]
    fn rate_approximately_two_to_one() {
        let codec = SoftViterbiCodec;
        let payload = vec![0u8; 100];
        let encoded = codec.encode(&payload);
        // (4-byte prefix + 100 bytes) * 2 rate ≈ 208 bytes, plus small tail overhead.
        assert!(encoded.len() >= 208 && encoded.len() <= 216);
    }

    #[test]
    fn single_bit_error_corrected() {
        let codec = SoftViterbiCodec;
        let payload = b"OpenPulse";
        let encoded = codec.encode(payload);
        let mut llrs = to_llrs(&encoded);
        // Flip one LLR to simulate a channel error.
        llrs[11] = -llrs[11];
        let decoded = codec.decode_soft(&llrs);
        assert_eq!(decoded.as_slice(), payload.as_ref());
    }
}
