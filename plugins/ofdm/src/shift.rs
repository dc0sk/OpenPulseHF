//! Frequency translation of a real passband signal, so the OFDM receiver can run at its fixed centre
//! (`docs/dev/design/ofdm-carrier-offset.md`).

use num_complex::Complex32;
use rustfft::FftPlanner;

/// `samples` translated by `hz`: the analytic signal (FFT, negative frequencies removed) times
/// `e^{j·2π·hz·t}`, real part. Exact for any `hz`, with no image, unlike an FIR Hilbert pair.
pub fn shift_hz(samples: &[f32], hz: f32, fs: f32) -> Vec<f32> {
    if samples.len() < 2 || hz == 0.0 {
        return samples.to_vec();
    }
    rotate_real(&analytic(samples), hz, fs)
}

/// The analytic signal of `samples`: DC once, positive frequencies doubled, negative ones removed.
/// Computed at the next power of two (zero-padded, then cut back), which is several times faster
/// than an arbitrary length and wraps less of the frame's edge around the buffer.
pub fn analytic(samples: &[f32]) -> Vec<Complex32> {
    let n = samples.len();
    let m = n.next_power_of_two().max(2);
    let mut planner = FftPlanner::<f32>::new();
    let mut buf: Vec<Complex32> = samples
        .iter()
        .map(|&s| Complex32::new(s, 0.0))
        .chain(std::iter::repeat_n(Complex32::new(0.0, 0.0), m - n))
        .collect();
    planner.plan_fft_forward(m).process(&mut buf);
    // DC and Nyquist once, positive bins doubled, negative bins zero (m is even).
    for b in buf.iter_mut().take(m / 2).skip(1) {
        *b *= 2.0;
    }
    for b in buf.iter_mut().skip(m / 2 + 1) {
        *b = Complex32::new(0.0, 0.0);
    }
    planner.plan_fft_inverse(m).process(&mut buf);
    let scale = 1.0 / m as f32;
    buf.truncate(n);
    for z in buf.iter_mut() {
        *z *= scale;
    }
    buf
}

/// The real part of `analytic` turned by `e^{j·2π·hz·t}`: one candidate offset in O(n), so a search
/// over offsets pays for the FFT pair once.
pub fn rotate_real(analytic: &[Complex32], hz: f32, fs: f32) -> Vec<f32> {
    let w = 2.0 * std::f64::consts::PI * f64::from(hz) / f64::from(fs);
    analytic
        .iter()
        .enumerate()
        .map(|(t, z)| {
            // f64 phase: an f32 product loses the phase over a 100 000-sample frame.
            let ph = (w * t as f64).rem_euclid(2.0 * std::f64::consts::PI);
            (z * Complex32::new(ph.cos() as f32, ph.sin() as f32)).re
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(f: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|t| (2.0 * std::f32::consts::PI * f * t as f32 / 8000.0).sin())
            .collect()
    }

    /// The tone's frequency by a fine DFT scan around `near`.
    fn peak_hz(x: &[f32], near: f32) -> f32 {
        let mut best = (0.0f32, near);
        let mut f = near - 40.0;
        while f <= near + 40.0 {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (t, &s) in x.iter().enumerate() {
                let ph = 2.0 * std::f64::consts::PI * f64::from(f) * t as f64 / 8000.0;
                re += f64::from(s) * ph.cos();
                im -= f64::from(s) * ph.sin();
            }
            let p = (re * re + im * im) as f32;
            if p > best.0 {
                best = (p, f);
            }
            f += 0.25;
        }
        best.1
    }

    #[test]
    fn a_tone_moves_by_the_shift_and_keeps_its_level() {
        for (n, hz) in [(8000usize, 17.5f32), (8001, -23.25), (4096, 0.75)] {
            let x = tone(1500.0, n);
            let y = shift_hz(&x, hz, 8000.0);
            assert_eq!(y.len(), n);
            let got = peak_hz(&y[200..n - 200], 1500.0 + hz);
            assert!(
                (got - (1500.0 + hz)).abs() <= 0.25,
                "n {n}, shift {hz}: tone at {got}"
            );
            let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
            let (a, b) = (rms(&x[200..n - 200]), rms(&y[200..n - 200]));
            assert!((a - b).abs() / a < 0.01, "level {a} → {b}");
        }
    }

    #[test]
    fn a_zero_shift_is_the_identity() {
        let x = tone(1234.0, 999);
        assert_eq!(shift_hz(&x, 0.0, 8000.0), x);
    }
}
