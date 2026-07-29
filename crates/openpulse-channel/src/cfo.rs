//! Carrier-frequency-offset (rig dial mismatch) channel.
//!
//! Two independent transceivers never agree exactly. At 144 MHz a 0.5 ppm crystal difference is
//! ~72 Hz, and the receiver hears the whole audio band shifted by that much: a tone transmitted at
//! 1500 Hz arrives at 1500 + Δf. Measured on air 2026-07-28 between an IC-9700 and an FT-991A both
//! commanded to 144.600000 MHz: the received carrier sat at **1436 Hz, a −64 Hz offset**, right at
//! the edge of BPSK250's ±62.5 Hz AFC range.
//!
//! **Why this channel exists.** Carrier recovery and AFC are the single most-churned area of this
//! modem (see the DSP playbook in `CLAUDE.md`), yet every offset bug so far had to be found on
//! hardware, because nothing in-process could give the acquisition path a real frequency offset.
//! [`SroChannel`](crate::sro::SroChannel) models a *sampling-clock* difference, which is a different
//! impairment: it drifts the symbol clock and leaves the carrier where it was.
//!
//! This is a pure multiplicative/spectral impairment: [`generate_noise`](CfoChannel::generate_noise)
//! returns zeros.
//!
//! **Method.** The real input is converted to its analytic signal (FFT, zero the negative
//! frequencies, double the positive ones, inverse FFT), rotated by `exp(j2π·Δf·t)`, and the real
//! part is returned. A phase accumulator carries across `apply` calls so a block-wise application
//! stays phase-continuous. The analytic step is computed per block, so very short blocks see edge
//! effects; apply it to a whole capture (as the test harness does) for an exact shift.

use rustfft::{num_complex::Complex, FftPlanner};

use crate::{ChannelError, ChannelModel};

type Complex32 = Complex<f32>;

/// Carrier-frequency-offset configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct CfoConfig {
    /// Frequency shift applied to the received audio, Hz. Positive moves the signal UP the audio
    /// band (receiver tuned low relative to the transmitter).
    pub offset_hz: f32,
    /// Sample rate of the audio being shifted, Hz.
    pub sample_rate: f32,
}

impl CfoConfig {
    pub fn new(offset_hz: f32, sample_rate: f32) -> Self {
        Self {
            offset_hz,
            sample_rate,
        }
    }
}

/// Shifts the whole audio band by a fixed frequency offset.
pub struct CfoChannel {
    offset_hz: f64,
    sample_rate: f64,
    /// Running phase so successive blocks join without a discontinuity.
    phase: f64,
}

impl CfoChannel {
    pub fn new(config: CfoConfig) -> Result<Self, ChannelError> {
        if !config.offset_hz.is_finite() {
            return Err(ChannelError::InvalidParameter(
                "offset_hz must be finite".into(),
            ));
        }
        if !(config.sample_rate.is_finite() && config.sample_rate > 0.0) {
            return Err(ChannelError::InvalidParameter(
                "sample_rate must be finite and positive".into(),
            ));
        }
        Ok(Self {
            offset_hz: config.offset_hz as f64,
            sample_rate: config.sample_rate as f64,
            phase: 0.0,
        })
    }

    /// Analytic (complex) signal of a real block: zero the negative frequencies, double the
    /// positive ones. This is what lets the band be rotated without producing a mirror image.
    fn analytic(input: &[f32]) -> Vec<Complex32> {
        let n = input.len();
        let mut planner = FftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(n);
        let inv = planner.plan_fft_inverse(n);

        let mut buf: Vec<Complex32> = input.iter().map(|&s| Complex32::new(s, 0.0)).collect();
        fwd.process(&mut buf);

        // Hilbert weighting: DC and (for even n) Nyquist keep their value, positive frequencies
        // double, negative frequencies vanish.
        let half = n / 2;
        for (k, c) in buf.iter_mut().enumerate() {
            if k == 0 || (n.is_multiple_of(2) && k == half) {
                // unchanged
            } else if k < half || (!n.is_multiple_of(2) && k <= half) {
                *c *= 2.0;
            } else {
                *c = Complex32::new(0.0, 0.0);
            }
        }

        inv.process(&mut buf);
        let scale = 1.0 / n as f32;
        for c in buf.iter_mut() {
            *c *= scale;
        }
        buf
    }
}

