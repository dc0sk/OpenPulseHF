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
/// Pilot spacing — every 5th SC in the occupied range is a pilot.
pub const PILOT_SPACING: usize = 5;

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
}

impl ScFdmaParams {
    /// Total occupied subcarriers (data + pilots).
    pub fn total_sc(&self) -> usize {
        self.last_sc - self.first_sc + 1
    }

    /// Bits per SC-FDMA symbol (QPSK = 2 bits/data SC).
    pub fn bits_per_symbol(&self) -> usize {
        self.n_data * 2
    }

    /// Gross bit rate (bps).
    pub fn gross_bps(&self) -> f32 {
        self.bits_per_symbol() as f32 / (SYM_LEN as f32 / SAMPLE_RATE as f32)
    }
}

/// SCFDMA-16: 16 data SCs + 4 pilots, SCs 38–57, centre at SC 48 (1500 Hz), BW ≈ 625 Hz.
pub const SCFDMA16: ScFdmaParams = ScFdmaParams {
    first_sc: 38,
    last_sc: 57,
    n_data: 16,
    n_pilots: 4,
};

/// SCFDMA-52: 52 data SCs + 13 pilots, SCs 16–80, centre at SC 48 (1500 Hz), BW ≈ 2031 Hz.
pub const SCFDMA52: ScFdmaParams = ScFdmaParams {
    first_sc: 16,
    last_sc: 80,
    n_data: 52,
    n_pilots: 13,
};

/// Select `ScFdmaParams` from a mode string (case-insensitive).
pub fn params_for_mode(mode: &str) -> Option<ScFdmaParams> {
    match mode.to_ascii_uppercase().as_str() {
        "SCFDMA16" => Some(SCFDMA16),
        "SCFDMA52" => Some(SCFDMA52),
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
}
