use std::time::{Duration, Instant};

/// Data Carrier Detect state machine.
///
/// Computes RMS energy over each batch of received samples.  When energy
/// exceeds `threshold` the channel is marked busy; the busy flag persists for
/// `hold_duration` of wall-clock time after the last detection.
///
/// The hold window is wall-clock-based so that an application that stops
/// polling `receive()` for an extended period will still see the channel
/// become clear after the hold expires.
#[derive(Debug, Clone)]
pub struct DcdState {
    threshold: f32,
    hold_duration: Duration,
    last_busy_instant: Option<Instant>,
    current_energy: f32,
}

impl DcdState {
    /// Create a DCD detector.
    ///
    /// `threshold` is the RMS amplitude (0.0–1.0) above which the channel is
    /// considered busy.  `hold_samples` is converted to a hold duration
    /// assuming 8 kHz sample rate (e.g. 800 → 100 ms).
    pub fn new(threshold: f32, hold_samples: usize) -> Self {
        let hold_ms = hold_samples as u64 * 1000 / 8000;
        Self::new_with_duration(threshold, Duration::from_millis(hold_ms.max(1)))
    }

    /// Create a DCD detector with an explicit hold duration.  Useful in tests
    /// where precise timing control is required.
    pub fn new_with_duration(threshold: f32, hold_duration: Duration) -> Self {
        Self {
            threshold,
            hold_duration,
            last_busy_instant: None,
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
        if self.current_energy >= self.threshold {
            self.last_busy_instant = Some(Instant::now());
        }
    }

    /// Returns `true` when the channel is currently (or recently) busy.
    ///
    /// The busy flag expires after `hold_duration` of wall-clock time since
    /// the last energy detection, regardless of whether `update()` is called
    /// in the interim.
    pub fn is_busy(&self) -> bool {
        self.last_busy_instant
            .map(|t| t.elapsed() < self.hold_duration)
            .unwrap_or(false)
    }

    /// Most recent RMS energy estimate.
    pub fn energy(&self) -> f32 {
        self.current_energy
    }

    /// Force the DCD into the busy state; useful in tests to inject carrier
    /// presence without going through the audio pipeline.
    pub fn force_busy(&mut self) {
        self.last_busy_instant = Some(Instant::now());
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
        let mut dcd = DcdState::new_with_duration(0.01, Duration::from_millis(10));
        dcd.update(&vec![0.5f32; 100]);
        assert!(dcd.is_busy());
        std::thread::sleep(Duration::from_millis(20));
        assert!(!dcd.is_busy());
    }

    #[test]
    fn busy_clears_even_without_further_receive_calls() {
        // Regression: hold must expire via wall clock, not sample count.
        let mut dcd = DcdState::new_with_duration(0.01, Duration::from_millis(10));
        dcd.update(&vec![0.5f32; 8]);
        assert!(dcd.is_busy());
        // No more update() calls — busy flag must still expire.
        std::thread::sleep(Duration::from_millis(20));
        assert!(!dcd.is_busy(), "busy flag must clear via wall clock");
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
