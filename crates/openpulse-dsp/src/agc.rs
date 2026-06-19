//! Automatic Gain Control — exponential-envelope loop.
//!
//! Normalises a signal to a target RMS level so downstream demodulation, soft-LLR
//! scaling, and amplitude-sensitive constellation decisions (QAM/APSK) see a
//! consistent level despite the 20–40 dB QSB fading and inter-station level
//! spread typical of HF. Models liquid-dsp's `agc_crcf`: a smoothed output-power
//! estimate drives a log-domain multiplicative gain update, with a [`lock`] that
//! freezes the gain during burst processing so a mid-frame gain change cannot
//! corrupt soft-decision scaling.
//!
//! Intended position: inside a plugin's demodulation chain (AGC → symbol timing →
//! carrier loop), the placement fielded HF modems use — *not* on the raw capture
//! buffer, whose long leading silence would ramp the gain to its clamp before the
//! burst arrives. Channel-busy / squelch detection stays in `DcdState`
//! (openpulse-core); this primitive only normalises level.
//!
//! [`lock`]: Agc::lock

/// Exponential-envelope automatic gain control for a real sample stream.
pub struct Agc {
    gain: f32,
    target_pow: f32,
    out_pow_est: f32,
    alpha: f32,
    gain_min: f32,
    gain_max: f32,
    locked: bool,
    initialised: bool,
}

impl Agc {
    /// Create an AGC.
    ///
    /// `target_rms` — desired output RMS (e.g. 0.3 leaves headroom below ±1.0).
    /// `bandwidth` — adaptation rate α in (0, 1]; smaller is slower/smoother
    ///   (1e-3..1e-1 typical; cf. the fielded SSB ratio AGC ≈ 0.02·loop_bw).
    /// `max_gain_db` — symmetric clamp on the gain magnitude in dB (both boost and
    ///   attenuation are bounded to ±`max_gain_db`).
    pub fn new(target_rms: f32, bandwidth: f32, max_gain_db: f32) -> Self {
        let g_max = 10f32.powf(max_gain_db.abs() / 20.0);
        Self {
            gain: 1.0,
            target_pow: (target_rms * target_rms).max(f32::MIN_POSITIVE),
            out_pow_est: 0.0,
            alpha: bandwidth.clamp(1e-6, 1.0),
            gain_min: 1.0 / g_max,
            gain_max: g_max,
            locked: false,
            initialised: false,
        }
    }

    /// Process one sample: apply the current gain, update the loop, return the output.
    pub fn process_sample(&mut self, x: f32) -> f32 {
        let y = self.gain * x;
        let y2 = y * y;
        if !self.initialised {
            self.out_pow_est = y2.max(f32::MIN_POSITIVE);
            self.initialised = true;
        } else {
            self.out_pow_est += self.alpha * (y2 - self.out_pow_est);
        }
        if !self.locked {
            // Log-domain multiplicative update drives out_pow_est → target_pow.
            // The 0.5 factor converts the power error into an amplitude (gain) step.
            let log_err = (self.out_pow_est / self.target_pow).ln();
            self.gain = (self.gain * (-0.5 * self.alpha * log_err).exp())
                .clamp(self.gain_min, self.gain_max);
        }
        y
    }

    /// Process a buffer in place.
    pub fn process(&mut self, samples: &mut [f32]) {
        for s in samples.iter_mut() {
            *s = self.process_sample(*s);
        }
    }

    /// Current linear gain.
    pub fn gain(&self) -> f32 {
        self.gain
    }

    /// Current gain in dB.
    pub fn gain_db(&self) -> f32 {
        20.0 * self.gain.log10()
    }

    /// Input-referred RMS estimate (`target_rms / gain` at steady state).
    ///
    /// A free signal-level readout once the loop has settled — useful for SNR
    /// reporting and rate adaptation.
    pub fn estimated_input_rms(&self) -> f32 {
        if self.gain > 0.0 {
            self.target_pow.sqrt() / self.gain
        } else {
            0.0
        }
    }

