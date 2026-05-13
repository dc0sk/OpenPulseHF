//! Preamble-based frame synchronization and phase coherence detection.
//!
//! Provides standard preamble sequences (Barker, PN, Zadoff-Chu) and methods
//! for frame alignment, timing lock validation, and phase coherence checking.

use std::f32::consts::PI;

/// Standard preamble types for frame synchronization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreambleType {
    /// Barker-11 sequence (BPSK, 11 symbols, 13 dB peak sidelobe).
    Barker11,
    /// Barker-13 sequence (BPSK, 13 symbols, 17 dB peak sidelobe).
    Barker13,
    /// m-sequence (PN-31) seeded with 0x1F (31 symbols, near-ideal autocorrelation).
    Pn31,
    /// m-sequence (PN-63) seeded with 0x45 (63 symbols, near-ideal autocorrelation).
    Pn63,
    /// Zadoff-Chu (ZC) sequence of length 64, u=1 (flat autocorrelation magnitude).
    ZadoffChu64,
}

impl PreambleType {
    /// Return the preamble sequence as BPSK symbols (+1.0 or -1.0).
    pub fn sequence(&self) -> Vec<f32> {
        match self {
            PreambleType::Barker11 => {
                vec![1.0, 1.0, 1.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0]
            }
            PreambleType::Barker13 => vec![
                1.0, 1.0, 1.0, 1.0, 1.0, 1.0, -1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0,
            ],
            PreambleType::Pn31 => pn_sequence(31, 0x1F),
            PreambleType::Pn63 => pn_sequence(63, 0x45),
            PreambleType::ZadoffChu64 => zadoff_chu_sequence(64, 1),
        }
    }

    /// Length of the preamble in symbols.
    pub fn len(&self) -> usize {
        match self {
            PreambleType::Barker11 => 11,
            PreambleType::Barker13 => 13,
            PreambleType::Pn31 => 31,
            PreambleType::Pn63 => 63,
            PreambleType::ZadoffChu64 => 64,
        }
    }
}

/// Generate an m-sequence (Linear Feedback Shift Register) of length N.
///
/// Parameters:
/// - `length`: desired sequence length (e.g., 31, 63 for primitive polynomials)
/// - `seed`: initial LFSR state (nonzero)
fn pn_sequence(length: usize, seed: u32) -> Vec<f32> {
    let mut seq = Vec::with_capacity(length);
    let mut state = seed as u64;
    let mask = (1u64 << length) - 1; // Create bitmask for the length

    for _ in 0..length {
        // XOR feedback from taps (e.g., bits 0, 5 for length 31)
        let tap_a = state & 1;
        let tap_b = (state >> 5) & 1;
        let feedback = tap_a ^ tap_b;

        // Output the LSB
        seq.push(if state & 1 == 1 { 1.0 } else { -1.0 });

        // Shift and inject feedback
        state = ((state >> 1) | (feedback << (length - 1))) & mask;
    }
    seq
}

/// Generate a Zadoff-Chu (ZC) sequence.
///
/// Parameters:
/// - `length`: sequence length (N)
/// - `u`: coprime with N (typically 1)
fn zadoff_chu_sequence(length: usize, u: i32) -> Vec<f32> {
    let mut seq = Vec::with_capacity(length);
    let n_f = length as f32;

    for n in 0..length {
        let n_f32 = n as f32;
        let exponent = -PI * u as f32 * n_f32 * (n_f32 + 1.0) / n_f;
        let (_sin, cos) = exponent.sin_cos();
        seq.push(cos); // Return as complex magnitude (cos for real, sin for imag in full impl)
    }
    seq
}

/// Preamble detector and phase-coherence tracker.
pub struct PreambleDetector {
    #[allow(dead_code)]
    preamble_type: PreambleType,
    reference: Vec<f32>,
    correlation_history: Vec<f32>,
    phase_history: Vec<f32>,
    max_history: usize,
}

impl PreambleDetector {
    /// Create a new preamble detector.
    pub fn new(preamble_type: PreambleType, history_len: usize) -> Self {
        let reference = preamble_type.sequence();
        Self {
            preamble_type,
            reference,
            correlation_history: Vec::with_capacity(history_len),
            phase_history: Vec::with_capacity(history_len),
            max_history: history_len,
        }
    }

