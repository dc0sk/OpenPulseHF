//! Wire-stream whitening, so the transmitted signal always has symbol transitions.
//!
//! **The defect this exists to prevent (measured on air, issue #1021).** `FecCodec::encode` pads a
//! short payload with **zeros** to fill a 223-byte Reed–Solomon data block. In differentially
//! encoded BPSK a run of zero bits is *no phase change* — an unmodulated carrier. A 14-byte payload
//! therefore transmitted 0.90 s of data, **6.24 s of dead carrier**, then 1.02 s of parity
//! (predicted 6.24 s; measured 6.2 s off the air, with the spectral spread ratio reading exactly
//! 0.000 through the middle). Gardner timing recovery and the decision-directed carrier loop have
//! nothing to track across that, so lock drifts and the parity after it decodes as garbage: RS then
//! sees more than its 16-byte correction capacity and the block fails. Uncoded frames of the same
//! payload, over the same link minutes apart, decoded fine.
//!
//! Padding is only the most visible case. **Real payload data with long runs of identical bytes
//! fails the same way** — a run of 0x00 or 0xFF, a zero-filled file block, a long silence in a
//! Winlink attachment. Whitening the wire is the standard fix precisely because it is content-
//! independent: it guarantees transition density whatever the payload happens to contain.
//!
//! **Additive, not multiplicative.** This XORs a pseudo-random sequence restarted at every frame,
//! rather than feeding the data through a self-synchronising LFSR. A multiplicative scrambler needs
//! no frame alignment but **multiplies errors** — one channel bit error becomes several after
//! descrambling — which is exactly the wrong property in front of an error-correcting code. The
//! additive form needs frame alignment (which the preamble already establishes) and is transparent
//! to the FEC: the error pattern reaching the decoder is unchanged.
//!
//! The sequence is a 9-bit LFSR, `x^9 + x^5 + 1`, seeded to all-ones. Period 511 bytes; it is a
//! maximal-length polynomial, so every 9-bit state except zero occurs once per period and the byte
//! stream is well balanced. Scrambling is self-inverse: apply the same function to undo it.

/// LFSR seed. Non-zero by necessity — an all-zero state is the degenerate fixed point that would
/// emit zeros forever and reintroduce the very defect this module prevents.
const SEED: u16 = 0x1FF;

/// Generate `n` bytes of the whitening sequence.
fn keystream(n: usize) -> Vec<u8> {
    let mut state = SEED;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let mut byte = 0u8;
        for bit in 0..8 {
            // x^9 + x^5 + 1: taps at bit 8 (x^9) and bit 4 (x^5) of a 9-bit register.
            let fb = ((state >> 8) ^ (state >> 4)) & 1;
            byte |= ((state & 1) as u8) << bit;
            state = ((state >> 1) | (fb << 8)) & 0x1FF;
        }
        out.push(byte);
    }
    out
}

/// Whiten (or un-whiten) a wire stream in place. Self-inverse.
pub fn scramble(data: &mut [u8]) {
    let ks = keystream(data.len());
    for (b, k) in data.iter_mut().zip(ks) {
        *b ^= k;
    }
}

/// Whiten a copy of `data`.
pub fn scrambled(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    scramble(&mut out);
    out
}