    /// Freeze the gain (e.g. after burst detection) so soft-decision scaling is
    /// stable across the frame.
    pub fn lock(&mut self) {
        self.locked = true;
    }

    /// Resume gain adaptation.
    pub fn unlock(&mut self) {
        self.locked = false;
    }

    /// Whether the gain is currently frozen.
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Reset to the initial (unity-gain, unsettled, unlocked) state.
    pub fn reset(&mut self) {
        self.gain = 1.0;
        self.out_pow_est = 0.0;
        self.locked = false;
        self.initialised = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RMS of the last `n` samples.
    fn tail_rms(samples: &[f32], n: usize) -> f32 {
        let tail = &samples[samples.len().saturating_sub(n)..];
        let mean_sq = tail.iter().map(|&s| s * s).sum::<f32>() / tail.len() as f32;
        mean_sq.sqrt()
    }

    /// Constant-magnitude ±`a` stream (clean, deterministic AGC level reference).
    fn alternating(a: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| if i % 2 == 0 { a } else { -a }).collect()
    }

    #[test]
    fn converges_to_target_from_very_low_input() {
        let target = 0.3f32;
        let mut agc = Agc::new(target, 0.02, 80.0);
        let mut buf = alternating(0.001, 8000);
        agc.process(&mut buf);
        let rms = tail_rms(&buf, 1000);
        assert!(
            (rms - target).abs() < 0.05 * target,
            "low-input output RMS {rms} should reach target {target}"
        );
    }

    #[test]
    fn converges_to_target_from_very_high_input() {
        let target = 0.3f32;
        let mut agc = Agc::new(target, 0.02, 80.0);
        let mut buf = alternating(50.0, 8000);
        agc.process(&mut buf);
        let rms = tail_rms(&buf, 1000);
        assert!(
            (rms - target).abs() < 0.05 * target,
            "high-input output RMS {rms} should reach target {target}"
        );
    }

    #[test]
    fn lock_freezes_gain() {
        let mut agc = Agc::new(0.3, 0.05, 80.0);
        agc.process(&mut alternating(0.01, 4000));
        let frozen = agc.gain();
        agc.lock();
        // Feed a wildly different level; gain must not move.
        agc.process(&mut alternating(5.0, 4000));
        assert_eq!(agc.gain(), frozen, "locked gain must not adapt");
        agc.unlock();
        agc.process(&mut alternating(5.0, 4000));
        assert!(
            agc.gain() < frozen,
            "gain should fall after unlock on a loud signal"
        );
    }

    #[test]
    fn respects_max_gain_clamp() {
        // 6 dB clamp ⇒ gain bounded to ×2. A near-silent input would otherwise
        // demand enormous boost.
        let mut agc = Agc::new(0.3, 0.1, 6.0);
        agc.process(&mut alternating(1e-6, 4000));
        let g_max = 10f32.powf(6.0 / 20.0);
        assert!(
            agc.gain() <= g_max * 1.001,
            "gain {} must be clamped to ×{g_max}",
            agc.gain()
        );
    }

    #[test]
    fn estimated_input_rms_tracks_true_level() {
        let mut agc = Agc::new(0.3, 0.02, 80.0);
        let input_rms = 0.02f32; // |x| = 0.02 constant ⇒ RMS = 0.02
        agc.process(&mut alternating(input_rms, 8000));
        let est = agc.estimated_input_rms();
        assert!(
            (est - input_rms).abs() < 0.1 * input_rms,
            "estimated input RMS {est} should track true {input_rms}"
        );
    }

    #[test]
    fn reset_restores_unity_gain() {
        let mut agc = Agc::new(0.3, 0.1, 80.0);
        agc.process(&mut alternating(10.0, 2000));
        assert!(agc.gain() < 1.0);
        agc.reset();
        assert_eq!(agc.gain(), 1.0);
        assert!(!agc.is_locked());
    }
}
