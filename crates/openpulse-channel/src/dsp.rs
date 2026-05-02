//! DSP utilities: power spectrum estimation and waterfall buffer.
//!
//! Used by the testbench GUI to display real-time spectral data.

use rustfft::{num_complex::Complex, FftPlanner};

/// FFT size for spectrum analysis (yields 512 positive-frequency bins at 8000 Hz).
pub const FFT_SIZE: usize = 1024;

/// Number of positive-frequency bins (FFT_SIZE / 2).
pub const FREQ_BINS: usize = FFT_SIZE / 2;

/// Number of rows retained in the waterfall history.
pub const WATERFALL_ROWS: usize = 200;

/// Pre-computed Hann window coefficients (power-normalised).
fn hann_window() -> Vec<f32> {
    let n = FFT_SIZE as f32;
    // Power normalisation: sum(w²) = N/2 for Hann → scale by sqrt(2/N).
    let scale = (2.0 / n).sqrt();
    (0..FFT_SIZE)
        .map(|i| {
            scale
                * 0.5
                * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos())
        })
        .collect()
}

/// Computes the single-sided power spectrum (dBFS) of a block of samples.
///
/// Returns `FREQ_BINS` values in the range roughly [-120, 0] dBFS.
pub struct PowerSpectrum {
    planner: FftPlanner<f32>,
    window: Vec<f32>,
}

impl PowerSpectrum {
    pub fn new() -> Self {
        Self {
            planner: FftPlanner::new(),
            window: hann_window(),
        }
    }

    /// Compute the power spectrum from `samples`.
    ///
    /// If `samples.len() < FFT_SIZE` the buffer is zero-padded.
    /// If longer, only the first FFT_SIZE samples are used.
    pub fn compute(&mut self, samples: &[f32]) -> Vec<f32> {
        let mut buf: Vec<Complex<f32>> = (0..FFT_SIZE)
            .map(|i| {
                let s = if i < samples.len() { samples[i] } else { 0.0 };
                Complex::new(s * self.window[i], 0.0)
            })
            .collect();

        let fft = self.planner.plan_fft_forward(FFT_SIZE);
        fft.process(&mut buf);

        (0..FREQ_BINS)
            .map(|k| {
                let power = buf[k].norm_sqr() / FFT_SIZE as f32;
                10.0 * power.max(1e-12).log10()
            })
            .collect()
    }

    /// Bin index for a given frequency at the configured sample rate.
    pub fn freq_to_bin(freq_hz: f32, sample_rate: u32) -> usize {
        ((freq_hz / sample_rate as f32) * FFT_SIZE as f32).round() as usize
    }
}

impl Default for PowerSpectrum {
    fn default() -> Self {
        Self::new()
    }
}

/// Rolling waterfall buffer: newest row first.
pub struct WaterfallBuffer {
    rows: Vec<Vec<u8>>,
    capacity: usize,
}

impl WaterfallBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            rows: Vec::with_capacity(capacity),
            capacity,
        }
    }

    /// Push a new power spectrum row (converted to a plasma colormap index 0–255).
    ///
    /// `min_db` and `max_db` define the dBFS range mapped to 0–255.
    pub fn push(&mut self, spectrum: &[f32], min_db: f32, max_db: f32) {
        let row: Vec<u8> = spectrum
            .iter()
            .map(|&db| {
                let norm = (db - min_db) / (max_db - min_db);
                (norm.clamp(0.0, 1.0) * 255.0) as u8
            })
            .collect();
        if self.rows.len() == self.capacity {
            self.rows.pop();
        }
        self.rows.insert(0, row);
    }

    pub fn rows(&self) -> &[Vec<u8>] {
        &self.rows
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1500 Hz tone at 8000 Hz sample rate must peak at bin 192.
    ///
    /// bin = round(1500 / 8000 × 1024) = round(192.0) = 192
    #[test]
    fn fft_1500hz_peaks_at_bin_192() {
        let mut ps = PowerSpectrum::new();
        let tone: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
            .collect();
        let spectrum = ps.compute(&tone);

        let expected_bin = PowerSpectrum::freq_to_bin(1500.0, 8000);
        assert_eq!(expected_bin, 192);

        let peak_bin = spectrum
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        assert_eq!(
            peak_bin, expected_bin,
            "1500 Hz tone should peak at bin 192"
        );
    }

    #[test]
    fn waterfall_capacity_respected() {
        let mut wf = WaterfallBuffer::new(5);
        let spectrum = vec![0.0f32; FREQ_BINS];
        for _ in 0..10 {
            wf.push(&spectrum, -120.0, 0.0);
        }
        assert_eq!(wf.rows().len(), 5);
    }

    #[test]
    fn freq_to_bin_1500hz() {
        assert_eq!(PowerSpectrum::freq_to_bin(1500.0, 8000), 192);
    }
}
