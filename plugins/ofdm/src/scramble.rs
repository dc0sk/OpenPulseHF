//! Additive data whitening for the OFDM bit stream.
//!
//! A low-entropy or RS-padded payload maps every data subcarrier to the same constellation point, whose
//! IDFT is a time-domain impulse train — a very high PAPR symbol. The engine's CE-SSB peak-stretch
//! conditioner then crushes it and the frame fails to decode **even on a perfect channel**. Whitening the
//! bit stream with a deterministic keystream (the standard OFDM practice, e.g. DVB-T / 802.11) decorrelates
//! the subcarriers so no payload can produce that impulse train, and it lowers PAPR generally.
//!
//! The keystream is a fixed, position-indexed pseudo-random bit sequence — identical on both ends, so no
//! negotiation is needed. `scramble_bits` is XOR (self-inverse: the same call descrambles on receive);
//! `descramble_llrs` maps a keystream 1 to an LLR sign flip.

/// Deterministic keystream generator (top bit of a fixed-seed LCG — well-distributed for whitening).
struct Whitener {
    s: u64,
}

impl Whitener {
    fn new() -> Self {
        // Fixed nonzero seed (digits of π); shared by transmitter and receiver.
        Self {
            s: 0x243F_6A88_85A3_08D3,
        }
    }

    fn next_bit(&mut self) -> bool {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 63) & 1 == 1
    }
}

/// XOR the bit stream in place with the whitening keystream. Self-inverse: apply once on transmit to
/// scramble, once on receive to descramble.
pub fn scramble_bits(bits: &mut [bool]) {
    let mut w = Whitener::new();
    for b in bits.iter_mut() {
        *b ^= w.next_bit();
    }
}

/// Descramble soft LLRs: a keystream bit of 1 inverts that bit, i.e. flips the sign of its LLR. The bit
/// ordering matches `scramble_bits` (one keystream bit per LLR, in transmit bit order).
pub fn descramble_llrs(llrs: &mut [f32]) {
    let mut w = Whitener::new();
    for l in llrs.iter_mut() {
        if w.next_bit() {
            *l = -*l;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scramble_is_self_inverse_and_whitens_zeros() {
        let mut bits = vec![false; 512]; // all-zero → the worst-case impulse-train input
        scramble_bits(&mut bits);
        // Whitened: the keystream is roughly balanced, not a constant run.
        let ones = bits.iter().filter(|&&b| b).count();
        assert!(
            (200..=312).contains(&ones),
            "whitened all-zeros should be ~balanced, got {ones}/512 ones"
        );
        // Self-inverse: a second application restores the original.
        scramble_bits(&mut bits);
        assert!(bits.iter().all(|&b| !b), "scramble must be self-inverse");
    }

    #[test]
    fn descramble_llrs_recovers_the_original_bits() {
        // Model the pipeline: original bits → scramble → on-air bits → LLR(on-air) → descramble_llrs →
        // recovered LLR whose sign must equal the original bit (negative = 1, positive = 0).
        let n = 256;
        let mut seed = 7u64;
        let orig: Vec<bool> = (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                (seed >> 40) & 1 == 1
            })
            .collect();
        let mut onair = orig.clone();
        scramble_bits(&mut onair);
        // LLR for the on-air bit: positive ⇒ bit 0, negative ⇒ bit 1.
        let mut llrs: Vec<f32> = onair.iter().map(|&b| if b { -2.5 } else { 2.5 }).collect();
        descramble_llrs(&mut llrs);
        for k in 0..n {
            assert_eq!(orig[k], llrs[k] < 0.0, "bit {k} descramble_llrs mismatch");
        }
    }
}