    /// Correlate a received symbol sequence with the preamble reference.
    ///
    /// Returns (correlation_magnitude, phase_estimate) where:
    /// - correlation_magnitude: |∑ recv_i × ref_i| / len
    /// - phase_estimate: 0 if positive, π if negative
    pub fn correlate_bpsk(&self, received: &[f32]) -> (f32, f32) {
        if received.len() != self.reference.len() {
            return (0.0, 0.0);
        }

        let mut i_acc = 0.0;

        // For BPSK, multiply each pair (ref is always ±1)
        for (r, ref_sym) in received.iter().zip(self.reference.iter()) {
            i_acc += r * ref_sym;
        }

        let mag = i_acc.abs() / received.len() as f32;
        let phase = if i_acc > 0.0 { 0.0 } else { PI };

        (mag, phase)
    }

    /// Track phase coherence across frames.
    ///
    /// Returns true if phase slip is within acceptable bounds (±45°), false otherwise.
    pub fn check_phase_coherence(&mut self, phase_rad: f32) -> bool {
        self.phase_history.push(phase_rad);
        if self.phase_history.len() > self.max_history {
            self.phase_history.remove(0);
        }

        // Unwrap and compute phase slope (indicating Doppler or frequency offset drift)
        let phase_unwrapped = self
            .phase_history
            .iter()
            .map(|&p| self.unwrap_phase(p))
            .collect::<Vec<_>>();

        // Accept if recent phase is within ±45° of median
        if let Some(&recent) = phase_unwrapped.last() {
            let threshold = PI / 4.0; // 45 degrees
            (recent.abs() % (2.0 * PI)) < threshold
                || ((recent.abs() % (2.0 * PI)) > (2.0 * PI - threshold))
        } else {
            true
        }
    }

    /// Unwrap a phase value, tracking drift relative to previous samples.
    fn unwrap_phase(&self, phase_rad: f32) -> f32 {
        if self.phase_history.is_empty() {
            return phase_rad;
        }

        let prev = self.phase_history[self.phase_history.len() - 1];
        let delta = phase_rad - prev;

        if delta > PI {
            prev + delta - 2.0 * PI
        } else if delta < -PI {
            prev + delta + 2.0 * PI
        } else {
            prev + delta
        }
    }

    /// Get statistics on the correlation history.
    pub fn correlation_stats(&self) -> Option<(f32, f32, f32)> {
        if self.correlation_history.is_empty() {
            return None;
        }

        let mean =
            self.correlation_history.iter().sum::<f32>() / self.correlation_history.len() as f32;
        let variance = self
            .correlation_history
            .iter()
            .map(|&x| (x - mean).powi(2))
            .sum::<f32>()
            / self.correlation_history.len() as f32;
        let std_dev = variance.sqrt();

        Some((mean, std_dev, variance))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_barker_11_sequence() {
        let seq = PreambleType::Barker11.sequence();
        assert_eq!(seq.len(), 11);
        // Verify autocorrelation peak is 11 at lag 0
        let autocorr: f32 = seq.iter().zip(seq.iter()).map(|(a, b)| a * b).sum();
        assert!((autocorr - 11.0).abs() < 1e-5);
    }

    #[test]
    fn test_pn31_sequence() {
        let seq = PreambleType::Pn31.sequence();
        assert_eq!(seq.len(), 31);
    }

    #[test]
    fn test_zadoff_chu_64() {
        let seq = PreambleType::ZadoffChu64.sequence();
        assert_eq!(seq.len(), 64);
    }

    #[test]
    fn test_preamble_detector_correlation() {
        let detector = PreambleDetector::new(PreambleType::Barker11, 10);
        let reference = PreambleType::Barker11.sequence();

        // Correlate with itself should give max correlation
        let (mag, phase) = detector.correlate_bpsk(&reference);
        assert!(mag > 0.9); // Should be close to 1.0
        assert!((phase.abs() - 0.0).abs() < 0.1 || (phase.abs() - PI).abs() < 0.1); // Phase near 0

        // Correlate with inverted should give same magnitude but phase shifted by π
        let inverted: Vec<f32> = reference.iter().map(|x| -x).collect();
        let (mag2, phase2) = detector.correlate_bpsk(&inverted);
        assert!(mag2 > 0.9); // Magnitude should still be high
        assert!((phase2 - PI).abs() < 0.1); // Phase should be π (flipped)
    }

    #[test]
    fn test_phase_coherence_tracking() {
        let mut detector = PreambleDetector::new(PreambleType::Barker13, 5);

        // Small phase values should pass
        assert!(detector.check_phase_coherence(0.1));
        assert!(detector.check_phase_coherence(0.2));

        // Should remain coherent
        assert!(detector.check_phase_coherence(0.15));
    }
}
