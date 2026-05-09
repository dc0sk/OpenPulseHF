//! OFDM subcarrier layout and frame geometry constants.

/// FFT size used for all OFDM modes.  Must be a power of two.
pub const FFT_SIZE: usize = 256;
/// Cyclic prefix length in samples.
pub const CP: usize = 32;
/// OFDM symbol length including cyclic prefix.
pub const SYM_LEN: usize = FFT_SIZE + CP;
/// Sample rate assumed by these parameters.
pub const SAMPLE_RATE: u32 = 8000;
/// Subcarrier spacing = SAMPLE_RATE / FFT_SIZE = 31.25 Hz.
pub const SC_SPACING_HZ: f32 = SAMPLE_RATE as f32 / FFT_SIZE as f32;
/// Pilot tone: known real BPSK +1.
pub const PILOT_AMPLITUDE: f32 = 1.0;
/// Pilot spacing — every 5th SC in the occupied range is a pilot.
pub const PILOT_SPACING: usize = 5;
/// Target PAPR after iterative clipping (dB).
pub const TARGET_PAPR_DB: f32 = 6.0;
/// Maximum iterations for iterative PAPR clipping.
pub const CLIP_MAX_ITER: usize = 50;

/// Per-mode subcarrier layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OfdmParams {
    /// First occupied subcarrier index (inclusive).
    pub first_sc: usize,
    /// Last occupied subcarrier index (inclusive).
    pub last_sc: usize,
    /// Number of data subcarriers (excludes pilots).
    pub n_data: usize,
    /// Number of pilot subcarriers.
    pub n_pilots: usize,
}

impl OfdmParams {
    /// Total occupied subcarriers (data + pilots).
    pub fn total_sc(&self) -> usize {
        self.last_sc - self.first_sc + 1
    }

    /// Bits per OFDM symbol (QPSK = 2 bits/SC).
    pub fn bits_per_symbol(&self) -> usize {
        self.n_data * 2
    }

    /// Bytes per OFDM symbol (rounded down).
    pub fn bytes_per_symbol(&self) -> usize {
        self.bits_per_symbol() / 8
    }

    /// Gross bit rate (bps).
    pub fn gross_bps(&self) -> f32 {
        self.bits_per_symbol() as f32 / (SYM_LEN as f32 / SAMPLE_RATE as f32)
    }

    /// Occupied bandwidth (Hz), from first SC edge to last SC edge.
    pub fn occupied_bw_hz(&self) -> f32 {
        self.total_sc() as f32 * SC_SPACING_HZ
    }

    /// Centre frequency of the occupied band (Hz).
    pub fn centre_hz(&self) -> f32 {
        (self.first_sc + self.last_sc) as f32 * 0.5 * SC_SPACING_HZ
    }
}

/// OFDM-16: 16 data SCs + 4 pilots, SCs 38–57, centre at SC 48 (1500 Hz), BW ≈ 625 Hz.
pub const OFDM16: OfdmParams = OfdmParams {
    first_sc: 38,
    last_sc: 57,
    n_data: 16,
    n_pilots: 4,
};

/// OFDM-52: 52 data SCs + 13 pilots, SCs 16–80, centre at SC 48 (1500 Hz), BW ≈ 2031 Hz.
pub const OFDM52: OfdmParams = OfdmParams {
    first_sc: 16,
    last_sc: 80,
    n_data: 52,
    n_pilots: 13,
};

/// Select `OfdmParams` from a mode string (case-insensitive).
pub fn params_for_mode(mode: &str) -> Option<OfdmParams> {
    match mode.to_ascii_uppercase().as_str() {
        "OFDM16" => Some(OFDM16),
        "OFDM52" => Some(OFDM52),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ofdm16_geometry() {
        assert_eq!(OFDM16.total_sc(), 20);
        assert_eq!(OFDM16.n_data + OFDM16.n_pilots, 20);
        // OFDM16 has 20 SCs (even), so the geometric centre falls between SCs 47 and 48.
        // Accept within one subcarrier spacing (31.25 Hz) of 1500 Hz.
        assert!((OFDM16.centre_hz() - 1500.0).abs() < 32.0);
    }

    #[test]
    fn ofdm52_geometry() {
        assert_eq!(OFDM52.total_sc(), 65);
        assert_eq!(OFDM52.n_data + OFDM52.n_pilots, 65);
        assert!((OFDM52.centre_hz() - 1500.0).abs() < 1.0);
    }
}