/// Un-whiten soft decisions.
///
/// A soft demodulator yields a log-likelihood ratio per **bit**, not bytes, so descrambling cannot
/// be an XOR. XOR with a 1 flips the bit, and flipping a bit negates its LLR — the magnitude
/// (confidence) is untouched, only the sign moves.
///
/// **Bit order is LSB-first within each byte**, matching how the engine packs soft decisions back
/// into wire bytes (`acc | (bit << i)` for i = 0..8). Getting this backwards flips the wrong bits
/// and yields `invalid magic` on every frame — and a unit test written to the same wrong convention
/// will still pass, because it is self-consistent. The test below therefore packs bytes exactly the
/// way the engine does rather than asserting against an assumed order.
pub fn descramble_llrs(llrs: &mut [f32]) {
    let ks = keystream(llrs.len().div_ceil(8));
    for (i, l) in llrs.iter_mut().enumerate() {
        // Bit i of the stream: byte i/8, LSB-first within the byte (the engine's packing order).
        if (ks[i / 8] >> (i % 8)) & 1 == 1 {
            *l = -*l;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Self-inverse: scrambling twice returns the original. Without this the receive path would
    /// need a separate descrambler that could drift out of step with the transmitter.
    #[test]
    fn scrambling_twice_is_the_identity() {
        for case in [
            vec![0u8; 300],
            vec![0xFFu8; 300],
            (0..=255u8).collect::<Vec<_>>(),
            b"OpenPulseHF".to_vec(),
            vec![],
        ] {
            let mut x = case.clone();
            scramble(&mut x);
            scramble(&mut x);
            assert_eq!(x, case, "scramble must be self-inverse");
        }
    }

    /// THE POINT: an all-zero block — exactly what RS padding produces — must come out with a
    /// balanced mix of ones and zeros, because that is what puts transitions on the air.
    #[test]
    fn an_all_zero_block_becomes_transition_rich() {
        // 195 zero bytes is the real padding measured on air for a 14-byte payload.
        let out = scrambled(&[0u8; 195]);
        let ones: u32 = out.iter().map(|b| b.count_ones()).sum();
        let total = (out.len() * 8) as u32;
        let ratio = ones as f32 / total as f32;
        assert!(
            (0.4..=0.6).contains(&ratio),
            "whitened zero padding is {ratio:.3} ones — it must be near 0.5 or the carrier is \
             still effectively unmodulated"
        );
        // No long run of identical BITS, which is what actually starves timing recovery.
        let bits: Vec<u8> = out
            .iter()
            .flat_map(|b| (0..8).rev().map(move |i| (b >> i) & 1))
            .collect();
        let mut longest = 1usize;
        let mut run = 1usize;
        for w in bits.windows(2) {
            run = if w[0] == w[1] { run + 1 } else { 1 };
            longest = longest.max(run);
        }
        assert!(
            longest <= 16,
            "whitened zero padding still contains a {longest}-bit run with no transition"
        );
    }

    /// An all-ones block is the other degenerate input and must whiten just as well.
    #[test]
    fn an_all_ones_block_becomes_transition_rich() {
        let out = scrambled(&[0xFFu8; 195]);
        let ones: u32 = out.iter().map(|b| b.count_ones()).sum();
        let ratio = ones as f32 / (out.len() * 8) as f32;
        assert!(
            (0.4..=0.6).contains(&ratio),
            "all-ones whitened to {ratio:.3} ones"
        );
    }

    /// The sequence must not repeat within a frame-sized block in a way that recreates long runs.
    /// (Period is 511 bytes; a 255-byte RS block fits inside one period.)
    #[test]
    fn the_keystream_is_not_degenerate() {
        let ks = keystream(511);
        assert!(
            ks.iter().any(|&b| b != ks[0]),
            "keystream is constant — the LFSR is not running"
        );
        let ones: u32 = ks.iter().map(|b| b.count_ones()).sum();
        let ratio = ones as f32 / (ks.len() * 8) as f32;
        assert!(
            (0.45..=0.55).contains(&ratio),
            "keystream is unbalanced ({ratio:.3} ones)"
        );
    }

    /// Descrambling soft decisions must flip exactly the bits the byte scrambler flips, and must
    /// leave confidence untouched — an LLR carries magnitude as well as sign.
    #[test]
    fn soft_descrambling_matches_the_byte_scrambler() {
        let data = vec![0u8; 32];
        let wire = scrambled(&data);
        // Ideal soft demod of `wire`: +1 for a 0 bit, -1 for a 1 bit (MSB first).
        // Pack exactly as the engine unpacks: bit i of each byte is LLR index i (LSB-first).
        let mut llrs: Vec<f32> = wire
            .iter()
            .flat_map(|b| (0..8).map(move |i| if (b >> i) & 1 == 1 { -1.0 } else { 1.0 }))
            .collect();
        let magnitudes: Vec<f32> = llrs.iter().map(|l| l.abs()).collect();
        descramble_llrs(&mut llrs);
        // After descrambling every bit must read 0 again (the original data was all zeros).
        assert!(
            llrs.iter().all(|&l| l > 0.0),
            "soft descrambling did not recover the original all-zero data"
        );
        let after: Vec<f32> = llrs.iter().map(|l| l.abs()).collect();
        assert_eq!(
            magnitudes, after,
            "descrambling must not change LLR confidence"
        );
    }
}
