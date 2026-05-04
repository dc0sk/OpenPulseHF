//! Rate-1/2 convolutional FEC codec with hard-decision Viterbi decoder.
//!
//! Parameters: K=3 (4-state), generators G0=0b111, G1=0b101 (octal 7,5).
//! This matches the ARDOP rate-2/3 inner code at its base rate before puncturing,
//! and is used here as a benchmark proxy for Turbo-class streaming FEC.

use crate::error::ModemError;

const STATES: usize = 4;
const G0: u32 = 0b111;
const G1: u32 = 0b101;
// Number of tail bits to flush trellis to state 0.
const TAIL_BITS: usize = 2;

/// Rate-1/2 convolutional encoder/decoder — stateless.
///
/// Wire format mirrors `FecCodec`: 4-byte big-endian original-length prefix
/// followed by the encoded bit stream packed MSB-first into bytes.
pub struct ConvCodec;

impl ConvCodec {
    pub fn new() -> Self {
        Self
    }

    /// Encode `data` at rate 1/2.  Output is ≈ 2× the input size.
    pub fn encode(&self, data: &[u8]) -> Vec<u8> {
        let orig_len = data.len() as u32;
        // Prepend 4-byte length prefix (matches RS framing convention).
        let mut src = Vec::with_capacity(4 + data.len());
        src.extend_from_slice(&orig_len.to_be_bytes());
        src.extend_from_slice(data);

        let mut out_bits: Vec<u8> = Vec::with_capacity((src.len() * 8 + TAIL_BITS) * 2);
        let mut state: u32 = 0; // 2-bit shift register

        let push_symbol = |state: u32, bit: u32, out: &mut Vec<u8>| {
            let reg = (bit << 2) | state; // current bit + history
            out.push(parity(reg & G0) as u8);
            out.push(parity(reg & G1) as u8);
        };

        for &byte in &src {
            for shift in (0..8).rev() {
                let bit = (byte as u32 >> shift) & 1;
                push_symbol(state, bit, &mut out_bits);
                state = ((state >> 1) | (bit << 1)) & 0b11;
            }
        }
        // Flush trellis with tail zero bits.
        for _ in 0..TAIL_BITS {
            push_symbol(state, 0, &mut out_bits);
            state = (state >> 1) & 0b11;
        }

        pack_bits(&out_bits)
    }

    /// Decode a rate-1/2 encoded block with hard-decision Viterbi.
    pub fn decode(&self, data: &[u8]) -> Result<Vec<u8>, ModemError> {
        if data.len() < 4 {
            return Err(ModemError::Fec("too short".into()));
        }
        // The first 4 bytes of the *decoded* bit-stream are the length prefix.
        // We need to unpack all received bits and run Viterbi, then read the prefix.
        let received_bits = unpack_bits(data);
        let n_symbols = received_bits.len() / 2;

        // Forward pass: path metrics + decisions.
        let inf = u32::MAX / 2;
        let mut metrics = [inf; STATES];
        metrics[0] = 0;
        // decisions[t][state] = (prev_state, input_bit)
        let mut decisions: Vec<[(u32, u32); STATES]> = Vec::with_capacity(n_symbols);

        for t in 0..n_symbols {
            let r0 = received_bits[2 * t] as u32;
            let r1 = received_bits[2 * t + 1] as u32;
            let mut new_metrics = [inf; STATES];
            let mut dec = [(0u32, 0u32); STATES];

            for next_state in 0..STATES {
                for bit in 0u32..2 {
                    for prev_state in 0u32..STATES as u32 {
                        let ns = ((prev_state >> 1) | (bit << 1)) as usize & 0b11;
                        if ns != next_state {
                            continue;
                        }
                        let reg = (bit << 2) | prev_state;
                        let c0 = parity(reg & G0);
                        let c1 = parity(reg & G1);
                        let branch = hamming(c0, r0) + hamming(c1, r1);
                        let candidate = metrics[prev_state as usize].saturating_add(branch);
                        if candidate < new_metrics[next_state] {
                            new_metrics[next_state] = candidate;
                            dec[next_state] = (prev_state, bit);
                        }
                    }
                }
            }
            metrics = new_metrics;
            decisions.push(dec);
        }

        // Traceback from best terminal state.
        let mut best_state = (0..STATES).min_by_key(|&s| metrics[s]).unwrap() as u32;
        let mut decoded_bits: Vec<u8> = vec![0; n_symbols];
        for t in (0..n_symbols).rev() {
            let (prev, bit) = decisions[t][best_state as usize];
            decoded_bits[t] = bit as u8;
            best_state = prev;
        }

        // Skip tail bits at the end.
        let data_bits = &decoded_bits[..decoded_bits.len().saturating_sub(TAIL_BITS)];

        // First 32 bits are the length prefix.
        if data_bits.len() < 32 {
            return Err(ModemError::Fec(
                "decoded stream too short for prefix".into(),
            ));
        }
        let orig_len = bits_to_u32(&data_bits[..32]) as usize;
        let total_data_bits = 32 + orig_len * 8;
        if data_bits.len() < total_data_bits {
            return Err(ModemError::Fec(format!(
                "decoded stream has {} bits, need {}",
                data_bits.len(),
                total_data_bits
            )));
        }
        let payload_bits = &data_bits[32..total_data_bits];
        Ok(pack_bits(payload_bits))
    }
}

