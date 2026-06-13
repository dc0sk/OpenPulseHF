//! SC-FDMA subcarrier layout and frame geometry constants.
//!
//! Geometry is identical to OFDM (same FFT size, CP, SC spacing, pilot layout)
//! so SC-FDMA occupies exactly the same bandwidth as the corresponding OFDM mode.

/// FFT size used for all SC-FDMA modes.
pub const FFT_SIZE: usize = 256;
/// Cyclic prefix length in samples.
pub const CP: usize = 32;
/// Symbol length including cyclic prefix.
pub const SYM_LEN: usize = FFT_SIZE + CP;
/// Sample rate assumed by these parameters.
pub const SAMPLE_RATE: u32 = 8000;
/// Subcarrier spacing = SAMPLE_RATE / FFT_SIZE = 31.25 Hz.
pub const SC_SPACING_HZ: f32 = SAMPLE_RATE as f32 / FFT_SIZE as f32;
/// Pilot tone amplitude: known real BPSK +1.
pub const PILOT_AMPLITUDE: f32 = 1.0;
/// Default pilot spacing — every 5th SC in the occupied range is a pilot.
pub const DEFAULT_PILOT_SPACING: usize = 5;

/// Per-mode subcarrier layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScFdmaParams {
    /// First occupied subcarrier index (inclusive).
    pub first_sc: usize,
    /// Last occupied subcarrier index (inclusive).
    pub last_sc: usize,
    /// Number of data subcarriers (excludes pilots).
    pub n_data: usize,
    /// Number of pilot subcarriers.
    pub n_pilots: usize,
    /// Bits carried per data subcarrier per SC-FDMA symbol (2=QPSK, 3=8PSK, 4=16QAM, 5=32QAM, 6=64QAM).
    pub bits_per_sc: usize,
    /// Pilot spacing in occupied subcarriers.
    pub pilot_spacing: usize,
}

impl ScFdmaParams {
    /// Total occupied subcarriers (data + pilots).
    pub fn total_sc(&self) -> usize {
        self.last_sc - self.first_sc + 1
    }

    /// Bits per SC-FDMA symbol across all data subcarriers.
    pub fn bits_per_symbol(&self) -> usize {
        self.n_data * self.bits_per_sc
    }

    /// Gross bit rate (bps).
    pub fn gross_bps(&self) -> f32 {
        self.bits_per_symbol() as f32 / (SYM_LEN as f32 / SAMPLE_RATE as f32)
    }

    /// Return a copy with pilot spacing adjusted to match the observed coherence bandwidth.
    ///
    /// - `coh_bw_hz` < 100 Hz → spacing 4 (dense, every 125 Hz)
    /// - `coh_bw_hz` > 300 Hz → spacing 8 (sparse, every 250 Hz)
    /// - otherwise           → default spacing 5 (every 156.25 Hz)
    ///
    /// `n_pilots` and `n_data` are recomputed to preserve `total_sc()`.
    pub fn with_pilot_density(self, coh_bw_hz: f32) -> Self {
        let new_spacing = if coh_bw_hz < 100.0 {
            4
        } else if coh_bw_hz > 300.0 {
            8
        } else {
            DEFAULT_PILOT_SPACING
        };
        if new_spacing == self.pilot_spacing {
            return self;
        }
        let first_pilot = self.first_sc + new_spacing - 1;
        let n_pilots = if first_pilot <= self.last_sc {
            (self.last_sc - first_pilot) / new_spacing + 1
        } else {
            0
        };
        let n_data = self.total_sc().saturating_sub(n_pilots);
        Self {
            pilot_spacing: new_spacing,
            n_pilots,
            n_data,
            ..self
        }
    }
}

/// SCFDMA-16: 16 data SCs + 4 pilots, SCs 38–57, centre at SC 48 (1500 Hz), BW ≈ 625 Hz.
pub const SCFDMA16: ScFdmaParams = ScFdmaParams {
    first_sc: 38,
    last_sc: 57,
    n_data: 16,
    n_pilots: 4,
    bits_per_sc: 2,
    pilot_spacing: DEFAULT_PILOT_SPACING,
};

/// SCFDMA-52: 52 data SCs + 13 pilots, SCs 16–80, centre at SC 48 (1500 Hz), BW ≈ 2031 Hz.
pub const SCFDMA52: ScFdmaParams = ScFdmaParams {
    first_sc: 16,
    last_sc: 80,
    n_data: 52,
    n_pilots: 13,
    bits_per_sc: 2,
    pilot_spacing: DEFAULT_PILOT_SPACING,
};

/// SCFDMA-52 with 8PSK subcarriers: 4,333 bps gross.
pub const SCFDMA52_8PSK: ScFdmaParams = ScFdmaParams {
    first_sc: 16,
    last_sc: 80,
    n_data: 52,
    n_pilots: 13,
    bits_per_sc: 3,
    pilot_spacing: DEFAULT_PILOT_SPACING,
};

