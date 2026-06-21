//! Watterson two-ray ITU-R F.1487 ionospheric channel.
//!
//! Models two delayed Rayleigh-faded rays, each with independent complex
//! Gaussian fading envelopes shaped by a Doppler-spread spectral filter.
//!
//! # Implementation notes
//!
//! The Doppler envelope is synthesised in the frequency domain:
//!   1. Generate N complex Gaussian samples where N = next_power_of_two(signal_len).
//!   2. FFT → apply Gaussian spectral filter centred at DC with σ proportional
//!      to `doppler_spread_hz / (sample_rate / N)`.
//!   3. IFFT → time-domain fading envelope (first `signal_len` samples used).
//!   4. Scale so that E[|h|²] = 1.
//!
//! Using N = next_power_of_two(signal_len) gives proper temporal correlation over the
//! full call rather than jumping to independent states at fixed-size block boundaries.

use rand::Rng;
use rand::SeedableRng;
use rand_distr::StandardNormal;
use rustfft::{num_complex::Complex, FftPlanner};

type Complex32 = Complex<f32>;

use crate::{ChannelError, ChannelModel, WattersonConfig};

/// Two-ray Watterson ionospheric fading channel.
pub struct WattersonChannel {
    config: WattersonConfig,
    rng: rand::rngs::StdRng,
    planner: FftPlanner<f32>,
}

impl WattersonChannel {
    pub fn new(config: WattersonConfig) -> Result<Self, ChannelError> {
        if !config.doppler_spread_hz.is_finite() || config.doppler_spread_hz < 0.0 {
            return Err(ChannelError::InvalidParameter(
                "doppler_spread_hz must be a non-negative finite value".into(),
            ));
        }
        if !config.snr_db.is_finite() {
            return Err(ChannelError::InvalidParameter(
                "snr_db must be finite".into(),
            ));
        }
        if !config.delay_spread_ms.is_finite() || config.delay_spread_ms < 0.0 {
            return Err(ChannelError::InvalidParameter(
                "delay_spread_ms must be a non-negative finite value".into(),
            ));
        }
        if config.sample_rate == 0 {
            return Err(ChannelError::InvalidParameter(
                "sample_rate must be > 0".into(),
            ));
        }

        let rng = match config.seed {
            Some(s) => rand::rngs::StdRng::seed_from_u64(s),
            None => rand::rngs::StdRng::from_entropy(),
        };
        let planner = FftPlanner::new();

        Ok(Self {
            config,
            rng,
            planner,
        })
    }

