//! FIR filter with sample-by-sample and block processing.

use std::collections::VecDeque;

/// Linear-phase FIR filter with an internal state buffer.
pub struct FirFilter {
    coeffs: Vec<f32>,
    state: VecDeque<f32>,
}

impl FirFilter {
    /// Create a filter from the given coefficient vector.
    pub fn new(coeffs: Vec<f32>) -> Self {
        let n = coeffs.len();
        Self {
            coeffs,
            state: VecDeque::from(vec![0.0f32; n]),
        }
    }

    /// Process one sample and return the filtered output.
    #[inline]
    pub fn apply_once(&mut self, sample: f32) -> f32 {
        self.state.push_front(sample);
        self.state.pop_back();
        self.state
            .iter()
            .zip(&self.coeffs)
            .map(|(s, c)| s * c)
            .sum()
    }

    /// Process a block of samples and return an equal-length output vector.
    pub fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        input.iter().map(|&s| self.apply_once(s)).collect()
    }

    /// Number of filter taps.
    pub fn num_taps(&self) -> usize {
        self.coeffs.len()
    }

    /// Group delay in samples (half the filter length, rounded down).
    pub fn group_delay(&self) -> usize {
        (self.coeffs.len() - 1) / 2
    }

    /// Reset the internal state buffer to zero.
    pub fn reset(&mut self) {
        self.state.iter_mut().for_each(|x| *x = 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_impulse_reproduces_coefficients() {
        let coeffs = vec![0.25, 0.5, 0.25];
        let mut f = FirFilter::new(coeffs.clone());
        let impulse: Vec<f32> = std::iter::once(1.0f32)
            .chain(std::iter::repeat_n(0.0, coeffs.len() - 1))
            .collect();
        let out = f.apply(&impulse);
        // The impulse response equals the stored coefficients; for symmetric
        // (linear-phase) filters the reversed order is identical, so either
        // direction passes — but the assertion is against the forward order.
        for (o, c) in out.iter().zip(coeffs.iter()) {
            assert!((o - c).abs() < 1e-6, "got {o}, expected {c}");
        }
    }

    #[test]
    fn dc_gain_is_sum_of_coefficients() {
        let coeffs = vec![0.1, 0.3, 0.4, 0.2]; // sum = 1.0
        let mut f = FirFilter::new(coeffs);
        // Feed a long DC-1 stream and check steady-state output ≈ 1.0
        let dc = vec![1.0f32; 64];
        let out = f.apply(&dc);
        let steady = out[out.len() - 1];
        assert!((steady - 1.0).abs() < 1e-5, "DC gain {steady}");
    }

    #[test]
    fn group_delay_is_half_length_minus_one() {
        let f = FirFilter::new(vec![0.0f32; 513]);
        assert_eq!(f.group_delay(), 256);
        let f2 = FirFilter::new(vec![0.0f32; 64]);
        assert_eq!(f2.group_delay(), 31);
    }

    #[test]
    fn reset_clears_state() {
        let mut f = FirFilter::new(vec![1.0, 0.0]);
        f.apply_once(5.0);
        f.reset();
        // After reset the first sample through a [1,0] filter must be the input itself
        let out = f.apply_once(3.0);
        assert!((out - 3.0).abs() < 1e-6);
    }
}