/// SCFDMA-52 with 16QAM subcarriers: 5,778 bps gross.
pub const SCFDMA52_16QAM: ScFdmaParams = ScFdmaParams {
    first_sc: 16,
    last_sc: 80,
    n_data: 52,
    n_pilots: 13,
    bits_per_sc: 4,
    pilot_spacing: DEFAULT_PILOT_SPACING,
};

/// SCFDMA-52 with cross-32QAM subcarriers: 7,222 bps gross.
///
/// Cross-32QAM uses a 6×6 PAM grid with the four corner points removed (32 = 36 − 4).
/// At 5 bits/SC, peak-SNR requirement is ~5 dB lower than 64QAM and matches VARA HF Level 16.
pub const SCFDMA52_32QAM: ScFdmaParams = ScFdmaParams {
    first_sc: 16,
    last_sc: 80,
    n_data: 52,
    n_pilots: 13,
    bits_per_sc: 5,
    pilot_spacing: DEFAULT_PILOT_SPACING,
};

/// SCFDMA-52 with 64QAM subcarriers: 8,667 bps gross.
pub const SCFDMA52_64QAM: ScFdmaParams = ScFdmaParams {
    first_sc: 16,
    last_sc: 80,
    n_data: 52,
    n_pilots: 13,
    bits_per_sc: 6,
    pilot_spacing: DEFAULT_PILOT_SPACING,
};

/// Experimental dense-pilot SCFDMA-52 with 64QAM:
/// 49 data SCs + 16 pilots (pilot spacing 4) within the same 16..80 allocation.
pub const SCFDMA52_64QAM_P4: ScFdmaParams = ScFdmaParams {
    first_sc: 16,
    last_sc: 80,
    n_data: 49,
    n_pilots: 16,
    bits_per_sc: 6,
    pilot_spacing: 4,
};

// ── Narrowband higher-order family ──────────────────────────────────────────────
//
// Half the SCFDMA52 width (SCs 32–63: 26 data + 6 pilots, BW ≈ 1000 Hz, centre SC
// 47.5 ≈ 1484 Hz — same centring convention as SCFDMA16). Concentrating the same
// transmit power into ~half the subcarriers raises per-subcarrier SNR by ~3 dB and
// cuts band-edge/ICI loss, so the dense constellations decode where the full-width
// SCFDMA52-* modes are SNR-starved. Lower throughput, higher robustness — the rung
// an adaptive profile drops to when the wide high-order mode won't close.

/// SCFDMA26 with 8PSK subcarriers (26 data SCs): ~2,167 bps gross.
pub const SCFDMA26_8PSK: ScFdmaParams = ScFdmaParams {
    first_sc: 32,
    last_sc: 63,
    n_data: 26,
    n_pilots: 6,
    bits_per_sc: 3,
    pilot_spacing: DEFAULT_PILOT_SPACING,
};

/// SCFDMA26 with 16QAM subcarriers (26 data SCs): ~2,889 bps gross.
pub const SCFDMA26_16QAM: ScFdmaParams = ScFdmaParams {
    first_sc: 32,
    last_sc: 63,
    n_data: 26,
    n_pilots: 6,
    bits_per_sc: 4,
    pilot_spacing: DEFAULT_PILOT_SPACING,
};

/// SCFDMA26 with cross-32QAM subcarriers (26 data SCs): ~3,611 bps gross.
pub const SCFDMA26_32QAM: ScFdmaParams = ScFdmaParams {
    first_sc: 32,
    last_sc: 63,
    n_data: 26,
    n_pilots: 6,
    bits_per_sc: 5,
    pilot_spacing: DEFAULT_PILOT_SPACING,
};

