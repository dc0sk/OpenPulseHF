//! Controlled-Envelope SSB (CE-SSB) baseband envelope conditioner.
//!
//! After David L. Hershberger W9GR, "Controlled Envelope Single Sideband", QEX
//! Nov/Dec 2014 (public domain). CE-SSB limits the **complex envelope** of a
//! signal so its average power can be raised at a fixed peak (PEP), using a
//! look-ahead "peak stretcher" so the limiting itself does not overshoot.
//!
//! This module is waveform-agnostic: it works on the analytic envelope and emits
//! a per-sample limiting **gain**, which a caller applies either to the complex
//! I/Q ([`condition_iq`]) or directly to the real passband signal the envelope
//! was derived from. Applying the real gain to the passband is the practical
//! "RF-domain" form and avoids any Hilbert reconstruction.

/// Peak-to-average ratio of a real signal, in dB (`20·log10(peak / rms)`).
///
/// This is the headline metric: the average-power gain available at a fixed peak
/// equals the reduction in this value.
pub fn papr_db(signal: &[f32]) -> f32 {
    if signal.is_empty() {
        return 0.0;
    }
    let peak = signal.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
    let rms = (signal.iter().map(|x| x * x).sum::<f32>() / signal.len() as f32).sqrt();
    if rms <= f32::MIN_POSITIVE || peak <= 0.0 {
        return 0.0;
    }
    20.0 * (peak / rms).log10()
}

/// Complex envelope `|i + jq|`.
pub fn envelope(i: &[f32], q: &[f32]) -> Vec<f32> {
    i.iter()
        .zip(q.iter())
        .map(|(&a, &b)| (a * a + b * b).sqrt())
        .collect()
}

/// Hard envelope-clip gain (no look-ahead): the naive limiter — `g = level/|z|`
/// where the envelope exceeds `level`, else 1. Leaves post-clip overshoot when the
/// signal is later band-limited; provided as the baseline to compare CE-SSB against.
pub fn magnitude_clip_gain(env: &[f32], level: f32) -> Vec<f32> {
    env.iter()
        .map(|&e| if e > level { level / e } else { 1.0 })
        .collect()
}

/// CE-SSB peak-stretch limiting gain: divide by the **windowed-max** envelope over
/// `±lookahead` samples, so peaks are anticipated and the gain ramps down *before*
/// a peak. This is the mechanism that lets CE-SSB limit the envelope without the
/// overshoot a memoryless clip leaves behind. Returns a per-sample gain in `(0, 1]`.
pub fn peak_stretch_gain(env: &[f32], level: f32, lookahead: usize) -> Vec<f32> {
    let n = env.len();
    (0..n)
        .map(|k| {
            let lo = k.saturating_sub(lookahead);
            let hi = (k + lookahead + 1).min(n);
            let peak = env[lo..hi].iter().copied().fold(0.0f32, f32::max);
            if peak > level {
                level / peak
            } else {
                1.0
            }
        })
        .collect()
}

/// Configuration for [`condition_iq`].
#[derive(Debug, Clone, Copy)]
pub struct CessbConfig {
    /// Envelope clip level (absolute). Set relative to the signal's RMS envelope —
    /// a smaller level clips harder (more average-power gain, more distortion).
    pub level: f32,
    /// Look-ahead/behind window (samples) for the peak stretcher.
    pub lookahead: usize,
}

/// Apply CE-SSB peak-stretch envelope limiting to a complex I/Q signal.
pub fn condition_iq(i: &[f32], q: &[f32], cfg: &CessbConfig) -> (Vec<f32>, Vec<f32>) {
    let env = envelope(i, q);
    let gain = peak_stretch_gain(&env, cfg.level, cfg.lookahead);
    let out_i = i.iter().zip(&gain).map(|(&v, &g)| v * g).collect();
    let out_q = q.iter().zip(&gain).map(|(&v, &g)| v * g).collect();
    (out_i, out_q)
}

/// Multiply a real passband signal by a per-sample gain (e.g. from
/// [`peak_stretch_gain`] computed on the analytic envelope) — the RF-domain form.
pub fn apply_gain(signal: &[f32], gain: &[f32]) -> Vec<f32> {
    signal
        .iter()
        .zip(gain.iter())
        .map(|(&s, &g)| s * g)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_stretch_bounds_envelope_to_level() {
        // A spike well above `level` must be brought down to (at most) `level`.
        let i = vec![0.2, 0.2, 3.0, 0.2, 0.2];
        let q = vec![0.0; 5];
        let cfg = CessbConfig {
            level: 1.0,
            lookahead: 1,
        };
        let (ci, cq) = condition_iq(&i, &q, &cfg);
        let out_env = envelope(&ci, &cq);
        for e in out_env {
            assert!(e <= 1.0 + 1e-5, "envelope {e} exceeds the clip level");
        }
    }

    #[test]
    fn lookahead_pulls_gain_down_before_the_peak() {
        // With look-ahead, the sample BEFORE the spike is already attenuated
        // (anticipated), unlike a memoryless clip which leaves it untouched.
        let env = vec![1.0, 1.0, 4.0, 1.0];
        let stretched = peak_stretch_gain(&env, 1.0, 1);
        let clipped = magnitude_clip_gain(&env, 1.0);
        assert!(
            stretched[1] < 1.0,
            "look-ahead must attenuate before the peak"
        );
        assert_eq!(
            clipped[1], 1.0,
            "memoryless clip leaves the pre-peak sample"
        );
    }

    #[test]
    fn no_op_when_level_above_signal() {
        let i = vec![0.1, -0.2, 0.3];
        let q = vec![0.0, 0.1, -0.1];
        let cfg = CessbConfig {
            level: 100.0,
            lookahead: 4,
        };
        let (ci, cq) = condition_iq(&i, &q, &cfg);
        assert_eq!(ci, i);
        assert_eq!(cq, q);
    }

    #[test]
    fn papr_of_sine_is_about_3db() {
        let n = 8000;
        let s: Vec<f32> = (0..n)
            .map(|k| (2.0 * std::f32::consts::PI * 1000.0 / 8000.0 * k as f32).sin())
            .collect();
        // A pure sine has peak/rms = sqrt(2) ≈ 3.01 dB.
        assert!((papr_db(&s) - 3.01).abs() < 0.1, "got {} dB", papr_db(&s));
    }
}