impl ChannelModel for CfoChannel {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() || self.offset_hz == 0.0 {
            return input.to_vec();
        }
        let analytic = Self::analytic(input);
        let dphi = std::f64::consts::TAU * self.offset_hz / self.sample_rate;
        let mut out = Vec::with_capacity(input.len());
        for c in analytic {
            let (s, co) = self.phase.sin_cos();
            // Re{ z · e^{jθ} } = re·cos θ − im·sin θ
            out.push((c.re as f64 * co - c.im as f64 * s) as f32);
            self.phase += dphi;
            if self.phase > std::f64::consts::TAU {
                self.phase -= std::f64::consts::TAU;
            } else if self.phase < -std::f64::consts::TAU {
                self.phase += std::f64::consts::TAU;
            }
        }
        out
    }

    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        vec![0.0; length]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Measure the dominant frequency of a real block by parabolic-interpolated FFT peak.
    fn peak_hz(x: &[f32], fs: f32) -> f32 {
        let n = x.len();
        let mut planner = FftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(n);
        // Hann window keeps the peak clean enough for sub-bin interpolation.
        let mut buf: Vec<Complex32> = x
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (n as f32 - 1.0)).cos();
                Complex32::new(s * w, 0.0)
            })
            .collect();
        fwd.process(&mut buf);
        let half = n / 2;
        let mag: Vec<f32> = buf[..half].iter().map(|c| c.norm()).collect();
        let (mut k, mut best) = (1usize, 0.0f32);
        for (i, &m) in mag.iter().enumerate().take(half - 1).skip(1) {
            if m > best {
                best = m;
                k = i;
            }
        }
        let (a, b, c) = (mag[k - 1], mag[k], mag[k + 1]);
        let denom = a - 2.0 * b + c;
        let delta = if denom != 0.0 {
            0.5 * (a - c) / denom
        } else {
            0.0
        };
        (k as f32 + delta) * fs / n as f32
    }

    fn tone(freq: f32, fs: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (std::f32::consts::TAU * freq * i as f32 / fs).sin())
            .collect()
    }

    /// THE self-validation: the channel must move the spectrum by exactly the requested amount.
    /// A CFO channel that silently did nothing would let every acquisition test "pass".
    #[test]
    fn a_tone_moves_by_exactly_the_requested_offset() {
        let fs = 8000.0;
        let n = 8192;
        for offset in [-200.0f32, -64.0, 25.0, 150.0] {
            let x = tone(1500.0, fs, n);
            let before = peak_hz(&x, fs);
            let mut ch = CfoChannel::new(CfoConfig::new(offset, fs)).unwrap();
            let y = ch.apply(&x);
            let after = peak_hz(&y, fs);
            let measured = after - before;
            assert!(
                (measured - offset).abs() < 2.0,
                "requested {offset} Hz shift but measured {measured:.1} Hz \
                 ({before:.1} -> {after:.1})"
            );
        }
    }

    /// A zero offset must be a true passthrough, so a test that uses this channel at 0 Hz is
    /// measuring the mode and not the channel.
    #[test]
    fn zero_offset_is_a_passthrough() {
        let x = tone(1500.0, 8000.0, 1024);
        let mut ch = CfoChannel::new(CfoConfig::new(0.0, 8000.0)).unwrap();
        assert_eq!(ch.apply(&x), x);
    }

    /// The shift must not cost amplitude: a decode failure under CFO has to be attributable to the
    /// frequency error, not to the channel quietly attenuating the signal.
    #[test]
    fn the_shift_preserves_signal_power() {
        let fs = 8000.0;
        let x = tone(1200.0, fs, 4096);
        let mut ch = CfoChannel::new(CfoConfig::new(80.0, fs)).unwrap();
        let y = ch.apply(&x);
        let p_in: f32 = x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32;
        let p_out: f32 = y.iter().map(|s| s * s).sum::<f32>() / y.len() as f32;
        let ratio = p_out / p_in;
        assert!(
            (0.9..=1.1).contains(&ratio),
            "power ratio {ratio:.3} — the shift must preserve amplitude"
        );
    }

    #[test]
    fn generate_noise_is_silent() {
        let mut ch = CfoChannel::new(CfoConfig::new(50.0, 8000.0)).unwrap();
        assert!(ch.generate_noise(64).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn invalid_parameters_are_rejected() {
        assert!(CfoChannel::new(CfoConfig::new(f32::NAN, 8000.0)).is_err());
        assert!(CfoChannel::new(CfoConfig::new(10.0, 0.0)).is_err());
    }
}
