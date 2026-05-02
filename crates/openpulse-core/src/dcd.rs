/// Data Carrier Detect state machine.
///
/// Computes RMS energy over each batch of received samples.  When energy
/// exceeds `threshold` the channel is marked busy and that state is held for
/// `hold_samples` additional samples after the last detection.
#[derive(Debug, Clone)]
pub struct DcdState {
    threshold: f32,
    hold_samples: usize,
    sample_clock: usize,
    last_busy_at: Option<usize>,
    current_energy: f32,
}

impl DcdState {
    /// Create a DCD detector.
    ///
    /// `threshold` is the RMS amplitude (0.0–1.0) above which the channel is
    /// busy.  `hold_samples` controls how long the busy flag persists after
    /// the last energy detection (e.g. `sample_rate * hold_ms / 1000`).
    pub fn new(threshold: f32, hold_samples: usize) -> Self {
        Self {
            threshold,
            hold_samples,
            sample_clock: 0,
            last_busy_at: None,
            current_energy: 0.0,
        }
    }

    /// Update DCD state from a batch of received samples.
    pub fn update(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let mean_sq: f32 = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
        self.current_energy = mean_sq.sqrt();
        self.sample_clock += samples.len();
        if self.current_energy >= self.threshold {
            self.last_busy_at = Some(self.sample_clock);
        }
    }

    /// Returns `true` when the channel is currently (or recently) busy.
    pub fn is_busy(&self) -> bool {
        match self.last_busy_at {
            None => false,
            Some(at) => self.sample_clock.saturating_sub(at) <= self.hold_samples,
        }
    }

    /// Most recent RMS energy estimate.
    pub fn energy(&self) -> f32 {
        self.current_energy
    }

    /// Force the DCD into the busy state; useful in tests to inject carrier
    /// presence without going through the audio pipeline.
    pub fn force_busy(&mut self) {
        self.last_busy_at = Some(self.sample_clock);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_channel_is_not_busy() {
        let mut dcd = DcdState::new(0.01, 800);
        dcd.update(&vec![0.0f32; 8000]);
        assert!(!dcd.is_busy());
    }

    #[test]
    fn loud_signal_marks_busy() {
        let mut dcd = DcdState::new(0.01, 800);
        dcd.update(&vec![0.5f32; 8000]);
        assert!(dcd.is_busy());
    }

    #[test]
    fn busy_clears_after_hold_window() {
        let mut dcd = DcdState::new(0.01, 400);
        dcd.update(&vec![0.5f32; 100]); // triggers busy, clock = 100
        assert!(dcd.is_busy());
        // advance past hold window without further energy
        dcd.update(&vec![0.0f32; 500]); // clock = 600; 600 - 100 = 500 > 400
        assert!(!dcd.is_busy());
    }

    #[test]
    fn force_busy_makes_is_busy_true() {
        let mut dcd = DcdState::new(0.01, 800);
        dcd.force_busy();
        assert!(dcd.is_busy());
    }

    #[test]
    fn empty_update_does_not_change_state() {
        let mut dcd = DcdState::new(0.01, 800);
        dcd.force_busy();
        dcd.update(&[]);
        assert!(dcd.is_busy());
    }
}
