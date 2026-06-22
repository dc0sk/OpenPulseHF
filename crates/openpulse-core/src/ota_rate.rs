//! Receiver-led, per-direction adaptive rate control with RX lockstep.
//!
//! On a two-way link each *data receiver* leads the rate for the direction it
//! receives: it measures channel quality, picks an **absolute** target speed
//! level, and ships that level to the sender in the ACK
//! ([`crate::ack::AckFrame::recommended_level`]). The sender simply follows.
//!
//! ## Lockstep invariant
//!
//! The receiver advances its recommendation at most **one mapped step** above the
//! highest level it has actually decoded (`rx_confirmed`). So the demodulation
//! candidate set `{rx_recommended, rx_confirmed}` is always exactly the 1–2 modes
//! the sender could be transmitting:
//!
//! - sender adopted the recommendation → it sends at `rx_recommended`;
//! - the recommending ACK was lost → the sender still uses the last level it was
//!   told, which is `rx_confirmed`.
//!
//! Because the node that *decides* the mode is the node that *demodulates* it, a
//! lost ACK can never desync the two ends — it only delays the climb by one frame.
//!
//! This is pure logic: no I/O, no engine coupling, fully unit-tested. The modem
//! engine drives it by reporting which candidate decoded and the measured SNR.

use crate::ack::AckType;
use crate::profile::SessionProfile;
use crate::rate::SpeedLevel;

/// Outcome of demodulating a received data frame, fed to [`OtaRateController::on_rx_frame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxOutcome {
    /// A candidate mode decoded cleanly at the given speed level, with measured SNR.
    Decoded(SpeedLevel),
    /// No candidate decoded — treat as a NACK.
    Failed,
}

/// What the receiver should put in the ACK after [`OtaRateController::on_rx_frame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RxAck {
    /// ACK type for legacy peers (derived from the recommendation direction / failure).
    pub ack_type: AckType,
    /// Absolute receiver-led rate target the sender should adopt.
    pub recommended_level: SpeedLevel,
}

/// Receiver-led per-direction rate controller for one session.
#[derive(Debug, Clone)]
pub struct OtaRateController {
    profile: SessionProfile,
    levels: Vec<SpeedLevel>, // mapped levels, ascending
    // RX direction (we are the data receiver and lead the rate):
    rx_recommended: SpeedLevel,
    rx_confirmed: SpeedLevel,
    rx_consecutive_nack: u8,
    // TX direction (we are the data sender and follow the peer):
    tx_level: SpeedLevel,
}

impl OtaRateController {
    /// Create a controller for `profile`, both directions starting at the profile's
    /// initial level (clamped to a mapped level).
    pub fn new(profile: SessionProfile) -> Self {
        let levels = profile.defined_levels();
        let initial = if levels.contains(&profile.initial_level) {
            profile.initial_level
        } else {
            // Fall back to the lowest mapped level if the configured initial is unmapped.
            *levels.first().unwrap_or(&SpeedLevel::Sl1)
        };
        Self {
            profile,
            levels,
            rx_recommended: initial,
            rx_confirmed: initial,
            rx_consecutive_nack: 0,
            tx_level: initial,
        }
    }

    // ── Mapped-level navigation ────────────────────────────────────────────────

    fn next_mapped(&self, level: SpeedLevel) -> SpeedLevel {
        self.levels
            .iter()
            .copied()
            .find(|&l| l > level)
            .unwrap_or(level)
    }

    fn prev_mapped(&self, level: SpeedLevel) -> SpeedLevel {
        self.levels
            .iter()
            .copied()
            .rev()
            .find(|&l| l < level)
            .unwrap_or(level)
    }

    fn clamp_mapped(&self, level: SpeedLevel) -> SpeedLevel {
        if self.levels.contains(&level) {
            level
        } else {
            // Snap down to the nearest mapped level at or below `level`.
            self.levels
                .iter()
                .copied()
                .rev()
                .find(|&l| l <= level)
                .or_else(|| self.levels.first().copied())
                .unwrap_or(level)
        }
    }

    // ── TX side (we follow the peer) ───────────────────────────────────────────

    /// Adopt the peer's absolute rate recommendation as our TX level.
    pub fn adopt_recommendation(&mut self, level: SpeedLevel) {
        self.tx_level = self.clamp_mapped(level);
    }

    /// Current TX speed level.
    pub fn tx_level(&self) -> SpeedLevel {
        self.tx_level
    }

