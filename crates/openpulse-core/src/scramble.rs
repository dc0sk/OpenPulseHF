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
            // x^9 + x^5 + 1, in the O.150 recurrence s[n+9] = s[n+5] ^ s[n]: with the register
            // shifting right and bit 0 leaving as output, the bit entering bit 8 is `s0 ^ s5`.
            //
            // It read `(state >> 8) ^ (state >> 4)` until 2026-08-21 (#1148), which is
            // s[n+9] = s[n+8] ^ s[n+4] — characteristic x^9+x^8+x^4 = x^4(x^5+x^4+1), and
            // x^5+x^4+1 = (x^2+x+1)(x^3+x+1) is REDUCIBLE, so the period was lcm(3,7) = **21 bits**,
            // not 511, with bits 0..3 a dead delay line that never reached the feedback. Every
            // shipped test passed on it because none of them measured the period.
            let fb = (state ^ (state >> 5)) & 1;
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
    use crate::fec::FecCodec;
    use crate::frame::Frame;

    /// The wire's own bit order: **LSB-first** within each byte.
    ///
    /// This is not a stylistic choice — it is what `bytes_to_bits` does in every modulator, and what
    /// [`descramble_llrs`] documents for the soft path. Measuring transition density in the other
    /// order silently reorders every run that crosses a byte boundary, so a stream that is a dead
    /// carrier on the air can read as healthy here. That is precisely how the previous version of
    /// the gate below was blind.
    fn wire_bits(bytes: &[u8]) -> Vec<u8> {
        bytes
            .iter()
            .flat_map(|b| (0..8).map(move |i| (b >> i) & 1))
            .collect()
    }

    /// The longest run of **zero** bits — the differential-BPSK dead-carrier condition.
    ///
    /// `nrzi_encode` flips the carrier phase on a `1` and holds it on a `0`, so only a run of zeros
    /// starves timing recovery. A run of ones is the opposite: a phase reversal every symbol, the
    /// densest transition pattern there is. Counting runs of *identical* bits therefore measures
    /// the wrong quantity in both directions — it flags the healthy case and its bound on the
    /// harmful one is incidental.
    fn longest_zero_run(bits: &[u8]) -> usize {
        let mut longest = 0usize;
        let mut run = 0usize;
        for &b in bits {
            run = if b == 0 { run + 1 } else { 0 };
            longest = longest.max(run);
        }
        longest
    }

    /// Pack a bit sequence into bytes in the wire's LSB-first order. Inverse of [`wire_bits`].
    fn pack_wire_bits(bits: &[u8]) -> Vec<u8> {
        bits.chunks(8)
            .map(|c| {
                c.iter()
                    .enumerate()
                    .fold(0u8, |acc, (i, &b)| acc | ((b & 1) << i))
            })
            .collect()
    }

    fn ones_ratio(bytes: &[u8]) -> f32 {
        let ones: u32 = bytes.iter().map(|b| b.count_ones()).sum();
        ones as f32 / (bytes.len() * 8) as f32
    }

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

    /// The longest dead carrier tolerated on the whitened wire, in bits.
    ///
    /// The keystream is a maximal-length 9-bit LFSR, whose longest run of zeros is `n - 1 = 8` per
    /// period by construction, so the real margin is 8. Set at 12 to leave headroom for a seed or
    /// tap change without flagging, while staying far below the 17+ runs a genuinely weakened
    /// keystream produces.
    const MAX_DEAD_BITS: usize = 12;

    /// THE PERIOD GATE: the keystream must be the sequence this format froze, not merely a
    /// well-behaved one (#1148).
    ///
    /// The shipped taps ran at a **21-bit** period for the module's whole life while the docs
    /// claimed 511, and all six tests here passed on it — because none of them measured period.
    /// Non-constancy and ones-balance, which they did measure, are satisfied by a period-21
    /// sequence.
    ///
    /// Four assertions, and the third is the one that is easy to leave out:
    ///
    /// 1. minimal **bit** period is exactly 511 — and "no period found" must fail, which is what
    ///    the old taps produce from index 0 (their state map is 2-to-1, so there is a 4-bit
    ///    transient before the 21-bit cycle);
    /// 2. minimal **byte** period is exactly 511, pinning the gcd(8, 511) = 1 argument together
    ///    with the LSB-first packing;
    /// 3. a **known-answer vector** — because the reciprocal trinomial x^9+x^4+1 is *also*
    ///    primitive with period 511 and would pass 1 and 2 while putting different bits on the air
    ///    (`FF C1 FB E8 ...` against our `FF E1 1D 9A ...`). This is a format freeze: pin the
    ///    sequence, not a property of it;
    /// 4. longest zero run over a wrapped period is 8 = n-1, the maximal-LFSR property that is the
    ///    module's whole reason for existing.
    ///
    /// Regenerating the vector (independent of this implementation — from the ITU-T O.150 PRBS9
    /// recurrence, with only the LSB-first packing taken from our wire format):
    ///
    /// ```text
    /// s = [1]*9
    /// for n in range(4096): s.append(s[n+5] ^ s[n])
    /// bytes = [sum(s[8*k+i] << i for i in range(8)) for k in range(16)]
    /// ```
    #[test]
    fn the_keystream_is_the_frozen_prbs9_sequence() {
        // Through the public seam: XOR against zeros yields the keystream itself, so this covers
        // `scramble()` and the packing, not just the private generator.
        let ks = scrambled(&[0u8; 1022]);

        let bits: Vec<u8> = ks
            .iter()
            .flat_map(|b| (0..8).map(move |i| (b >> i) & 1))
            .collect();
        let bit_period = (1..=511usize)
            .find(|p| bits.iter().zip(bits.iter().skip(*p)).all(|(a, b)| a == b))
            .expect(
                "the keystream has NO period within 511 bits — the shipped x^9+x^8+x^4 taps produce \
                 exactly this from index 0, because their 2-to-1 state map leaves a transient",
            );
        assert_eq!(
            bit_period, 511,
            "minimal bit period is {bit_period}, not 511: this is not the primitive x^9+x^5+1 \
             sequence the wire format froze"
        );

        let byte_period = (1..=511usize)
            .find(|p| ks.iter().zip(ks.iter().skip(*p)).all(|(a, b)| a == b))
            .expect("the keystream has no byte-domain period within 511 bytes");
        assert_eq!(
            byte_period, 511,
            "minimal byte period is {byte_period}, not 511 — gcd(8, 511) = 1 is what makes the \
             byte sequence repeat on the same period as the bits"
        );

        const KAT: [u8; 16] = [
            0xFF, 0xE1, 0x1D, 0x9A, 0xED, 0x85, 0x33, 0x24, 0xEA, 0x7A, 0xD2, 0x39, 0x70, 0x97,
            0x57, 0x0A,
        ];
        assert_eq!(
            &ks[..16],
            &KAT,
            "keystream prefix does not match the frozen PRBS9 vector. Period alone cannot catch \
             this: the reciprocal trinomial x^9+x^4+1 is also primitive with period 511 and starts \
             FF C1 FB E8. A different seed also lands here while the period stays 511."
        );

        let longest_zero_run = {
            let wrapped: Vec<u8> = bits.iter().copied().chain(bits.iter().copied()).collect();
            let mut best = 0usize;
            let mut run = 0usize;
            for b in wrapped.iter().take(511 + 16) {
                run = if *b == 0 { run + 1 } else { 0 };
                best = best.max(run);
            }
            best
        };
        assert_eq!(
            longest_zero_run, 8,
            "longest zero run is {longest_zero_run}, not the n-1 = 8 of a maximal 9-bit LFSR — a \
             longer run is dead carrier, which is the defect this module exists to prevent"
        );
    }

    /// THE GATE: the real on-air wire for the frame that failed in #1021 must carry no long run of
    /// zero bits.
    ///
    /// **This test was rewritten on 2026-07-30 (archetype scan 2026-07-29, finding 17).** The
    /// previous version stood behind the whole #1021 fix while measuring three things about a
    /// regime the wire does not have: it unpacked MSB-first where the wire is LSB-first, it counted
    /// runs of *identical* bits where only *zero* runs are a dead carrier, and it whitened a bare
    /// `[0u8; 195]` at keystream offset 0 where the real padding begins at offset 28. Verified
    /// blind: a stream with repeated 17-bit zero runs — a real dead carrier — passed both of its
    /// assertions. The property did happen to hold in the real regime, so this was a blind spot
    /// rather than a live regression, but the defect it exists to prevent could have recurred
    /// unseen.
    ///
    /// It now measures the actual bytes that go on the air, which removes the offset question by
    /// construction rather than approximating it.
    #[test]
    fn the_real_padded_wire_carries_no_dead_carrier_run() {
        // The #1021 frame: a 14-byte payload, framed, through the standard RS codec. `FecCodec`
        // prefixes 4 length bytes and pads with ZEROS to fill the 223-byte data block, so the
        // padding starts at offset 4 + 10 (frame header) + 14 = 28 — the real keystream offset.
        let frame = Frame::new(1, vec![0xA5u8; 14]).expect("frame");
        let coded = FecCodec::new().encode(&frame.encode());

        // CONTROL: unwhitened, this wire IS the defect — one uninterrupted run of zero bits where
        // the padding sits. If this ever stops holding, the fixture no longer reproduces #1021 and
        // the assertion below proves nothing.
        let raw_run = longest_zero_run(&wire_bits(&coded));
        assert!(
            raw_run > 1000,
            "the fixture no longer reproduces #1021: the unwhitened wire's longest zero run is \
             only {raw_run} bits. Whitening cannot be shown to fix a defect the fixture does not \
             contain."
        );

        let whitened = scrambled(&coded);
        let run = longest_zero_run(&wire_bits(&whitened));
        assert!(
            run <= MAX_DEAD_BITS,
            "the whitened wire still contains a {run}-bit run of zeros — in differential BPSK that \
             is {run} symbols of unmodulated carrier, which is the #1021 failure (measured 6.2 s \
             on air). Unwhitened the same wire runs {raw_run} bits."
        );

        let ratio = ones_ratio(&whitened);
        assert!(
            (0.4..=0.6).contains(&ratio),
            "whitened wire is {ratio:.3} ones — it must be near 0.5"
        );
    }

    /// Anti-vacuity: the measurement must be ABLE to fail, and specifically on the case that
    /// slipped past the old one.
    ///
    /// A stream can be perfectly balanced in ones and zeros and still be a dead carrier — balance
    /// is an average, and a dead carrier is a run. This is the exact pattern that passed the
    /// previous gate: 17 zeros then 17 ones, repeating. It is 50 % ones, and in MSB-first order its
    /// longest identical run is 16, one below the old bound. Read the way the wire actually reads
    /// it, it is 17 symbols of unmodulated carrier every 34 symbols.
    #[test]
    fn the_dead_carrier_measurement_catches_a_balanced_but_dead_stream() {
        let bits: Vec<u8> = (0..1224)
            .map(|i| u8::from((i / 17) % 2 == 1))
            .collect::<Vec<_>>();
        let bytes = pack_wire_bits(&bits);

        let ratio = ones_ratio(&bytes);
        assert!(
            (0.4..=0.6).contains(&ratio),
            "fixture is not balanced ({ratio:.3}) — it would be caught by the balance check \
             instead, which is not what this test is demonstrating"
        );

        let run = longest_zero_run(&wire_bits(&bytes));
        assert_eq!(
            run, 17,
            "the dead-carrier measurement failed to see a 17-bit zero run"
        );
        assert!(
            run > MAX_DEAD_BITS,
            "a balanced stream with 17-bit zero runs must exceed the tolerance, or the gate above \
             cannot fail for the reason it exists"
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
