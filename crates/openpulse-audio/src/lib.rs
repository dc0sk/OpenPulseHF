//! Audio backend implementations for OpenPulse.
//!
//! Two backends are provided:
//!
//! * [`LoopbackBackend`] – pure in-memory loopback, ideal for testing.
//! * [`CpalBackend`] – cross-platform audio via the `cpal` crate (feature
//!   `cpal-backend`, enabled by default).  Supports ALSA, PipeWire, CoreAudio
//!   and WASAPI depending on the platform.

pub mod loopback;

pub mod fault;

#[cfg(feature = "cpal-backend")]
pub mod cpal_backend;

pub use loopback::{LoopbackBackend, LoopbackIqOutputStream};

pub use fault::StreamFault;

#[cfg(feature = "cpal-backend")]
pub use cpal_backend::CpalBackend;

/// Apply soft tanh limiting to `samples` in place.
///
/// Each sample `s` is replaced with `threshold * tanh(s / threshold)`.
/// When `threshold` is 0.0 the function is a no-op (limiter disabled).
/// Peak amplitude after limiting is bounded by `threshold`.
pub fn tanh_limit(samples: &mut [f32], threshold: f32) {
    if threshold <= 0.0 {
        return;
    }
    let inv = 1.0 / threshold;
    for s in samples.iter_mut() {
        *s = threshold * (*s * inv).tanh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tanh_limit_bounds_peak() {
        let mut samples = vec![0.0f32, 0.5, 1.0, 2.0, -2.0, 3.0, -0.1];
        let threshold = 1.0;
        tanh_limit(&mut samples, threshold);
        for s in &samples {
            assert!(
                s.abs() <= threshold,
                "sample {s} exceeds threshold {threshold}"
            );
        }
    }

    #[test]
    fn tanh_limit_zero_threshold_is_noop() {
        let original = vec![0.0f32, 1.5, -2.0];
        let mut samples = original.clone();
        tanh_limit(&mut samples, 0.0);
        assert_eq!(samples, original);
    }

    #[test]
    fn tanh_limit_small_signals_approx_linear() {
        // For |s| << threshold, tanh(s/t)*t ≈ s
        let threshold = 10.0f32;
        let mut samples = vec![0.01f32, -0.01, 0.001];
        let original = samples.clone();
        tanh_limit(&mut samples, threshold);
        for (a, b) in samples.iter().zip(original.iter()) {
            assert!((a - b).abs() < 1e-4, "large distortion on small signal");
        }
    }
}
