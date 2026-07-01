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

    /// Welch power spectrum (dBFS): average the power of up to `max_segments` Hann-windowed
    /// `FFT_SIZE` segments spread evenly across `samples`. Unlike [`compute`](Self::compute) — which
    /// only windows the first `FFT_SIZE` samples (a fixed preamble for a framed burst, so its trace
    /// is static) — this covers the whole burst, so the estimate reflects the actual modulated data.
    /// The averaging is bounded (not a full-burst average, which would converge to a static
    /// envelope) so a finite-sample variance remains and the trace varies naturally frame-to-frame.
    /// Falls back to [`compute`](Self::compute) when the burst is shorter than one segment.
    pub fn compute_welch(&mut self, samples: &[f32], max_segments: usize) -> Vec<f32> {
        if samples.len() <= FFT_SIZE {
            return self.compute(samples);
        }
        // Half-overlapped windows that fit, capped at `max_segments` to keep some variance.
        let fit = 1 + (samples.len() - FFT_SIZE) / (FFT_SIZE / 2);
        let n_seg = fit.min(max_segments.max(1)).max(1);
        let last_start = samples.len() - FFT_SIZE;
        let fft = self.planner.plan_fft_forward(FFT_SIZE);
        let mut acc = vec![0.0f32; FREQ_BINS];
        let mut buf = vec![Complex::new(0.0f32, 0.0); FFT_SIZE];
        for s in 0..n_seg {
            // Evenly space the segment starts from 0 to the last full-segment position.
            let start = if n_seg == 1 {
                0
            } else {
                last_start * s / (n_seg - 1)
            };
            for (i, b) in buf.iter_mut().enumerate() {
                *b = Complex::new(samples[start + i] * self.window[i], 0.0);
            }
            fft.process(&mut buf);
            for (a, c) in acc.iter_mut().zip(buf.iter().take(FREQ_BINS)) {
                *a += c.norm_sqr();
            }
        }
        let scale = 1.0 / (n_seg as f32 * FFT_SIZE as f32);
        acc.iter()
            .map(|&p| 10.0 * (p * scale).max(1e-12).log10())
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

    #[test]
    fn welch_tracks_a_tone() {
        // A Welch PSD of a 1500 Hz tone still peaks at bin 192 (it's a valid PSD estimate).
        let mut ps = PowerSpectrum::new();
        let tone: Vec<f32> = (0..FFT_SIZE * 4)
            .map(|i| (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
            .collect();
        let spectrum = ps.compute_welch(&tone, 6);
        let peak_bin = spectrum
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        assert_eq!(peak_bin, 192, "1500 Hz tone should peak at bin 192");
    }

    #[test]
    fn welch_reflects_the_whole_burst_not_just_the_prefix() {
        // Two bursts that share an identical first-FFT_SIZE prefix (the "preamble") but differ
        // afterwards: `compute` sees only the prefix so it returns identical spectra (the frozen
        // trace), while `compute_welch` covers the whole burst so it differs — the mechanism that
        // makes the linksim TX spectrum breathe over the real random data (no synthetic jitter).
        let mut ps = PowerSpectrum::new();
        let prefix: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
            .collect();
        let mut a = prefix.clone();
        let mut b = prefix.clone();
        // Different "data" tails (different tones) after the shared prefix.
        for i in 0..FFT_SIZE * 3 {
            let t = (FFT_SIZE + i) as f32 / 8000.0;
            a.push((2.0 * std::f32::consts::PI * 1200.0 * t).sin());
            b.push((2.0 * std::f32::consts::PI * 1800.0 * t).sin());
        }
        // `compute` only windows the shared prefix → identical.
        assert_eq!(
            ps.compute(&a),
            ps.compute(&b),
            "prefix-only spectra should match"
        );
        // `compute_welch` covers the differing tails → differs.
        assert_ne!(
            ps.compute_welch(&a, 6),
            ps.compute_welch(&b, 6),
            "whole-burst Welch spectra should differ when the data differs"
        );
    }
}
