//! Tone assembly: 174-bit LDPC codeword → 79-symbol tone sequence (JS8Call `genjs8.f90`).
//!
//! The frame is `S7 D29 S7 D29 S7`: three 7-symbol Costas sync blocks (positions 0–6, 36–42, 72–78)
//! interleaved with two 29-symbol data blocks. Each data symbol's tone is **direct binary** over three
//! codeword bits — `tone = c[i]·4 + c[i+1]·2 + c[i+2]` — *not* Gray-coded (verified against
//! `genjs8.f90`, which writes `itone(k)=indx` with no gray map; the JS8 lib has no gray table at all).

use crate::costas::CostasKind;
use crate::ldpc174::{encode174, K, N};
use crate::submode::{COSTAS_BLOCK_STARTS, COSTAS_LEN, NUM_DATA_SYMBOLS, NUM_SYMBOLS};

/// 0-based symbol positions carrying the 58 data tones: `7..=35` then `43..=71` (the middle Costas
/// block occupies 36–42).
fn data_positions() -> [usize; NUM_DATA_SYMBOLS] {
    let mut pos = [0usize; NUM_DATA_SYMBOLS];
    let mut p = 7;
    for (j, slot) in pos.iter_mut().enumerate() {
        if j == 29 {
            p = 43; // skip the middle Costas block
        }
        *slot = p;
        p += 1;
    }
    pos
}

/// Assemble the 79-symbol tone sequence from a 174-bit `codeword` and the submode's `costas`.
pub fn codeword_to_tones(codeword: &[u8; N], costas: CostasKind) -> [u8; NUM_SYMBOLS] {
    let mut itone = [0u8; NUM_SYMBOLS];
    for (b, &start) in COSTAS_BLOCK_STARTS.iter().enumerate() {
        let blk = costas.block(b);
        itone[start..start + COSTAS_LEN].copy_from_slice(&blk);
    }
    for (j, &pos) in data_positions().iter().enumerate() {
        let i = 3 * j;
        itone[pos] = (codeword[i] & 1) * 4 + (codeword[i + 1] & 1) * 2 + (codeword[i + 2] & 1);
    }
    itone
}

/// Recover the 174-bit codeword from a 79-symbol tone sequence (inverse of [`codeword_to_tones`] on
/// the data positions; sync tones are ignored).
pub fn tones_to_codeword(itone: &[u8; NUM_SYMBOLS]) -> [u8; N] {
    let mut cw = [0u8; N];
    for (j, &pos) in data_positions().iter().enumerate() {
        let t = itone[pos];
        let i = 3 * j;
        cw[i] = (t >> 2) & 1;
        cw[i + 1] = (t >> 1) & 1;
        cw[i + 2] = t & 1;
    }
    cw
}

/// Encode 87 info bits into the full 79-symbol tone sequence: LDPC encode then tone assembly.
pub fn message_to_tones(info: &[u8; K], costas: CostasKind) -> [u8; NUM_SYMBOLS] {
    codeword_to_tones(&encode174(info), costas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ldpc174::parity_syndrome;
    use crate::submode::{NUM_SYNC_SYMBOLS, NUM_TONES};

    fn lcg_codeword(seed: u64) -> [u8; N] {
        // A valid codeword (so round-trips land on parity-satisfying words): encode a random message.
        let mut s = seed;
        let mut msg = [0u8; K];
        for m in msg.iter_mut() {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *m = ((s >> 33) & 1) as u8;
        }
        encode174(&msg)
    }

    #[test]
    fn structure_is_s7_d29_s7_d29_s7() {
        let tones = codeword_to_tones(&[0u8; N], CostasKind::Original);
        // Sync blocks carry the Costas arrays exactly.
        for (b, &start) in COSTAS_BLOCK_STARTS.iter().enumerate() {
            assert_eq!(
                &tones[start..start + COSTAS_LEN],
                &CostasKind::Original.block(b)
            );
        }
        // 21 sync + 58 data positions, all tones < 8.
        assert!(tones.iter().all(|&t| (t as usize) < NUM_TONES));
        let dp = data_positions();
        assert_eq!(dp.len(), NUM_DATA_SYMBOLS);
        assert_eq!(dp[0], 7);
        assert_eq!(dp[28], 35);
        assert_eq!(dp[29], 43);
        assert_eq!(dp[57], 71);
        // No data position lands inside a Costas block.
        for &p in &dp {
            for &s in &COSTAS_BLOCK_STARTS {
                assert!(!(s..s + COSTAS_LEN).contains(&p), "data at sync pos {p}");
            }
        }
        assert_eq!(NUM_SYNC_SYMBOLS, COSTAS_BLOCK_STARTS.len() * COSTAS_LEN);
    }

    #[test]
    fn data_tone_is_direct_binary_of_three_codeword_bits() {
        let cw = lcg_codeword(0xabcd_1234);
        let tones = codeword_to_tones(&cw, CostasKind::Modified);
        for (j, &pos) in data_positions().iter().enumerate() {
            let i = 3 * j;
            let want = cw[i] * 4 + cw[i + 1] * 2 + cw[i + 2];
            assert_eq!(tones[pos], want, "symbol {j}");
        }
    }

    #[test]
    fn tones_round_trip_back_to_the_codeword() {
        for seed in [1u64, 42, 0xdead_beef, 0x1234_5678_9abc] {
            let cw = lcg_codeword(seed);
            let tones = codeword_to_tones(&cw, CostasKind::Original);
            let back = tones_to_codeword(&tones);
            assert_eq!(back, cw, "codeword round-trip (seed {seed})");
        }
    }

    #[test]
    fn message_to_tones_recovers_a_parity_valid_codeword() {
        let mut msg = [0u8; K];
        for (j, m) in msg.iter_mut().enumerate() {
            *m = (j % 5 == 0) as u8;
        }
        let tones = message_to_tones(&msg, CostasKind::Original);
        let cw = tones_to_codeword(&tones);
        assert_eq!(
            parity_syndrome(&cw),
            [0u8; N - K],
            "recovered codeword must satisfy parity"
        );
    }
}
