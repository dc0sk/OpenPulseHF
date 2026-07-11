//! JS8 Costas sync arrays (plan §2.1; JS8Call-improved `JS8_Mode/JS8.h:24–36`).
//!
//! Every JS8 transmission is 79 symbols with three 7-symbol Costas sync blocks at symbol positions
//! 0–6, 36–42, 72–78; the remaining 58 positions carry data tones. NORMAL uses the FT8-legacy
//! `ORIGINAL` array in all three blocks; every other submode uses three per-block-unique `MODIFIED`
//! arrays, whose distinctness prevents false sync at ±36-symbol offsets.

use crate::submode::{COSTAS_BLOCK_STARTS, COSTAS_LEN, NUM_COSTAS_BLOCKS, NUM_SYMBOLS};

/// FT8-legacy Costas array, used by NORMAL for all three sync blocks.
pub const ORIGINAL: [u8; COSTAS_LEN] = [4, 2, 5, 6, 1, 3, 0];

/// Per-block Costas arrays used by every non-NORMAL submode (block 0/1/2).
pub const MODIFIED: [[u8; COSTAS_LEN]; NUM_COSTAS_BLOCKS] = [
    [0, 6, 2, 3, 5, 4, 1],
    [1, 5, 0, 2, 3, 6, 4],
    [2, 5, 0, 6, 4, 1, 3],
];

/// Which Costas layout a submode uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostasKind {
    /// FT8-legacy single array in all three blocks (NORMAL).
    Original,
    /// Three per-block-unique arrays (SLOW/FAST/TURBO/ULTRA).
    Modified,
}

impl CostasKind {
    /// The 7 sync tones for Costas block `block` (0..[`NUM_COSTAS_BLOCKS`]).
    pub fn block(&self, block: usize) -> [u8; COSTAS_LEN] {
        match self {
            CostasKind::Original => ORIGINAL,
            CostasKind::Modified => MODIFIED[block % NUM_COSTAS_BLOCKS],
        }
    }

    /// The full 79-symbol sync map: `Some(tone)` at the three Costas blocks, `None` at data positions.
    pub fn sync_map(&self) -> [Option<u8>; NUM_SYMBOLS] {
        let mut map = [None; NUM_SYMBOLS];
        for (b, &start) in COSTAS_BLOCK_STARTS.iter().enumerate() {
            let tones = self.block(b);
            for (i, &tone) in tones.iter().enumerate() {
                map[start + i] = Some(tone);
            }
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::submode::{NUM_DATA_SYMBOLS, NUM_SYNC_SYMBOLS, NUM_TONES};

    /// A length-7 Costas array over 8 tones uses 7 distinct tones, each `< 8` (one tone unused).
    fn is_distinct_valid_tones(a: &[u8]) -> bool {
        let mut seen = [false; NUM_TONES];
        for &t in a {
            if t as usize >= NUM_TONES || seen[t as usize] {
                return false;
            }
            seen[t as usize] = true;
        }
        true
    }

    #[test]
    fn arrays_use_distinct_valid_tones() {
        assert_eq!(ORIGINAL.len(), COSTAS_LEN);
        assert!(
            is_distinct_valid_tones(&ORIGINAL),
            "ORIGINAL tones must be distinct and < 8"
        );
        for (b, arr) in MODIFIED.iter().enumerate() {
            assert!(
                is_distinct_valid_tones(arr),
                "MODIFIED block {b} tones must be distinct and < 8"
            );
        }
    }

    #[test]
    fn modified_blocks_are_pairwise_distinct() {
        // The anti-false-sync property: no two MODIFIED blocks are identical.
        assert_ne!(MODIFIED[0], MODIFIED[1]);
        assert_ne!(MODIFIED[0], MODIFIED[2]);
        assert_ne!(MODIFIED[1], MODIFIED[2]);
    }

    #[test]
    fn block_dispatch() {
        for (b, expected) in MODIFIED.iter().enumerate() {
            assert_eq!(CostasKind::Original.block(b), ORIGINAL);
            assert_eq!(CostasKind::Modified.block(b), *expected);
        }
    }

    #[test]
    fn sync_map_places_sync_tones_and_leaves_data_open() {
        for kind in [CostasKind::Original, CostasKind::Modified] {
            let map = kind.sync_map();
            let sync = map.iter().filter(|s| s.is_some()).count();
            let data = map.iter().filter(|s| s.is_none()).count();
            assert_eq!(sync, NUM_SYNC_SYMBOLS, "21 sync tones");
            assert_eq!(data, NUM_DATA_SYMBOLS, "58 data positions");
            // The blocks sit exactly at 0–6, 36–42, 72–78.
            for &start in &COSTAS_BLOCK_STARTS {
                for i in 0..COSTAS_LEN {
                    assert!(map[start + i].is_some(), "sync tone at {}", start + i);
                }
            }
            assert!(map[7].is_none(), "position 7 is data");
            assert!(map[35].is_none(), "position 35 is data");
        }
    }
}