/// Select `ScFdmaParams` from a mode string (case-insensitive).
pub fn params_for_mode(mode: &str) -> Option<ScFdmaParams> {
    match mode.to_ascii_uppercase().as_str() {
        "SCFDMA16" => Some(SCFDMA16),
        "SCFDMA52" => Some(SCFDMA52),
        "SCFDMA52-8PSK" => Some(SCFDMA52_8PSK),
        "SCFDMA52-16QAM" => Some(SCFDMA52_16QAM),
        "SCFDMA52-32QAM" => Some(SCFDMA52_32QAM),
        "SCFDMA52-64QAM" => Some(SCFDMA52_64QAM),
        "SCFDMA52-64QAM-P4" => Some(SCFDMA52_64QAM_P4),
        "SCFDMA26-8PSK" => Some(SCFDMA26_8PSK),
        "SCFDMA26-16QAM" => Some(SCFDMA26_16QAM),
        "SCFDMA26-32QAM" => Some(SCFDMA26_32QAM),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scfdma16_geometry() {
        assert_eq!(SCFDMA16.total_sc(), 20);
        assert_eq!(SCFDMA16.n_data + SCFDMA16.n_pilots, 20);
        assert_eq!(SCFDMA16.bits_per_symbol(), 32);
    }

    #[test]
    fn scfdma52_geometry() {
        assert_eq!(SCFDMA52.total_sc(), 65);
        assert_eq!(SCFDMA52.n_data + SCFDMA52.n_pilots, 65);
        assert_eq!(SCFDMA52.bits_per_symbol(), 104);
    }

    #[test]
    fn scfdma52_8psk_geometry() {
        assert_eq!(SCFDMA52_8PSK.n_data, 52);
        assert_eq!(SCFDMA52_8PSK.bits_per_sc, 3);
        assert_eq!(SCFDMA52_8PSK.bits_per_symbol(), 156);
        // 52 × 3 × 8000/288 ≈ 4333 bps
        assert!((SCFDMA52_8PSK.gross_bps() - 4333.0).abs() < 5.0);
    }

    #[test]
    fn scfdma52_16qam_geometry() {
        assert_eq!(SCFDMA52_16QAM.n_data, 52);
        assert_eq!(SCFDMA52_16QAM.bits_per_sc, 4);
        assert_eq!(SCFDMA52_16QAM.bits_per_symbol(), 208);
        // 52 × 4 × 8000/288 ≈ 5778 bps
        assert!((SCFDMA52_16QAM.gross_bps() - 5778.0).abs() < 5.0);
    }

    #[test]
    fn scfdma52_32qam_geometry() {
        assert_eq!(SCFDMA52_32QAM.n_data, 52);
        assert_eq!(SCFDMA52_32QAM.bits_per_sc, 5);
        assert_eq!(SCFDMA52_32QAM.bits_per_symbol(), 260);
        // 52 × 5 × 8000/288 ≈ 7222 bps
        assert!((SCFDMA52_32QAM.gross_bps() - 7222.0).abs() < 5.0);
    }

    #[test]
    fn scfdma52_64qam_geometry() {
        assert_eq!(SCFDMA52_64QAM.n_data, 52);
        assert_eq!(SCFDMA52_64QAM.bits_per_sc, 6);
        assert_eq!(SCFDMA52_64QAM.bits_per_symbol(), 312);
        // 52 × 6 × 8000/288 ≈ 8667 bps
        assert!((SCFDMA52_64QAM.gross_bps() - 8667.0).abs() < 5.0);
    }

    #[test]
    fn with_pilot_density_dense() {
        let p = SCFDMA52.with_pilot_density(50.0); // < 100 Hz → spacing 4
        assert_eq!(p.pilot_spacing, 4);
        assert_eq!(p.n_pilots, 16);
        assert_eq!(p.n_data, 49);
        assert_eq!(p.n_data + p.n_pilots, SCFDMA52.total_sc());
        assert_eq!(p.first_sc, SCFDMA52.first_sc);
        assert_eq!(p.last_sc, SCFDMA52.last_sc);
        assert_eq!(p.bits_per_sc, SCFDMA52.bits_per_sc);
    }

    #[test]
    fn with_pilot_density_sparse() {
        let p = SCFDMA52.with_pilot_density(400.0); // > 300 Hz → spacing 8
        assert_eq!(p.pilot_spacing, 8);
        assert_eq!(p.n_pilots, 8);
        assert_eq!(p.n_data, 57);
        assert_eq!(p.n_data + p.n_pilots, SCFDMA52.total_sc());
    }

    #[test]
    fn with_pilot_density_default_unchanged() {
        let p = SCFDMA52.with_pilot_density(200.0); // 100–300 Hz → default spacing 5
        assert_eq!(p, SCFDMA52);
    }

    #[test]
    fn scfdma52_64qam_p4_geometry() {
        assert_eq!(SCFDMA52_64QAM_P4.total_sc(), 65);
        assert_eq!(SCFDMA52_64QAM_P4.n_data, 49);
        assert_eq!(SCFDMA52_64QAM_P4.n_pilots, 16);
        assert_eq!(SCFDMA52_64QAM_P4.n_data + SCFDMA52_64QAM_P4.n_pilots, 65);
        assert_eq!(SCFDMA52_64QAM_P4.bits_per_symbol(), 294);
        // 49 × 6 × 8000/288 ≈ 8167 bps
        assert!((SCFDMA52_64QAM_P4.gross_bps() - 8167.0).abs() < 5.0);
    }

    #[test]
    fn scfdma26_family_geometry() {
        // Half-width: 32 occupied SCs (26 data + 6 pilots), ~1000 Hz, centre 47.5.
        for p in [SCFDMA26_8PSK, SCFDMA26_16QAM, SCFDMA26_32QAM] {
            assert_eq!(p.total_sc(), 32);
            assert_eq!(p.n_data, 26);
            assert_eq!(p.n_pilots, 6);
            assert_eq!(p.n_data + p.n_pilots, p.total_sc());
            assert_eq!(p.first_sc, 32);
            assert_eq!(p.last_sc, 63);
        }
        // ~half the SCFDMA52 occupied width → ~+3 dB per-subcarrier power.
        assert!((SCFDMA52.total_sc() as f32 / 32.0 - 2.03).abs() < 0.05);
        assert_eq!(SCFDMA26_8PSK.bits_per_sc, 3);
        assert_eq!(SCFDMA26_16QAM.bits_per_sc, 4);
        assert_eq!(SCFDMA26_32QAM.bits_per_sc, 5);
    }
}