    /// Generate `n` fading envelope samples with Doppler-spectrum shaping.
    ///
    /// Uses a single FFT of length ≥ n (next power of two) so all samples within
    /// the call share a single coherent realization.  This gives correct temporal
    /// correlation across the full signal length rather than jumping to independent
    /// states at each fixed-size block boundary.
    ///
    /// For low Doppler spreads (e.g. F1 = 0.1 Hz), the signal-length FFT alone
    /// yields a sub-bin shaping filter (σ_bins ≪ 1) that would collapse to the
    /// 0.5 floor and produce a near-constant envelope.  The FFT is therefore
    /// enlarged so σ_bins ≥ `TARGET_SIGMA_BINS`, up to `MAX_FFT` samples
    /// (~2 MB of `Complex<f32>`).
    fn make_envelope(&mut self, n: usize) -> Vec<Complex32> {
        const TARGET_SIGMA_BINS: f32 = 2.0;
        const MAX_FFT: usize = 1 << 18;
        let signal_fft = n.next_power_of_two().max(4);
        let sr = self.config.sample_rate as f32;
        let required_fft = if self.config.doppler_spread_hz > 1e-4 {
            (TARGET_SIGMA_BINS * sr / self.config.doppler_spread_hz).ceil() as usize
        } else {
            signal_fft
        };
        let fft_size = signal_fft.max(required_fft.next_power_of_two().min(MAX_FFT));
        // Random complex Gaussian spectrum.
        let mut spec: Vec<Complex<f32>> = (0..fft_size)
            .map(|_| {
                Complex::new(
                    self.rng.sample::<f32, _>(StandardNormal),
                    self.rng.sample::<f32, _>(StandardNormal),
                )
            })
            .collect();

        // Gaussian Doppler shaping: sigma = doppler_hz / bin_width.
        // `fft_size` is sized above so σ_bins ≥ TARGET_SIGMA_BINS for non-trivial
        // Doppler; the 0.5 floor remains as defense-in-depth for the doppler≈0 case
        // and for the MAX_FFT cap.
        let sigma_bins = (self.config.doppler_spread_hz / (sr / fft_size as f32)).max(0.5);

        let filter_energy: f32 = (0..fft_size)
            .map(|k| {
                let freq = if k <= fft_size / 2 {
                    k as f32
                } else {
                    k as f32 - fft_size as f32
                };
                (-0.5 * (freq / sigma_bins).powi(2)).exp().powi(2)
            })
            .sum::<f32>();

        for (k, s) in spec.iter_mut().enumerate() {
            let freq = if k <= fft_size / 2 {
                k as f32
            } else {
                k as f32 - fft_size as f32
            };
            let h = (-0.5 * (freq / sigma_bins).powi(2)).exp();
            *s *= h;
        }

        // IFFT to time domain.
        let ifft = self.planner.plan_fft_inverse(fft_size);
        ifft.process(&mut spec);

        // Normalize to unit mean-square.  For rustfft's unnormalized IFFT, each time-domain
        // sample satisfies E[|h[n]|^2] = Σ_k E[|X[k]|^2] = 2·filter_energy (independent of
        // fft_size — the 1/N from Parseval cancels the N from the unnormalized transform).
        let scale = 1.0 / (2.0 * filter_energy).sqrt();
        spec[..n]
            .iter()
            .map(|c| Complex32::new(c.re * scale, c.im * scale))
            .collect()
    }

    /// Analytic signal of a real input via the FFT Hilbert method (re = input, im = Hilbert).
    fn analytic(&mut self, x: &[f32]) -> Vec<Complex32> {
        let n = x.len();
        let mut buf: Vec<Complex32> = x.iter().map(|&v| Complex32::new(v, 0.0)).collect();
        self.planner.plan_fft_forward(n).process(&mut buf);
        let half = n.div_ceil(2); // index of the first negative-frequency bin
        for v in buf.iter_mut().take(half).skip(1) {
            *v *= 2.0; // double the positive frequencies
        }
        for v in buf.iter_mut().skip(half) {
            *v = Complex32::new(0.0, 0.0); // zero the negative frequencies
        }
        self.planner.plan_fft_inverse(n).process(&mut buf);
        let scale = 1.0 / n as f32;
        for v in buf.iter_mut() {
            *v *= scale;
        }
        buf
    }

    /// Apply Watterson fading to complex baseband input using one coherent
    /// channel realization for both I and Q rails.
    pub fn apply_complex(&mut self, i_in: &[f32], q_in: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let n = i_in.len().min(q_in.len());
        if n == 0 {
            return (Vec::new(), Vec::new());
        }

        let env0 = self.make_envelope(n);
        let env1 = self.make_envelope(n);
        let delay_samples =
            (self.config.delay_spread_ms / 1000.0 * self.config.sample_rate as f32) as usize;

        let signal_rms = (i_in
            .iter()
            .zip(q_in.iter())
            .take(n)
            .map(|(&i, &q)| i * i + q * q)
            .sum::<f32>()
            / n as f32)
            .sqrt();

        // Per-component sigma so the total complex-noise RMS tracks the requested SNR.
        let noise_sigma = if signal_rms > 0.0 {
            signal_rms / (10f32.powf(self.config.snr_db / 20.0) * std::f32::consts::SQRT_2)
        } else {
            1e-4
        };

        let mut out_i = vec![0.0_f32; n];
        let mut out_q = vec![0.0_f32; n];

        for idx in 0..n {
            let x0 = Complex32::new(i_in[idx], q_in[idx]);
            let x1 = if idx >= delay_samples {
                Complex32::new(i_in[idx - delay_samples], q_in[idx - delay_samples])
            } else {
                Complex32::new(0.0, 0.0)
            };

            let y = env0[idx] * x0 + env1[idx] * x1;
            out_i[idx] = y.re + noise_sigma * self.rng.sample::<f32, _>(StandardNormal);
            out_q[idx] = y.im + noise_sigma * self.rng.sample::<f32, _>(StandardNormal);
        }

        (out_i, out_q)
    }
}

