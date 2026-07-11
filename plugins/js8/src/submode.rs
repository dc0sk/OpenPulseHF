//! JS8 submode table (plan §2.2; JS8Call-improved `commons.h` + `JS8Submode.cpp`).
//!
//! JS8Call runs at 12 000 Hz audio; OpenPulse runs at 8 000 Hz. Tone spacing and baud are
//! sample-rate-independent protocol facts, and every submode's samples-per-symbol is an **exact
//! integer** at 8 kHz, so no resampling is needed. MVP is NORMAL only (the interop baseline).

use crate::costas::CostasKind;

/// Audio sample rate these parameters are computed for.
pub const SAMPLE_RATE: u32 = 8000;
/// Symbols per JS8 transmission (all submodes): 3×7 Costas sync + 58 data.
pub const NUM_SYMBOLS: usize = 79;
/// Data-carrying symbols per transmission (58 × 3 bits = 174-bit LDPC codeword).
pub const NUM_DATA_SYMBOLS: usize = 58;
/// Sync symbols per transmission (3 Costas blocks × 7).
pub const NUM_SYNC_SYMBOLS: usize = 21;
/// 8-FSK: one of eight tones per symbol carries 3 bits.
pub const NUM_TONES: usize = 8;
/// Costas array length.
pub const COSTAS_LEN: usize = 7;
/// Number of Costas sync blocks.
pub const NUM_COSTAS_BLOCKS: usize = 3;
/// Symbol index at which each Costas sync block starts (0–6, 36–42, 72–78).
pub const COSTAS_BLOCK_STARTS: [usize; NUM_COSTAS_BLOCKS] = [0, 36, 72];

/// The five JS8 submodes (plan §2.2). MVP transmits NORMAL only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submode {
    /// 3.125 baud, 30 s slot; deepest (−28 dB).
    Slow,
    /// 6.25 baud, 15 s slot; the interop baseline (−24 dB).
    Normal,
    /// 10 baud, 10 s slot (−22 dB).
    Fast,
    /// 20 baud ("JS8 40"), 6 s slot (−20 dB).
    Turbo,
    /// 31.25 baud ("JS8 60"), 4 s slot; improved-fork only (−18 dB).
    Ultra,
}

/// Resolved parameters for one submode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SubmodeParams {
    /// The submode.
    pub submode: Submode,
    /// Mode string used at the plugin/registry boundary (e.g. `"JS8-NORMAL"`).
    pub mode: &'static str,
    /// Tone spacing in Hz — equal to the baud (symbol) rate.
    pub tone_spacing_hz: f32,
    /// Occupied bandwidth in Hz (`8 × tone_spacing`).
    pub bandwidth_hz: f32,
    /// Samples per symbol at [`SAMPLE_RATE`] (exact integer).
    pub samples_per_symbol: usize,
    /// Milliseconds into the wall-clock slot at which TX starts.
    pub start_delay_ms: u32,
    /// Wall-clock T/R slot length in seconds.
    pub slot_secs: u32,
    /// Costas layout (ORIGINAL for NORMAL, MODIFIED otherwise).
    pub costas: CostasKind,
}

impl SubmodeParams {
    /// Baud (symbol) rate in Hz — identical to the tone spacing.
    pub fn baud(&self) -> f32 {
        self.tone_spacing_hz
    }

    /// Total samples in one transmission (`79 × samples_per_symbol`).
    pub fn samples_per_period(&self) -> usize {
        NUM_SYMBOLS * self.samples_per_symbol
    }

    /// On-air transmission duration in seconds.
    pub fn tx_duration_secs(&self) -> f32 {
        self.samples_per_period() as f32 / SAMPLE_RATE as f32
    }
}

