//! Per-session adaptive pilot density state for SC-FDMA.

/// Exponentially-smoothed coherence bandwidth estimate used to adapt pilot spacing.
pub struct AdaptivePilotState {
    smoothed_coh_bw_hz: f32,
    alpha: f32,
    n_updates: u32,
}

impl AdaptivePilotState {
    pub fn new() -> Self {
        Self {
            smoothed_coh_bw_hz: 300.0,
            alpha: 0.3,
            n_updates: 0,
        }
    }

    /// Update the smoothed estimate with a new coherence BW observation.
    pub fn update(&mut self, coh_bw_hz: f32) {
        if self.n_updates == 0 {
            self.smoothed_coh_bw_hz = coh_bw_hz;
        } else {
            self.smoothed_coh_bw_hz =
                self.alpha * coh_bw_hz + (1.0 - self.alpha) * self.smoothed_coh_bw_hz;
        }
        self.n_updates += 1;
    }

    /// Return the current smoothed coherence bandwidth estimate in Hz.
    pub fn coh_bw_hz(&self) -> f32 {
        self.smoothed_coh_bw_hz
    }
}

impl Default for AdaptivePilotState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ema_step_response_matches_formula() {
        let mut s = AdaptivePilotState::new();
        s.update(2000.0); // first call → sets directly to 2000
        s.update(100.0); // EMA: 0.3×100 + 0.7×2000 = 1430
        let expected = 0.3 * 100.0 + 0.7 * 2000.0;
        assert!(
            (s.coh_bw_hz() - expected).abs() < 1.0,
            "EMA step: expected {expected:.1}, got {:.1}",
            s.coh_bw_hz()
        );
    }

    #[test]
    fn ema_converges_from_selective() {
        let mut s = AdaptivePilotState::new();
        for _ in 0..20 {
            s.update(60.0);
        }
        assert!(s.coh_bw_hz() < 100.0);
    }

    #[test]
    fn first_update_sets_directly() {
        let mut s = AdaptivePilotState::new();
        s.update(42.0);
        assert_eq!(s.coh_bw_hz(), 42.0);
    }
}