impl ChannelModel for WattersonChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        let n = input.len();
        if n == 0 {
            return Vec::new();
        }

        // Generate full-length envelopes so the fading is temporally correlated
        // across the entire call — no discontinuous jumps at fixed block boundaries.
        let env0 = self.make_envelope(n);
        let env1 = self.make_envelope(n);

        let delay_samples =
            (self.config.delay_spread_ms / 1000.0 * self.config.sample_rate as f32) as usize;
        let rms = (input.iter().map(|&s| s * s).sum::<f32>() / n as f32).sqrt();
        let noise_sigma = if rms > 0.0 {
            rms / 10f32.powf(self.config.snr_db / 20.0)
        } else {
            1e-4
        };

        // Apply each ray's complex gain to the analytic signal: out = Re{ analytic(s) · h }.
        // This rotates the carrier phase by arg(h) and scales by the Rayleigh magnitude |h|.
        // Multiplying the real passband signal by Re{h} directly (the previous approach) drops
        // the quadrature term, so the signal was annihilated whenever arg(h) ≈ ±90° — a deep
        // fade independent of |h| or SNR, with spurious sign inversions. The analytic-signal
        // form preserves |h| and turns a 90° gain into a harmless carrier-phase rotation.
        let analytic = self.analytic(input);
        let mut out = vec![0.0f32; n];
        for i in 0..n {
            let ray0 = analytic[i] * env0[i];
            let ray1 = if i >= delay_samples {
                analytic[i - delay_samples] * env1[i]
            } else {
                Complex32::new(0.0, 0.0)
            };
            out[i] = (ray0 + ray1).re + noise_sigma * self.rng.sample::<f32, _>(StandardNormal);
        }
        out
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        // Multiplicative fading is not independent additive noise.
        vec![0.0; length]
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WattersonConfig;

    /// The Watterson output must exhibit non-trivial amplitude variation across
    /// blocks (coefficient of variation > 10 %).
    ///
    /// Uses the extreme profile (10 Hz Doppler) where sigma_bins > 1 so the
    /// fading envelope varies meaningfully within each 1024-sample window.
    #[test]
    fn non_trivial_fading_envelope() {
        let cfg = WattersonConfig::extreme(Some(7));
        let mut ch = WattersonChannel::new(cfg).unwrap();

        let block_size = 256usize;
        let n_blocks = 20usize;
        let signal: Vec<f32> = (0..block_size)
            .map(|i| (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
            .collect();

        let mut block_rms: Vec<f32> = Vec::with_capacity(n_blocks);
        for _ in 0..n_blocks {
            let out = ch.apply(&signal);
            let rms = (out.iter().map(|&s| s * s).sum::<f32>() / block_size as f32).sqrt();
            block_rms.push(rms);
        }

        let mean = block_rms.iter().sum::<f32>() / n_blocks as f32;
        let variance = block_rms.iter().map(|&r| (r - mean).powi(2)).sum::<f32>() / n_blocks as f32;
        let cv = variance.sqrt() / mean; // coefficient of variation

        assert!(
            cv > 0.10,
            "coefficient of variation {:.3} should be > 0.10 (non-trivial fading)",
            cv
        );
    }

    /// The analytic-signal helper must reconstruct the input on the real rail and produce a
    /// near-constant magnitude envelope for a pure tone (the Hilbert pair of a sinusoid).
    #[test]
    fn analytic_signal_recovers_real_and_flat_envelope() {
        let cfg = WattersonConfig::good_f1(Some(1));
        let mut ch = WattersonChannel::new(cfg).unwrap();
        let n = 4096usize;
        let tone: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
            .collect();
        let a = ch.analytic(&tone);

        // Real part reconstructs the input.
        let re_err = a
            .iter()
            .zip(&tone)
            .map(|(c, &s)| (c.re - s).abs())
            .fold(0.0f32, f32::max);
        assert!(
            re_err < 1e-3,
            "analytic.re must equal input (max err {re_err})"
        );

        // Magnitude of a tone's analytic signal is flat (ignore FFT edge transients).
        let mid = &a[256..n - 256];
        let mean = mid.iter().map(|c| c.norm()).sum::<f32>() / mid.len() as f32;
        let cv = (mid.iter().map(|c| (c.norm() - mean).powi(2)).sum::<f32>() / mid.len() as f32)
            .sqrt()
            / mean;
        assert!(
            cv < 0.05,
            "tone analytic envelope should be flat (CV {cv:.3})"
        );
    }

    /// Regression guard for the dropped-quadrature bug: flat fading (no multipath) must scale
    /// a tone by the Rayleigh magnitude |h|, which rarely approaches zero — NOT by Re{h},
    /// which collapses to ~0 whenever the carrier phase lands near ±90°. The old `s·Re{h}`
    /// path deep-faded ~16% of realizations below 0.2× even at high SNR; the analytic-signal
    /// path keeps that fraction small.
    #[test]
    fn flat_fading_does_not_phase_annihilate() {
        let n = 1000usize; // ~0.13 s — short vs the coherence time below, so |h| ≈ const per call
        let tone: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
            .collect();
        let in_rms = (tone.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();

        let mut deep = 0;
        let seeds = 40u64;
        for seed in 0..seeds {
            let mut cfg = WattersonConfig::good_f1(Some(seed));
            cfg.doppler_spread_hz = 0.5; // ~1 s coherence ≫ the 0.13 s window; keeps envelope FFT small
            cfg.delay_spread_ms = 0.0; // flat fade only — isolate the per-sample gain
            cfg.snr_db = 60.0; // negligible additive noise
            let mut ch = WattersonChannel::new(cfg).unwrap();
            let out = ch.apply(&tone);
            let out_rms = (out.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();
            if out_rms / in_rms < 0.2 {
                deep += 1;
            }
        }
        // Rayleigh |h| dips below 0.2 only ~4% of the time; allow generous slack. The buggy
        // Re{h} path produced ~16% and would fail this bound.
        assert!(
            deep <= seeds as usize / 10,
            "{deep}/{seeds} flat-fade realizations collapsed below 0.2× — phase annihilation regressed"
        );
    }

    #[test]
    fn rejects_negative_doppler() {
        let mut cfg = WattersonConfig::moderate_f1(None);
        cfg.doppler_spread_hz = -1.0;
        assert!(WattersonChannel::new(cfg).is_err());
    }

    /// The Good-F1 profile (Doppler = 0.1 Hz) historically collapsed to a
    /// near-constant envelope because the shaping σ_bins fell below the 0.5
    /// floor.  After auto-sizing the FFT for low-Doppler resolution, the
    /// envelope must show non-trivial variation across a full call.
    ///
    /// At 0.1 Hz the coherence time is on the order of 10 s, so a multi-second
    /// signal is needed for the windows to span more than one fading dwell.
    #[test]
    fn f1_envelope_has_non_trivial_variation() {
        let cfg = WattersonConfig::good_f1(Some(7));
        let mut ch = WattersonChannel::new(cfg).unwrap();

        let n = 80_000usize; // 10 s @ 8 kHz — multiple coherence times of F1
        let signal: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / 8000.0).sin())
            .collect();
        let out = ch.apply(&signal);

        let window = 4000usize; // 0.5 s windows
        let n_windows = n / window;
        let window_rms: Vec<f32> = (0..n_windows)
            .map(|w| {
                let start = w * window;
                let end = start + window;
                (out[start..end].iter().map(|&s| s * s).sum::<f32>() / window as f32).sqrt()
            })
            .collect();
        let mean = window_rms.iter().sum::<f32>() / n_windows as f32;
        let variance =
            window_rms.iter().map(|&r| (r - mean).powi(2)).sum::<f32>() / n_windows as f32;
        let cv = variance.sqrt() / mean;

        assert!(
            cv > 0.10,
            "F1 envelope CV {:.4} should exceed 0.10; the FFT auto-sizing in make_envelope \
             must keep σ_bins clear of the 0.5 floor",
            cv
        );
    }
}