/// Parameters for `submode` at 8 kHz.
pub fn params(submode: Submode) -> SubmodeParams {
    use CostasKind::{Modified, Original};
    match submode {
        Submode::Slow => SubmodeParams {
            submode,
            mode: "JS8-SLOW",
            tone_spacing_hz: 3.125,
            bandwidth_hz: 25.0,
            samples_per_symbol: 2560,
            start_delay_ms: 500,
            slot_secs: 30,
            costas: Modified,
        },
        Submode::Normal => SubmodeParams {
            submode,
            mode: "JS8-NORMAL",
            tone_spacing_hz: 6.25,
            bandwidth_hz: 50.0,
            samples_per_symbol: 1280,
            start_delay_ms: 500,
            slot_secs: 15,
            costas: Original,
        },
        Submode::Fast => SubmodeParams {
            submode,
            mode: "JS8-FAST",
            tone_spacing_hz: 10.0,
            bandwidth_hz: 80.0,
            samples_per_symbol: 800,
            start_delay_ms: 200,
            slot_secs: 10,
            costas: Modified,
        },
        Submode::Turbo => SubmodeParams {
            submode,
            mode: "JS8-TURBO",
            tone_spacing_hz: 20.0,
            bandwidth_hz: 160.0,
            samples_per_symbol: 400,
            start_delay_ms: 100,
            slot_secs: 6,
            costas: Modified,
        },
        Submode::Ultra => SubmodeParams {
            submode,
            mode: "JS8-ULTRA",
            tone_spacing_hz: 31.25,
            bandwidth_hz: 250.0,
            samples_per_symbol: 256,
            start_delay_ms: 100,
            slot_secs: 4,
            costas: Modified,
        },
    }
}

/// Resolve a mode string (case-insensitive) to its submode parameters.
pub fn params_for_mode(mode: &str) -> Option<SubmodeParams> {
    let m = mode.to_ascii_uppercase();
    [
        Submode::Slow,
        Submode::Normal,
        Submode::Fast,
        Submode::Turbo,
        Submode::Ultra,
    ]
    .into_iter()
    .map(params)
    .find(|p| p.mode == m)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Submode; 5] = [
        Submode::Slow,
        Submode::Normal,
        Submode::Fast,
        Submode::Turbo,
        Submode::Ultra,
    ];

    #[test]
    fn samples_per_symbol_is_an_exact_integer_at_8k() {
        // 8000 / tone_spacing must land on the tabulated integer with no remainder.
        for s in ALL {
            let p = params(s);
            let exact = SAMPLE_RATE as f32 / p.tone_spacing_hz;
            assert_eq!(
                exact.fract(),
                0.0,
                "{} samples/symbol not integral: {exact}",
                p.mode
            );
            assert_eq!(
                exact as usize, p.samples_per_symbol,
                "{} table mismatch",
                p.mode
            );
        }
    }

    #[test]
    fn bandwidth_is_eight_tone_spacings() {
        for s in ALL {
            let p = params(s);
            assert_eq!(p.bandwidth_hz, 8.0 * p.tone_spacing_hz, "{} BW", p.mode);
        }
    }

    #[test]
    fn tx_duration_matches_the_published_table() {
        // 79 × samples/symbol / 8000, rounded to 1/100 s.
        let expect = [
            (Submode::Slow, 25.28),
            (Submode::Normal, 12.64),
            (Submode::Fast, 7.90),
            (Submode::Turbo, 3.95),
            (Submode::Ultra, 2.528),
        ];
        for (s, secs) in expect {
            let got = params(s).tx_duration_secs();
            assert!(
                (got - secs).abs() < 0.005,
                "{:?} duration {got} vs {secs}",
                s
            );
        }
    }

    #[test]
    fn normal_period_is_101120_samples() {
        assert_eq!(params(Submode::Normal).samples_per_period(), 101_120);
    }

    #[test]
    fn only_normal_uses_the_original_costas() {
        for s in ALL {
            let p = params(s);
            let expect = if s == Submode::Normal {
                CostasKind::Original
            } else {
                CostasKind::Modified
            };
            assert_eq!(p.costas, expect, "{} costas", p.mode);
        }
    }

    #[test]
    fn params_for_mode_is_case_insensitive_and_rejects_unknown() {
        assert_eq!(
            params_for_mode("js8-normal").map(|p| p.submode),
            Some(Submode::Normal)
        );
        assert_eq!(
            params_for_mode("JS8-ULTRA").map(|p| p.submode),
            Some(Submode::Ultra)
        );
        assert!(params_for_mode("JS8-HYPER").is_none());
        assert!(params_for_mode("BPSK250").is_none());
    }
}
