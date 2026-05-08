//! Hilbert-transform baseband I/Q extraction.
//!
//! Used as the fallback default implementation of
//! [`ModulationPlugin::modulate_iq`] for plugins that do not provide a
//! native I/Q path.  BPSK and QPSK plugins override the trait method and
//! bypass this entirely.

use std::f32::consts::PI;

/// Convert a real bandpass signal to baseband I/Q via FIR Hilbert transform.
///
/// `real` is a real-valued signal centred at `fc` Hz, sampled at `fs` Hz.
/// Returns `(i_bb, q_bb)` — the complex baseband envelope at the same sample
/// rate.  A 63-tap Hann-windowed FIR is used; group delay is 31 samples.
///
/// Only the middle portion of the output is free of edge artefacts; the first
/// and last ~31 samples carry window roll-on/off errors.
pub fn hilbert_iq(real: &[f32], fc: f32, fs: f32) -> (Vec<f32>, Vec<f32>) {
    const ORDER: usize = 62; // filter order (even → HALF = 31)
    const HALF: i32 = 31;

    // Build Hann-windowed Hilbert FIR kernel.
    let kernel: Vec<f32> = (0..=(ORDER as i32))
        .map(|k| {
            let n = k - HALF;
            if n == 0 || n % 2 == 0 {
                0.0
            } else {
                // Symmetric Hann window: 0 at edges, 1 at centre (k=HALF).
                let w = 0.5 * (1.0 - (2.0 * PI * k as f32 / ORDER as f32).cos());
                (2.0 / (PI * n as f32)) * w
            }
        })
        .collect();

    let len = real.len();
    let mut q_rf = vec![0.0f32; len];

    // Direct-form FIR convolution with zero boundary padding.
    for i in 0..len {
        let mut sum = 0.0f32;
        for (j, &h) in kernel.iter().enumerate() {
            let src = i as i32 - j as i32 + HALF;
            if src >= 0 && (src as usize) < len {
                sum += real[src as usize] * h;
            }
        }
        q_rf[i] = sum;
    }

    // Demodulate to baseband: multiply by e^(-j·2π·fc·k/fs).
    let omega = 2.0 * PI * fc / fs;
    let i_bb: Vec<f32> = (0..len)
        .map(|k| {
            let c = (omega * k as f32).cos();
            let s = (omega * k as f32).sin();
            real[k] * c + q_rf[k] * s
        })
        .collect();
    let q_bb: Vec<f32> = (0..len)
        .map(|k| {
            let c = (omega * k as f32).cos();
            let s = (omega * k as f32).sin();
            q_rf[k] * c - real[k] * s
        })
        .collect();

    (i_bb, q_bb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_cosine_demodulates_to_dc() {
        let fs = 8000.0f32;
        let fc = 1500.0f32;
        let n = 400;
        let real: Vec<f32> = (0..n)
            .map(|k| (2.0 * PI * fc / fs * k as f32).cos())
            .collect();
        let (i_bb, q_bb) = hilbert_iq(&real, fc, fs);

        // Middle portion avoids group-delay edge effects (±31 samples).
        let mid = 80..320;
        let i_mean: f32 = i_bb[mid.clone()].iter().sum::<f32>() / (mid.len() as f32);
        let q_rms: f32 =
            (q_bb[mid.clone()].iter().map(|x| x * x).sum::<f32>() / mid.len() as f32).sqrt();

        // I should be near 1.0 (the carrier amplitude); Q should be near zero.
        assert!(
            (i_mean - 1.0).abs() < 0.15,
            "expected I mean ≈ 1.0, got {i_mean:.4}"
        );
        assert!(q_rms < 0.15, "expected Q RMS ≈ 0, got {q_rms:.4}");
    }
}