    /// Mode string we should transmit data at.
    pub fn tx_mode(&self) -> Option<&'static str> {
        self.profile.mode_for(self.tx_level)
    }

    // ── RX side (we lead) ──────────────────────────────────────────────────────

    /// Current absolute level we are recommending to the peer.
    pub fn rx_recommended_level(&self) -> SpeedLevel {
        self.rx_recommended
    }

    /// Highest level we have actually decoded (the lockstep anchor).
    pub fn rx_confirmed_level(&self) -> SpeedLevel {
        self.rx_confirmed
    }

    /// Modes to attempt when demodulating the next data frame, most-likely first.
    ///
    /// The lockstep invariant guarantees this set covers whatever the sender is
    /// using: the recommended level (if it adopted our last ACK) or the confirmed
    /// level (if that ACK was lost). At most two entries.
    pub fn rx_candidate_modes(&self) -> Vec<&'static str> {
        let mut modes = Vec::with_capacity(2);
        if let Some(m) = self.profile.mode_for(self.rx_recommended) {
            modes.push(m);
        }
        if self.rx_confirmed != self.rx_recommended {
            if let Some(m) = self.profile.mode_for(self.rx_confirmed) {
                if !modes.contains(&m) {
                    modes.push(m);
                }
            }
        }
        modes
    }

    /// Update RX state from a demodulation outcome and measured SNR, and return the
    /// ACK the receiver should send (type + absolute recommendation).
    pub fn on_rx_frame(&mut self, outcome: RxOutcome, snr_db: f32) -> RxAck {
        match outcome {
            RxOutcome::Failed => {
                self.rx_consecutive_nack = self.rx_consecutive_nack.saturating_add(1);
                if self.rx_consecutive_nack >= self.profile.nack_threshold {
                    self.rx_consecutive_nack = 0;
                    self.rx_confirmed = self.prev_mapped(self.rx_confirmed);
                    self.rx_recommended = self.rx_confirmed;
                }
                RxAck {
                    ack_type: AckType::Nack,
                    recommended_level: self.rx_recommended,
                }
            }
            RxOutcome::Decoded(level) => {
                self.rx_consecutive_nack = 0;
                // Anchor on the level we actually decoded (recommended if the sender
                // adopted it, else the fallback level).
                self.rx_confirmed = self.clamp_mapped(level);

                // Choose the next recommendation: at most one mapped step from the
                // freshly confirmed anchor, gated by the per-level SNR thresholds.
                let floor = self.profile.snr_floor_for_level(self.rx_confirmed);
                let ceiling = self.profile.snr_ceiling_for_level(self.rx_confirmed);
                self.rx_recommended = if floor.is_some_and(|f| snr_db < f) {
                    self.prev_mapped(self.rx_confirmed)
                } else if ceiling.is_some_and(|c| snr_db >= c) {
                    self.next_mapped(self.rx_confirmed)
                } else {
                    self.rx_confirmed
                };

                let ack_type = match self.rx_recommended.cmp(&self.rx_confirmed) {
                    std::cmp::Ordering::Greater => AckType::AckUp,
                    std::cmp::Ordering::Less => AckType::AckDown,
                    std::cmp::Ordering::Equal => AckType::AckOk,
                };
                RxAck {
                    ack_type,
                    recommended_level: self.rx_recommended,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIGH_SNR: f32 = 1.0e9;
    const LOW_SNR: f32 = -1.0e9;

    fn ctrl() -> OtaRateController {
        OtaRateController::new(SessionProfile::hpx_hf())
    }

    #[test]
    fn tx_follows_recommendation_and_clamps_unmapped() {
        let mut c = ctrl();
        let levels = c.levels.clone();
        let top = *levels.last().unwrap();
        c.adopt_recommendation(top);
        assert_eq!(c.tx_level(), top);
        assert!(c.tx_mode().is_some());
        // SL1 (chirp) is unmapped in hpx_hf → clamps to a mapped level, never panics.
        c.adopt_recommendation(SpeedLevel::Sl1);
        assert!(c.levels.contains(&c.tx_level()));
    }

    #[test]
    fn candidate_set_is_at_most_two_and_recommended_first() {
        let mut c = ctrl();
        // Force a one-step-ahead recommendation via a confirmed decode at high SNR.
        let start = c.rx_confirmed;
        let _ = c.on_rx_frame(RxOutcome::Decoded(start), HIGH_SNR);
        let modes = c.rx_candidate_modes();
        assert!(modes.len() <= 2);
        assert_eq!(modes.first().copied(), c.profile.mode_for(c.rx_recommended));
    }

    #[test]
    fn recommendation_stays_within_one_mapped_step_of_confirmed() {
        let mut c = ctrl();
        // Drive a varied SNR sequence; the invariant must hold after every frame.
        let snrs = [
            HIGH_SNR, HIGH_SNR, LOW_SNR, HIGH_SNR, 0.0, HIGH_SNR, LOW_SNR,
        ];
        for &snr in snrs.iter().cycle().take(40) {
            // Sender transmits whatever it last adopted; model it as the confirmed level.
            let _ = c.on_rx_frame(RxOutcome::Decoded(c.rx_confirmed), snr);
            let conf = c.rx_confirmed;
            let rec = c.rx_recommended;
            // rec ∈ {prev(conf), conf, next(conf)}.
            assert!(
                rec == conf || rec == c.next_mapped(conf) || rec == c.prev_mapped(conf),
                "rec {rec:?} more than one mapped step from confirmed {conf:?}"
            );
        }
    }

    #[test]
    fn climbs_under_good_snr_without_loss() {
        let mut c = ctrl();
        let initial = c.rx_confirmed;
        // No ACK loss: sender always adopts the recommendation; receiver always
        // decodes at exactly what it recommended last round.
        let mut sender_tx = c.tx_level();
        for _ in 0..30 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), HIGH_SNR);
            sender_tx = ack.recommended_level; // delivered, sender adopts
        }
        assert!(
            c.rx_confirmed > initial,
            "expected the rate to climb above the initial level under sustained good SNR"
        );
    }

    /// The lockstep theorem: under adequate SNR and ANY ACK-loss pattern, the
    /// sender's level is always in the receiver's candidate set, so it never desyncs.
    #[test]
    fn never_desyncs_under_arbitrary_ack_loss() {
        // A few deterministic loss patterns (every Nth ACK lost, plus all-lost).
        for &period in &[1usize, 2, 3, 5, 7] {
            let mut c = ctrl();
            let mut sender_tx = c.tx_level();
            for round in 0..60 {
                // Receiver decides which candidate the sender's level matches.
                let candidate =
                    sender_tx == c.rx_recommended_level() || sender_tx == c.rx_confirmed_level();
                assert!(
                    candidate,
                    "desync: sender at {sender_tx:?} not in {{rec {:?}, conf {:?}}} (period {period}, round {round})",
                    c.rx_recommended_level(),
                    c.rx_confirmed_level()
                );
                let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), HIGH_SNR);
                // Lose this ACK if the round is on the loss period; else sender adopts.
                let lost = (round % period) == 0;
                if !lost {
                    sender_tx = ack.recommended_level;
                }
            }
        }
    }

    /// Even when every ACK is lost in the *climb-announcing* direction, a good
    /// channel still makes progress once an ACK gets through.
    #[test]
    fn recovers_and_climbs_through_intermittent_loss() {
        let mut c = ctrl();
        let initial = c.rx_confirmed;
        let mut sender_tx = c.tx_level();
        for round in 0..80 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), HIGH_SNR);
            // Deliver only every 3rd ACK.
            if round % 3 == 2 {
                sender_tx = ack.recommended_level;
            }
        }
        assert!(
            c.rx_confirmed > initial,
            "should still climb despite 2/3 ACK loss"
        );
    }

    #[test]
    fn steps_down_after_consecutive_nacks() {
        let mut c = ctrl();
        // Climb up first so there's room to fall.
        let mut sender_tx = c.tx_level();
        for _ in 0..10 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), HIGH_SNR);
            sender_tx = ack.recommended_level;
        }
        let before = c.rx_confirmed;
        assert!(before > c.levels[0]);
        for _ in 0..c.profile.nack_threshold {
            let ack = c.on_rx_frame(RxOutcome::Failed, LOW_SNR);
            assert_eq!(ack.ack_type, AckType::Nack);
        }
        assert!(
            c.rx_confirmed < before,
            "rate should step down after the NACK threshold"
        );
    }

    #[test]
    fn low_snr_recommends_step_down() {
        let mut c = ctrl();
        // Climb a couple steps so a down-step is possible.
        let mut sender_tx = c.tx_level();
        for _ in 0..6 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), HIGH_SNR);
            sender_tx = ack.recommended_level;
        }
        let conf = c.rx_confirmed;
        let ack = c.on_rx_frame(RxOutcome::Decoded(conf), LOW_SNR);
        assert_eq!(ack.ack_type, AckType::AckDown);
        assert!(ack.recommended_level < conf);
    }
}
