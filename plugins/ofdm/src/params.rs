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
/// Preamble subcarrier amplitude.  Only even SCs are loaded (half the band), so
/// boosting by √2 keeps the preamble's total power comparable to a data symbol.
pub const PREAMBLE_AMPLITUDE: f32 = std::f32::consts::SQRT_2;
/// Pilot spacing — every 5th SC in the occupied range is a pilot.
pub const PILOT_SPACING: usize = 5;
/// Target PAPR after iterative clipping (dB).
///
/// 12 dB allows peak reduction without introducing ICI that corrupts data
/// subcarriers in OFDM52 (25% FFT loading).  Below ~10.8 dB, clipping ICI
/// causes systematic bit errors in zero-padded RS(255,223) blocks.
/// 6 dB was the original value and was far too aggressive.
pub const TARGET_PAPR_DB: f32 = 12.0;
/// Maximum iterations for iterative PAPR clipping.
pub const CLIP_MAX_ITER: usize = 50;

/// Deterministic ±1 BPSK sign for the timing-acquisition preamble at SC index `k`.
///
/// A whitened (pseudo-random) pattern keeps the preamble PAPR moderate and gives
/// a sharp autocorrelation peak; an all-`+1` comb would be highly peaked.
pub fn preamble_sign(k: usize) -> f32 {
    let h = (k as u32).wrapping_mul(2_654_435_761) >> 13;
    if h & 1 == 0 {
        1.0
    } else {
        -1.0
    }
}

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
    /// Bits carried per data subcarrier (2=QPSK, 3=8PSK, 4=16QAM, 5=32QAM, 6=64QAM).
    pub bits_per_sc: usize,
}

impl OfdmParams {
    /// Total occupied subcarriers (data + pilots).
    pub fn total_sc(&self) -> usize {
        self.last_sc - self.first_sc + 1
    }

    /// Bits per OFDM symbol = data subcarriers × bits per subcarrier.
    pub fn bits_per_symbol(&self) -> usize {
        self.n_data * self.bits_per_sc
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
    bits_per_sc: 2,
};

/// OFDM-52: 52 data SCs + 13 pilots, SCs 16–80, centre at SC 48 (1500 Hz), BW ≈ 2031 Hz.
pub const OFDM52: OfdmParams = OfdmParams {
    first_sc: 16,
    last_sc: 80,
    n_data: 52,
    n_pilots: 13,
    bits_per_sc: 2,
};

/// OFDM-52 with 16QAM subcarriers: same SC layout as OFDM52, 4 bits/SC ≈ 5778 bps gross.
///
/// OFDM (not SC-FDMA) is the higher-throughput/higher-reliability HF path: the CP +
/// per-subcarrier equalization handle frequency-selective multipath natively, with no
/// DFT-despread noise enhancement.  Run FEC-protected (soft).
pub const OFDM52_16QAM: OfdmParams = OfdmParams {
    first_sc: 16,
    last_sc: 80,
    n_data: 52,
    n_pilots: 13,
    bits_per_sc: 4,
};

/// OFDM-52 with 8PSK subcarriers: 3 bits/SC ≈ 4333 bps gross.
pub const OFDM52_8PSK: OfdmParams = OfdmParams {
    first_sc: 16,
    last_sc: 80,
    n_data: 52,
    n_pilots: 13,
    bits_per_sc: 3,
};

/// OFDM-52 with cross-32QAM subcarriers: 5 bits/SC ≈ 7222 bps gross.
pub const OFDM52_32QAM: OfdmParams = OfdmParams {
    first_sc: 16,
    last_sc: 80,
    n_data: 52,
    n_pilots: 13,
    bits_per_sc: 5,
};

/// OFDM-52 with 64QAM subcarriers: 6 bits/SC ≈ 8667 bps gross.
pub const OFDM52_64QAM: OfdmParams = OfdmParams {
    first_sc: 16,
    last_sc: 80,
    n_data: 52,
    n_pilots: 13,
    bits_per_sc: 6,
};

/// Select `OfdmParams` from a mode string (case-insensitive).
pub fn params_for_mode(mode: &str) -> Option<OfdmParams> {
    match mode.to_ascii_uppercase().as_str() {
        "OFDM16" => Some(OFDM16),
        "OFDM52" => Some(OFDM52),
        "OFDM52-8PSK" => Some(OFDM52_8PSK),
        "OFDM52-16QAM" => Some(OFDM52_16QAM),
        "OFDM52-32QAM" => Some(OFDM52_32QAM),
        "OFDM52-64QAM" => Some(OFDM52_64QAM),
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