impl Default for ConvCodec {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parity(x: u32) -> u32 {
    (x.count_ones()) & 1
}

fn hamming(a: u32, b: u32) -> u32 {
    (a ^ b).count_ones()
}

fn pack_bits(bits: &[u8]) -> Vec<u8> {
    let n_bytes = bits.len().div_ceil(8);
    let mut out = vec![0u8; n_bytes];
    for (i, &b) in bits.iter().enumerate() {
        out[i / 8] |= b << (7 - (i % 8));
    }
    out
}

fn unpack_bits(data: &[u8]) -> Vec<u8> {
    let mut bits = Vec::with_capacity(data.len() * 8);
    for &byte in data {
        for shift in (0..8).rev() {
            bits.push((byte >> shift) & 1);
        }
    }
    bits
}

fn bits_to_u32(bits: &[u8]) -> u32 {
    debug_assert!(bits.len() == 32);
    bits.iter().fold(0u32, |acc, &b| (acc << 1) | b as u32)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn flip_bit(data: &mut Vec<u8>, bit_idx: usize) {
        data[bit_idx / 8] ^= 1 << (7 - (bit_idx % 8));
    }

    fn flip_bits_range(data: &mut Vec<u8>, start: usize, count: usize) {
        for i in start..start + count {
            if i / 8 < data.len() {
                flip_bit(data, i);
            }
        }
    }

    #[test]
    fn round_trip_empty() {
        let c = ConvCodec::new();
        let enc = c.encode(&[]);
        let dec = c.decode(&enc).unwrap();
        assert_eq!(dec, &[] as &[u8]);
    }

    #[test]
    fn round_trip_small() {
        let c = ConvCodec::new();
        for payload in [&[0xABu8] as &[u8], &[0x01, 0x02, 0x03]] {
            let dec = c.decode(&c.encode(payload)).unwrap();
            assert_eq!(&dec, payload);
        }
    }

    #[test]
    fn round_trip_100_bytes() {
        let c = ConvCodec::new();
        let payload: Vec<u8> = (0..100).map(|i| (i * 17 + 3) as u8).collect();
        let dec = c.decode(&c.encode(&payload)).unwrap();
        assert_eq!(dec, payload);
    }

    #[test]
    fn corrects_single_bit_error() {
        let c = ConvCodec::new();
        let payload = b"test payload";
        let mut enc = c.encode(payload);
        flip_bit(&mut enc, 10);
        let dec = c.decode(&enc).unwrap();
        assert_eq!(dec, payload);
    }

    #[test]
    fn corrects_2_isolated_errors() {
        // K=3 (df=5) can correct up to 2 isolated symbol errors.
        // Use widely spaced positions to avoid burst-overlap effects.
        let c = ConvCodec::new();
        let payload: Vec<u8> = (0..50u8).collect();
        let mut enc = c.encode(&payload);
        flip_bit(&mut enc, 100);
        flip_bit(&mut enc, 300);
        let dec = c.decode(&enc).unwrap();
        assert_eq!(dec, payload);
    }

    #[test]
    fn rate_is_approximately_half() {
        let c = ConvCodec::new();
        let payload: Vec<u8> = vec![0u8; 100];
        let enc = c.encode(&payload);
        // encoded = 2 × (4 + 100) bytes + small tail overhead
        let expected_min = 2 * (4 + 100);
        let expected_max = 2 * (4 + 100) + 4;
        assert!(
            enc.len() >= expected_min && enc.len() <= expected_max,
            "encoded len {} not in [{}, {}]",
            enc.len(),
            expected_min,
            expected_max
        );
    }
}
