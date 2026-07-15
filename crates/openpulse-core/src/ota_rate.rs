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
use crate::fec::FecMode;
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
    // Operator controls:
    /// Lowest level adaptation may use (`None` = the profile's lowest mapped level).
    min_level: Option<SpeedLevel>,
    /// Highest level adaptation may use (`None` = the profile's highest mapped level).
    max_level: Option<SpeedLevel>,
    /// When set, both directions are pinned to this level and adaptation is off.
    locked: Option<SpeedLevel>,
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
            min_level: None,
            max_level: None,
            locked: None,
        }
    }

    // ── Operator controls ──────────────────────────────────────────────────────

    /// Clamp adaptation to `[min, max]` (each `None` = the profile's natural bound).
    /// Current levels are immediately snapped into the new range.
    pub fn set_level_bounds(&mut self, min: Option<SpeedLevel>, max: Option<SpeedLevel>) {
        self.min_level = min;
        self.max_level = max;
        self.rx_recommended = self.clamp_mapped(self.rx_recommended);
        self.rx_confirmed = self.clamp_mapped(self.rx_confirmed);
        self.tx_level = self.clamp_mapped(self.tx_level);
    }

    /// Pin both directions to `level` and stop adapting (a manual override).
    pub fn lock_level(&mut self, level: SpeedLevel) {
        let l = self.clamp_mapped(level);
        self.locked = Some(l);
        self.tx_level = l;
        self.rx_recommended = l;
        self.rx_confirmed = l;
    }

    /// Release a [`lock_level`](Self::lock_level) and resume adapting from the current level.
    pub fn unlock(&mut self) {
        self.locked = None;
    }

    /// Whether a manual level lock is in effect.
    pub fn is_locked(&self) -> bool {
        self.locked.is_some()
    }

    // ── Mapped-level navigation (bounds-aware) ─────────────────────────────────

    fn lo(&self) -> SpeedLevel {
        self.min_level
            .unwrap_or_else(|| *self.levels.first().unwrap_or(&SpeedLevel::Sl1))
    }

    fn hi(&self) -> SpeedLevel {
        self.max_level
            .unwrap_or_else(|| *self.levels.last().unwrap_or(&SpeedLevel::Sl1))
    }

    fn next_mapped(&self, level: SpeedLevel) -> SpeedLevel {
        let hi = self.hi();
        self.levels
            .iter()
            .copied()
            .find(|&l| l > level && l <= hi)
            .unwrap_or_else(|| self.clamp_mapped(level))
    }

    fn prev_mapped(&self, level: SpeedLevel) -> SpeedLevel {
        let lo = self.lo();
        self.levels
            .iter()
            .copied()
            .rev()
            .find(|&l| l < level && l >= lo)
            .unwrap_or_else(|| self.clamp_mapped(level))
    }

    fn clamp_mapped(&self, level: SpeedLevel) -> SpeedLevel {
        let (lo, hi) = (self.lo(), self.hi());
        let bounded = level.max(lo).min(hi);
        if self.levels.contains(&bounded) {
            return bounded;
        }
        // Snap to the nearest mapped level at or below `bounded`, staying within [lo, hi].
        self.levels
            .iter()
            .copied()
            .rev()
            .find(|&l| l <= bounded && l >= lo)
            .or_else(|| self.levels.iter().copied().find(|&l| l >= lo && l <= hi))
            .unwrap_or(bounded)
    }

    /// Highest mapped level within `[lo, hi]` the measured SNR supports — the "SNR-adequate"
    /// level, per the profile's per-level `snr_floor` thresholds. A level with no floor (the
    /// most robust rungs) is always adequate; when the SNR is below even the lowest floor this
    /// falls back to `lo`. This is the direct SNR→level lookup the fast downshift jumps to.
    fn level_for_snr(&self, snr_db: f32) -> SpeedLevel {
        let (lo, hi) = (self.lo(), self.hi());
        self.levels
            .iter()
            .copied()
            .filter(|&l| l >= lo && l <= hi)
            .filter(|&l| {
                self.profile
                    .snr_floor_for_level(l)
                    .is_none_or(|f| snr_db >= f)
            })
            .max()
            .unwrap_or(lo)
    }

    // ── TX side (we follow the peer) ───────────────────────────────────────────

    /// Adopt the peer's absolute rate recommendation as our TX level.
    ///
    /// Ignored while locked (the manual override wins); otherwise clamped into the
    /// configured `[min, max]` bounds.
    pub fn adopt_recommendation(&mut self, level: SpeedLevel) {
        if self.locked.is_some() {
            return;
        }
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

    /// FEC scheme we should transmit data with at the current TX level (MODCOD).
    pub fn tx_fec(&self) -> FecMode {
        self.profile.fec_for(self.tx_level)
    }

    /// Mode string mapped to an arbitrary level in this profile (for ACK-waveform selection).
    pub fn mode_for_level(&self, level: SpeedLevel) -> Option<&'static str> {
        self.profile.mode_for(level)
    }

    /// Does this profile define a rung with the given mode? Gates the sub-floor union-listen ACK path.
    pub fn profile_has_mode(&self, mode: &str) -> bool {
        self.profile
            .defined_levels()
            .into_iter()
            .any(|l| self.profile.mode_for(l) == Some(mode))
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

    /// `(level, mode)` candidates to attempt when demodulating the next data
    /// frame, most-likely first.
    ///
    /// The lockstep invariant guarantees this set covers whatever the sender is
    /// using: the recommended level (if it adopted our last ACK) or the confirmed
    /// level (if that ACK was lost). At most two entries.
    pub fn rx_candidates(&self) -> Vec<(SpeedLevel, &'static str, FecMode)> {
        let mut out = Vec::with_capacity(2);
        if let Some(m) = self.profile.mode_for(self.rx_recommended) {
            out.push((
                self.rx_recommended,
                m,
                self.profile.fec_for(self.rx_recommended),
            ));
        }
        if self.rx_confirmed != self.rx_recommended {
            if let Some(m) = self.profile.mode_for(self.rx_confirmed) {
                if !out.iter().any(|&(l, _, _)| l == self.rx_confirmed) {
                    out.push((
                        self.rx_confirmed,
                        m,
                        self.profile.fec_for(self.rx_confirmed),
                    ));
                }
            }
        }
        out
    }

    /// Mode strings to attempt when demodulating the next data frame, most-likely first.
    pub fn rx_candidate_modes(&self) -> Vec<&'static str> {
        self.rx_candidates()
            .into_iter()
            .map(|(_, m, _)| m)
            .collect()
    }

    /// Update RX state from a demodulation outcome and measured SNR, and return the
    /// ACK the receiver should send (type + absolute recommendation).
    pub fn on_rx_frame(&mut self, outcome: RxOutcome, snr_db: f32) -> RxAck {
        // While locked, keep both directions pinned and recommend the locked level.
        if let Some(l) = self.locked {
            let ack_type = match outcome {
                RxOutcome::Failed => AckType::Nack,
                RxOutcome::Decoded(_) => AckType::AckOk,
            };
            return RxAck {
                ack_type,
                recommended_level: l,
            };
        }
        match outcome {
            RxOutcome::Failed => {
                // Asymmetric fast downshift: if the SNR estimate already explains the failure
                // (the SNR-adequate level is below what we're recommending), jump the
                // recommendation straight there instead of crawling down one rung per NACK
                // threshold — the "6 retries to find the step" symptom. `rx_confirmed` stays put
                // as the fallback candidate so a lost downshift ACK can't desync the receiver
                // (`rx_candidates` still covers whatever the sender is transmitting).
                let snr_level = self.level_for_snr(snr_db);
                if snr_level < self.rx_recommended {
                    self.rx_consecutive_nack = 0;
                    self.rx_recommended = snr_level;
                } else {
                    // SNR doesn't explain the failure (a transient fade or collision at an
                    // otherwise-adequate SNR): keep the consecutive-NACK hysteresis so a single
                    // blip can't drop the rate, stepping the anchor down one rung at the threshold.
                    self.rx_consecutive_nack = self.rx_consecutive_nack.saturating_add(1);
                    if self.rx_consecutive_nack >= self.profile.nack_threshold {
                        self.rx_consecutive_nack = 0;
                        self.rx_confirmed = self.prev_mapped(self.rx_confirmed);
                        self.rx_recommended = self.rx_confirmed;
                    }
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

                // Asymmetric recommendation from the SNR-adequate level:
                //  • fast DOWN — if SNR can't support the confirmed level, jump straight to the
                //    SNR-adequate rung (possibly several steps); the just-decoded `rx_confirmed`
                //    stays in the candidate set, so this is desync-safe.
                //  • cautious UP — never trust an optimistic SNR to leap up: climb one proven
                //    mapped step only, and only once SNR clears the confirmed level's ceiling.
                let snr_level = self.level_for_snr(snr_db);
                self.rx_recommended = if snr_level < self.rx_confirmed {
                    snr_level
                } else if self
                    .profile
                    .snr_ceiling_for_level(self.rx_confirmed)
                    .is_some_and(|c| snr_db >= c)
                {
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

    /// The MFSK16 SL1 sub-floor rung is not a trapdoor: once SNR recovers above SL1's ceiling, the
    /// receiver-led controller climbs the recommendation back out to SL2. (In production the real SNR comes
    /// from MFSK16's `estimate_snr_db`; the M2M4 fallback would bias it low and pin SL1 — see PR-1.)
    #[test]
    fn subfloor_sl1_climbs_back_out_when_snr_recovers() {
        let mut c = ctrl();
        // Entry: one low-SNR failed frame fast-downshifts the recommendation to the SL1 sub-floor rung.
        c.on_rx_frame(RxOutcome::Failed, -5.0);
        assert_eq!(c.rx_recommended_level(), SpeedLevel::Sl1);
        // Recovery: a decoded SL1 frame with SNR above SL1's 5 dB ceiling climbs the recommendation to SL2.
        let ack = c.on_rx_frame(RxOutcome::Decoded(SpeedLevel::Sl1), 6.0);
        assert_eq!(ack.ack_type, AckType::AckUp);
        assert_eq!(c.rx_recommended_level(), SpeedLevel::Sl2);
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
    fn recommendation_is_at_most_one_step_above_confirmed_but_may_drop_further() {
        let mut c = ctrl();
        // Drive a varied SNR sequence; the asymmetric invariant must hold after every frame.
        let snrs = [
            HIGH_SNR, HIGH_SNR, LOW_SNR, HIGH_SNR, 0.0, HIGH_SNR, LOW_SNR,
        ];
        for &snr in snrs.iter().cycle().take(40) {
            // Sender transmits whatever it last adopted; model it as the confirmed level.
            let _ = c.on_rx_frame(RxOutcome::Decoded(c.rx_confirmed), snr);
            let conf = c.rx_confirmed;
            let rec = c.rx_recommended;
            // Cautious UP: never more than one mapped step above the confirmed anchor.
            assert!(
                rec <= c.next_mapped(conf),
                "rec {rec:?} more than one step above confirmed {conf:?}"
            );
            // Fast DOWN: may drop multiple steps, but never below the configured floor.
            assert!(rec >= c.lo(), "rec {rec:?} below the floor {:?}", c.lo());
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
    fn fast_downshift_on_the_first_low_snr_failure() {
        // The HamRadio-2026 symptom: it took ~6 retries (one rung per NACK threshold) to reach a
        // decodable rate. A single low-SNR failure must now drop the recommendation straight to the
        // SNR-adequate floor — no crawl.
        let mut c = ctrl();
        let mut sender_tx = c.tx_level();
        for _ in 0..10 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), HIGH_SNR);
            sender_tx = ack.recommended_level;
        }
        let before = c.rx_recommended;
        assert!(
            before > c.levels[0],
            "precondition: climbed above the floor"
        );
        let ack = c.on_rx_frame(RxOutcome::Failed, LOW_SNR);
        assert_eq!(ack.ack_type, AckType::Nack);
        assert_eq!(
            c.rx_recommended,
            c.lo(),
            "one low-SNR NACK jumps straight to the SNR-floor level"
        );
        assert!(c.rx_recommended < before);
    }

    #[test]
    fn transient_failure_at_good_snr_keeps_the_nack_hysteresis() {
        // A failure while the SNR is still adequate (collision / momentary fade) must NOT fast-drop;
        // the consecutive-NACK hysteresis steps the anchor down one rung only at the threshold.
        let mut c = ctrl();
        let mut sender_tx = c.tx_level();
        for _ in 0..10 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), HIGH_SNR);
            sender_tx = ack.recommended_level;
        }
        let before = c.rx_confirmed;
        assert!(before > c.levels[0]);
        for i in 0..c.profile.nack_threshold {
            let ack = c.on_rx_frame(RxOutcome::Failed, HIGH_SNR);
            assert_eq!(ack.ack_type, AckType::Nack);
            if i + 1 < c.profile.nack_threshold {
                assert_eq!(
                    c.rx_confirmed, before,
                    "must not drop before the NACK threshold at good SNR"
                );
            }
        }
        assert!(
            c.rx_confirmed < before,
            "steps down one rung at the NACK threshold"
        );
    }

    #[test]
    fn max_level_clamp_caps_the_climb() {
        let mut c = ctrl();
        c.set_level_bounds(None, Some(SpeedLevel::Sl4));
        let mut sender_tx = c.tx_level();
        for _ in 0..30 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), HIGH_SNR);
            sender_tx = ack.recommended_level;
        }
        assert!(
            c.rx_confirmed <= SpeedLevel::Sl4,
            "must not climb past the max bound: {:?}",
            c.rx_confirmed
        );
        assert!(
            c.rx_recommended <= SpeedLevel::Sl4,
            "recommendation must respect the max bound"
        );
    }

    #[test]
    fn min_level_clamp_floors_the_descent() {
        let mut c = ctrl();
        // Climb up, then set a floor and hammer with failures.
        let mut sender_tx = c.tx_level();
        for _ in 0..10 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), HIGH_SNR);
            sender_tx = ack.recommended_level;
        }
        c.set_level_bounds(Some(SpeedLevel::Sl4), None);
        assert!(
            c.rx_confirmed >= SpeedLevel::Sl4,
            "bounds snap current level up to the floor"
        );
        for _ in 0..30 {
            let _ = c.on_rx_frame(RxOutcome::Failed, LOW_SNR);
        }
        assert!(
            c.rx_recommended >= SpeedLevel::Sl4,
            "must not drop below the min bound: {:?}",
            c.rx_recommended
        );
    }

    #[test]
    fn lock_pins_both_directions_and_ignores_peer() {
        let mut c = ctrl();
        c.lock_level(SpeedLevel::Sl4);
        assert!(c.is_locked());
        assert_eq!(c.tx_level(), SpeedLevel::Sl4);
        assert_eq!(c.rx_recommended_level(), SpeedLevel::Sl4);
        // RX decisions stay pinned regardless of SNR.
        let ack = c.on_rx_frame(RxOutcome::Decoded(SpeedLevel::Sl4), HIGH_SNR);
        assert_eq!(ack.recommended_level, SpeedLevel::Sl4);
        assert_eq!(c.rx_recommended_level(), SpeedLevel::Sl4);
        // Peer recommendations are ignored while locked.
        c.adopt_recommendation(SpeedLevel::Sl6);
        assert_eq!(c.tx_level(), SpeedLevel::Sl4);
        // Unlocking resumes adaptation.
        c.unlock();
        assert!(!c.is_locked());
        c.adopt_recommendation(SpeedLevel::Sl5);
        assert_eq!(c.tx_level(), SpeedLevel::Sl5);
    }

    #[test]
    fn low_snr_fast_downshifts_past_a_single_step() {
        let mut c = ctrl();
        // Climb several steps so a multi-step drop is possible.
        let mut sender_tx = c.tx_level();
        for _ in 0..6 {
            let ack = c.on_rx_frame(RxOutcome::Decoded(sender_tx), HIGH_SNR);
            sender_tx = ack.recommended_level;
        }
        let conf = c.rx_confirmed;
        assert!(
            conf > c.next_mapped(c.lo()),
            "precondition: climbed ≥2 steps above the floor"
        );
        // A decoded frame at very low SNR drops the recommendation straight to the SNR-adequate
        // floor — several steps, not one.
        let ack = c.on_rx_frame(RxOutcome::Decoded(conf), LOW_SNR);
        assert_eq!(ack.ack_type, AckType::AckDown);
        assert_eq!(
            ack.recommended_level,
            c.lo(),
            "jumps to the SNR floor level"
        );
        assert!(
            c.lo() < c.prev_mapped(conf),
            "the drop went below a single step (multi-step downshift)"
        );
    }

    #[test]
    fn cautious_upshift_still_climbs_one_step_only() {
        // The up direction is unchanged: even at unbounded SNR the recommendation advances by at
        // most one mapped step per confirmed decode (never leaps to the SNR-adequate top).
        let mut c = ctrl();
        let conf = c.rx_confirmed;
        let ack = c.on_rx_frame(RxOutcome::Decoded(conf), HIGH_SNR);
        assert_eq!(ack.ack_type, AckType::AckUp);
        assert_eq!(
            ack.recommended_level,
            c.next_mapped(conf),
            "up-shift is one proven step, not a jump to the SNR-adequate ceiling"
        );
    }
}
