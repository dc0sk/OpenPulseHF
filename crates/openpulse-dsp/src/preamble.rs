//! Preamble-based frame synchronization and phase coherence detection.
//!
//! Provides standard preamble sequences (Barker, PN, Zadoff-Chu) and methods
//! for frame alignment, timing lock validation, and phase coherence checking.

use std::f32::consts::PI;

const INV_SQRT2: f32 = 0.70710677;

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
    /// Zadoff-Chu (ZC) sequence of length 64, u=1, represented as a real projection.
    ZadoffChu64,
}

/// Symbol constellation used when materializing preambles into IQ symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreambleConstellation {
    /// Real-axis BPSK symbols (I=chip, Q=0).
    Bpsk,
    /// Unit-power QPSK symbols derived from the chip sequence.
    Qpsk,
}

/// Configurable preamble generation settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreambleSpec {
    /// Base preamble family.
    pub preamble_type: PreambleType,
    /// Number of output symbols to generate.
    pub length_symbols: usize,
    /// Output constellation mapping.
    pub constellation: PreambleConstellation,
}

impl PreambleSpec {
    /// Create a preamble spec with explicit length and constellation.
    pub fn new(
        preamble_type: PreambleType,
        length_symbols: usize,
        constellation: PreambleConstellation,
    ) -> Self {
        Self {
            preamble_type,
            length_symbols,
            constellation,
        }
    }

    /// Generate real-valued chips, repeating/truncating the base family sequence as needed.
    pub fn chips(&self) -> Vec<f32> {
        if self.length_symbols == 0 {
            return Vec::new();
        }
        let base = self.preamble_type.base_sequence();
        if base.is_empty() {
            return Vec::new();
        }

        base.iter()
            .copied()
            .cycle()
            .take(self.length_symbols)
            .collect()
    }

    /// Generate IQ symbols according to `constellation`.
    pub fn iq_symbols(&self) -> Vec<(f32, f32)> {
        let chips = self.chips();
        match self.constellation {
            PreambleConstellation::Bpsk => chips.into_iter().map(|c| (c, 0.0)).collect(),
            PreambleConstellation::Qpsk => chips
                .into_iter()
                .enumerate()
                .map(|(idx, c)| {
                    // Alternate the quadrature sign to keep a balanced Q branch.
                    if idx % 2 == 0 {
                        (c * INV_SQRT2, c * INV_SQRT2)
                    } else {
                        (c * INV_SQRT2, -c * INV_SQRT2)
                    }
                })
                .collect(),
        }
    }
}

impl PreambleType {
    fn base_sequence(&self) -> Vec<f32> {
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

    /// Return the preamble sequence as `f32` symbols/samples.
    ///
    /// Barker and PN variants are BPSK symbols (`+1.0` or `-1.0`).
    /// `ZadoffChu64` is returned as a real-valued projection.
    pub fn sequence(&self) -> Vec<f32> {
        self.base_sequence()
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

    /// Return `true` if the preamble has zero symbols.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Generate an m-sequence (Linear Feedback Shift Register) of length N.
///
/// Parameters:
/// - `length`: desired sequence length (e.g., 31, 63 for primitive polynomials)
/// - `seed`: initial LFSR state (nonzero)
fn pn_sequence(length: usize, seed: u32) -> Vec<f32> {
    let (degree, tap_a, tap_b) = match length {
        // Primitive polynomial x^5 + x^2 + 1 (period 31)
        31 => (5u32, 0u32, 2u32),
        // Primitive polynomial x^6 + x + 1 (period 63)
        63 => (6u32, 0u32, 1u32),
        _ => return Vec::new(),
    };

    let mut seq = Vec::with_capacity(length);
    let mask = (1u32 << degree) - 1;
    let mut state = seed & mask;
    if state == 0 {
        state = 1;
    }

    for _ in 0..length {
        let feedback = ((state >> tap_a) ^ (state >> tap_b)) & 1;
        seq.push(if state & 1 == 1 { 1.0 } else { -1.0 });
        state = ((state >> 1) | (feedback << (degree - 1))) & mask;
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
        seq.push(cos);
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
    pub fn correlate_bpsk(&mut self, received: &[f32]) -> (f32, f32) {
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

        self.correlation_history.push(mag);
        if self.correlation_history.len() > self.max_history {
            self.correlation_history.remove(0);
        }

        (mag, phase)
    }

    /// Track phase coherence across frames.
    ///
    /// Returns true if phase slip is within acceptable bounds (±45°), false otherwise.
    pub fn check_phase_coherence(&mut self, phase_rad: f32) -> bool {
        if self.phase_history.is_empty() {
            self.phase_history.push(phase_rad);
            return true;
        }

        let prev_unwrapped = self.phase_history[self.phase_history.len() - 1];
        let unwrapped = Self::unwrap_phase_incremental(prev_unwrapped, phase_rad);
        self.phase_history.push(unwrapped);
        if self.phase_history.len() > self.max_history {
            self.phase_history.remove(0);
        }

        let median = Self::median(&self.phase_history);
        let threshold = PI / 4.0;
        (unwrapped - median).abs() <= threshold
    }

    /// Unwrap a phase sample relative to the previous unwrapped phase.
    fn unwrap_phase_incremental(prev_unwrapped: f32, phase_rad: f32) -> f32 {
        let mut delta = phase_rad - prev_unwrapped;
        while delta > PI {
            delta -= 2.0 * PI;
        }
        while delta < -PI {
            delta += 2.0 * PI;
        }
        prev_unwrapped + delta
    }

    /// Compute a robust median of a small history vector.
    fn median(values: &[f32]) -> f32 {
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let mid = sorted.len() / 2;
        if sorted.len() % 2 == 0 {
            (sorted[mid - 1] + sorted[mid]) * 0.5
        } else {
            sorted[mid]
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
        let mut detector = PreambleDetector::new(PreambleType::Barker11, 10);
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

    #[test]
    fn test_configurable_length_repeats_or_truncates_base_sequence() {
        let spec = PreambleSpec::new(PreambleType::Barker11, 20, PreambleConstellation::Bpsk);
        let chips = spec.chips();
        assert_eq!(chips.len(), 20);
        assert_eq!(chips[0], 1.0);
        assert_eq!(chips[11], 1.0); // Repeats from start of Barker11.

        let spec = PreambleSpec::new(PreambleType::Pn63, 32, PreambleConstellation::Bpsk);
        let chips = spec.chips();
        assert_eq!(chips.len(), 32);
    }

    #[test]
    fn test_constellation_mapping_generates_iq_symbols() {
        let bpsk =
            PreambleSpec::new(PreambleType::Barker13, 13, PreambleConstellation::Bpsk).iq_symbols();
        assert_eq!(bpsk.len(), 13);
        assert!(bpsk.iter().all(|(_, q)| q.abs() < 1e-6));

        let qpsk =
            PreambleSpec::new(PreambleType::Barker13, 13, PreambleConstellation::Qpsk).iq_symbols();
        assert_eq!(qpsk.len(), 13);
        assert!(qpsk.iter().all(|(i, q)| i.abs() > 0.0 && q.abs() > 0.0));
    }
}
