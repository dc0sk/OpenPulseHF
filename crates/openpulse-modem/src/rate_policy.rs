//! Rate adaptation + SNR feedback policy extracted from `ModemEngine`.
//!
//! Owns the bidirectional rate adapter, active session profile, and the most
//! recent receive-path SNR estimate.  Returns `RateChangePayload` values for
//! the engine to forward as `EngineEvent::RateChange` broadcasts.

use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::{BiDirRateAdapter, RateEvent, RateTrigger, SpeedLevel};

use crate::event::RateDirection;

/// Snapshot of a rate-adapter change ready to be lifted into `EngineEvent::RateChange`.
#[derive(Debug, Clone)]
pub(crate) struct RateChangePayload {
    pub event: RateEvent,
    pub speed_level: SpeedLevel,
    pub mode: String,
    pub direction: Option<RateDirection>,
    pub trigger: Option<RateTrigger>,
}

/// Bidirectional rate adapter + session profile + last-RX SNR estimate.
pub(crate) struct RateAdaptationPolicy {
    rate_adapter: Option<BiDirRateAdapter>,
    session_profile: Option<SessionProfile>,
    last_rx_snr_db: Option<f32>,
    /// A2 (Mercury backlog-aware gating): minimum queued TX bytes required before
    /// an AckUp upgrade is acted on. `0` disables the gate (default).  Prevents
    /// spending MODE_REQ/upgrade airtime when only a frame or two remain queued.
    min_backlog_for_upgrade: usize,
    /// A2: current queued TX backlog in bytes, fed by the engine.
    tx_backlog_bytes: usize,
    /// A3 (Mercury anti-oscillation hold): number of upgrade attempts to suppress
    /// after a downgrade. `0` disables the hold (default).
    upgrade_hold_frames: u32,
    /// A3: remaining suppressed upgrade attempts.
    upgrade_hold_remaining: u32,
    /// Host/bandwidth cap (e.g. ARDOP `ARQBW`): the TX/RX level is never raised above this.
    /// `None` = no cap (the profile's own ceiling applies).
    max_tx_level: Option<SpeedLevel>,
    /// The session profile's own cap (`robust` = SL6). Kept apart from `max_tx_level` because the
    /// host cap is overwritten on every ARQBW change; this one only ever lowers the ceiling.
    profile_cap: Option<SpeedLevel>,
    /// Lowest level the session may use (`None` = SL1). A front end with no SL1 waveform (the ARDOP
    /// TNC registers no MFSK16) sets SL2 so NACK exhaustion cannot land on a dead rung.
    min_tx_level: Option<SpeedLevel>,
}

impl RateAdaptationPolicy {
    pub fn new() -> Self {
        Self {
            rate_adapter: None,
            session_profile: None,
            last_rx_snr_db: None,
            min_backlog_for_upgrade: 0,
            tx_backlog_bytes: 0,
            upgrade_hold_frames: 0,
            upgrade_hold_remaining: 0,
            max_tx_level: None,
            profile_cap: None,
            min_tx_level: None,
        }
    }

    /// The ceiling in force: the lower of the profile cap and the host cap.
    fn effective_cap(&self) -> Option<SpeedLevel> {
        [self.profile_cap, self.max_tx_level]
            .into_iter()
            .flatten()
            .min()
    }

    /// Set (or clear) the floor and immediately raise the active session to it.
    pub fn set_min_tx_level(&mut self, min: Option<SpeedLevel>) {
        self.min_tx_level = min;
        if let (Some(m), Some(adapter)) = (min, self.rate_adapter.as_mut()) {
            adapter.tx.raise_to(m);
            adapter.rx.raise_to(m);
        }
    }

    /// Hold both directions inside `[min_tx_level, effective_cap]`. Returns `(capped, floored)`:
    /// whether the TX level had to be lowered or raised.
    fn enforce_bounds(&mut self) -> (bool, bool) {
        let (cap, floor) = (self.effective_cap(), self.min_tx_level);
        let Some(adapter) = self.rate_adapter.as_mut() else {
            return (false, false);
        };
        let mut capped = false;
        let mut floored = false;
        if let Some(max) = cap {
            capped = adapter.tx.clamp_to(max);
            adapter.rx.clamp_to(max);
        }
        if let Some(min) = floor {
            floored = adapter.tx.raise_to(min);
            adapter.rx.raise_to(min);
        }
        (capped, floored)
    }

    /// Set (or clear) the host/bandwidth cap and immediately clamp the active session to it.
    pub fn set_max_tx_level(&mut self, max: Option<SpeedLevel>) {
        self.max_tx_level = max;
        self.enforce_bounds();
    }

    /// The active profile's defined `(level, mode)` pairs at or below its cap, ascending — used to
    /// map a bandwidth cap (Hz) to a max speed level. Empty when no session is active.
    pub fn defined_modes(&self) -> Vec<(SpeedLevel, &'static str)> {
        self.session_profile
            .as_ref()
            .map(|p| {
                p.reachable_levels()
                    .into_iter()
                    .filter_map(|l| p.mode_for(l).map(|m| (l, m)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A2: set the minimum queued-byte backlog that justifies acting on an AckUp
    /// upgrade (`0` disables the gate).
    pub fn set_min_backlog_for_upgrade(&mut self, bytes: usize) {
        self.min_backlog_for_upgrade = bytes;
    }

    /// A2: update the current queued TX backlog (bytes) used by the gate.
    pub fn set_tx_backlog(&mut self, bytes: usize) {
        self.tx_backlog_bytes = bytes;
    }

    /// A3: set how many upgrade attempts are suppressed after a downgrade
    /// (`0` disables the anti-oscillation hold).
    pub fn set_upgrade_hold_frames(&mut self, frames: u32) {
        self.upgrade_hold_frames = frames;
    }

    pub fn start_session(&mut self, profile: SessionProfile) {
        let initial = profile.initial_level;
        let threshold = profile.nack_threshold;
        self.profile_cap = profile.max_level();
        self.rate_adapter = Some(BiDirRateAdapter::new(initial, threshold));
        self.session_profile = Some(profile);
        self.enforce_bounds();
    }

    pub fn apply_ack(&mut self, ack: AckType) -> (RateEvent, Option<RateChangePayload>) {
        self.apply_ack_internal(ack, None)
    }

    pub fn apply_ack_frame(&mut self, frame: &AckFrame) -> (RateEvent, Vec<RateChangePayload>) {
        let mut payloads = Vec::new();
        let (tx_event, tx_payload) =
            self.apply_ack_internal(frame.ack_type, Some(RateDirection::Tx));
        if let Some(p) = tx_payload {
            payloads.push(p);
        }
        if let Some(rev) = frame.reverse_ack {
            if let Some(adapter) = self.rate_adapter.as_mut() {
                let rx_event = adapter.apply_reverse_ack(rev);
                self.enforce_bounds();
                let Some(adapter) = self.rate_adapter.as_ref() else {
                    return (tx_event, payloads);
                };
                let rx_level = adapter.rx_level();
                let mode = self
                    .session_profile
                    .as_ref()
                    .and_then(|p| p.mode_for(rx_level))
                    .unwrap_or("unknown")
                    .to_string();
                payloads.push(RateChangePayload {
                    event: rx_event,
                    speed_level: rx_level,
                    mode,
                    direction: Some(RateDirection::Rx),
                    trigger: None,
                });
            }
        }
        (tx_event, payloads)
    }

    fn apply_ack_internal(
        &mut self,
        ack: AckType,
        direction: Option<RateDirection>,
    ) -> (RateEvent, Option<RateChangePayload>) {
        let hold_ack_up = self.should_hold_ack_up_without_snr_candidate(ack);
        let mut rate_event = self.decide_rate_change(ack, hold_ack_up);
        // Enforce the profile/host cap and the floor: an AckUp must not raise TX above the cap, and
        // NACK exhaustion must not drop it below the floor (the frame is retried there instead).
        let (capped, floored) = self.enforce_bounds();
        if capped && matches!(rate_event, RateEvent::Increased(_)) {
            rate_event = RateEvent::Maintained;
        }
        if floored
            && matches!(
                rate_event,
                RateEvent::ChirpFallback | RateEvent::NackDecrement(_) | RateEvent::Decreased(_)
            )
        {
            rate_event = RateEvent::Retransmit;
        }
        let speed_level = self
            .rate_adapter
            .as_ref()
            .map(|a| a.tx_level())
            .unwrap_or(SpeedLevel::Sl2);
        let mode = self
            .current_adaptive_mode()
            .unwrap_or("unknown")
            .to_string();
        let payload = if self.rate_adapter.is_some() {
            Some(RateChangePayload {
                event: rate_event,
                speed_level,
                mode,
                direction,
                trigger: None,
            })
        } else {
            None
        };
        (rate_event, payload)
    }

    fn decide_rate_change(&mut self, ack: AckType, hold_ack_up: bool) -> RateEvent {
        // A3 re-upgrade hold + A2 backlog gate: suppress AckUp upgrades that
        // should not act yet, before touching the adapter. Both default off.
        if ack == AckType::AckUp {
            if self.upgrade_hold_remaining > 0 {
                self.upgrade_hold_remaining -= 1;
                return RateEvent::Maintained;
            }
            if self.min_backlog_for_upgrade > 0
                && self.tx_backlog_bytes < self.min_backlog_for_upgrade
            {
                return RateEvent::Maintained;
            }
        }

        let hold_frames = self.upgrade_hold_frames;
        let profile = self.session_profile.clone();
        let event = {
            let Some(adapter) = self.rate_adapter.as_mut() else {
                return RateEvent::Maintained;
            };
            if hold_ack_up {
                RateEvent::Maintained
            } else if ack != AckType::AckUp {
                adapter.apply_ack(ack)
            } else if let Some(profile) = profile.as_ref() {
                let current = adapter.tx_level();
                match Self::next_mapped_level_above(profile, current) {
                    None => RateEvent::Maintained,
                    Some(target) => {
                        let mut last_event = RateEvent::Maintained;
                        while adapter.tx_level() < target {
                            last_event = adapter.apply_ack(AckType::AckUp);
                            if matches!(last_event, RateEvent::Maintained) {
                                break;
                            }
                        }
                        match last_event {
                            RateEvent::Increased(_) => RateEvent::Increased(adapter.tx_level()),
                            other => other,
                        }
                    }
                }
            } else {
                adapter.apply_ack(ack)
            }
        };

        // A3: arm the re-upgrade hold whenever the rate steps down.
        if hold_frames > 0
            && matches!(
                event,
                RateEvent::Decreased(_) | RateEvent::NackDecrement(_) | RateEvent::ChirpFallback
            )
        {
            self.upgrade_hold_remaining = hold_frames;
        }
        event
    }

    fn should_hold_ack_up_without_snr_candidate(&self, ack: AckType) -> bool {
        if ack != AckType::AckUp {
            return false;
        }
        let Some(profile) = self.session_profile.as_ref() else {
            return false;
        };
        let Some(adapter) = self.rate_adapter.as_ref() else {
            return false;
        };
        let tx_level = adapter.tx_level();
        profile.ack_up_requires_snr_candidate_at() == Some(tx_level)
            && !adapter.tx.is_snr_upgrade_candidate()
    }

    fn next_mapped_level_above(
        profile: &SessionProfile,
        current: SpeedLevel,
    ) -> Option<SpeedLevel> {
        let mut probe = current;
        loop {
            let next = probe.step_up();
            if next == probe {
                return None;
            }
            probe = next;
            if profile.mode_for(probe).is_some() {
                return Some(probe);
            }
        }
    }

    pub fn apply_snr_hint(&mut self, snr_db: f32) -> Option<RateChangePayload> {
        let adapter = self.rate_adapter.as_mut()?;
        let profile = self.session_profile.as_ref()?;
        let tx_level = adapter.tx_level();
        let floor_db = profile
            .snr_floor_for_level(tx_level)
            .unwrap_or(f32::NEG_INFINITY);
        let ceiling_db = profile
            .snr_ceiling_for_level(tx_level)
            .unwrap_or(f32::INFINITY);
        let rate_event = adapter.tx.apply_snr_hint(snr_db, floor_db, ceiling_db)?;
        if self.enforce_bounds().1 {
            // The step down would have crossed the floor: no change after all.
            return None;
        }
        let adapter = self.rate_adapter.as_ref()?;
        let profile = self.session_profile.as_ref()?;
        let new_level = adapter.tx_level();
        let mode = profile.mode_for(new_level).unwrap_or("unknown").to_string();
        Some(RateChangePayload {
            event: rate_event,
            speed_level: new_level,
            mode,
            direction: Some(RateDirection::Tx),
            trigger: Some(RateTrigger::SnrFloor),
        })
    }

    pub fn select_rx_ack_type(&mut self, snr_db: f32) -> AckType {
        let Some(adapter) = self.rate_adapter.as_mut() else {
            return AckType::AckOk;
        };
        let Some(profile) = self.session_profile.as_ref() else {
            return AckType::AckOk;
        };
        let rx_level = adapter.rx_level();
        let floor_db = profile
            .snr_floor_for_level(rx_level)
            .unwrap_or(f32::NEG_INFINITY);
        let ceiling_db = profile
            .snr_ceiling_for_level(rx_level)
            .unwrap_or(f32::INFINITY);
        let snr_event = adapter.rx.apply_snr_hint(snr_db, floor_db, ceiling_db);
        let Some(adapter) = self.rate_adapter.as_ref() else {
            return AckType::AckOk;
        };
        let upgrade_candidate = adapter.rx.is_snr_upgrade_candidate();
        self.enforce_bounds();
        if snr_event.is_some() {
            AckType::AckDown
        } else if upgrade_candidate {
            AckType::AckUp
        } else {
            AckType::AckOk
        }
    }

    pub fn record_rx_snr(&mut self, snr_db: f32) {
        self.last_rx_snr_db = Some(snr_db);
    }

    pub fn last_rx_snr_db(&self) -> Option<f32> {
        self.last_rx_snr_db
    }

    pub fn current_adaptive_mode(&self) -> Option<&str> {
        let profile = self.session_profile.as_ref()?;
        let adapter = self.rate_adapter.as_ref()?;
        profile.mode_for(adapter.tx_level())
    }

    pub fn current_rx_mode(&self) -> Option<&str> {
        let profile = self.session_profile.as_ref()?;
        let adapter = self.rate_adapter.as_ref()?;
        profile.mode_for(adapter.rx_level())
    }

    pub fn current_tx_level(&self) -> Option<SpeedLevel> {
        self.rate_adapter.as_ref().map(|a| a.tx_level())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_core::profile::SessionProfile;

    #[test]
    fn apply_ack_without_session_returns_maintained() {
        let mut p = RateAdaptationPolicy::new();
        let (ev, payload) = p.apply_ack(AckType::AckUp);
        assert!(matches!(ev, RateEvent::Maintained));
        assert!(payload.is_none());
    }

    #[test]
    fn start_session_sets_initial_level() {
        let mut p = RateAdaptationPolicy::new();
        let profile = SessionProfile::fast();
        let expected = profile.initial_level;
        p.start_session(profile);
        assert_eq!(p.current_tx_level(), Some(expected));
    }

    #[test]
    fn robust_cap_survives_a_wider_host_cap() {
        let mut p = RateAdaptationPolicy::new();
        p.start_session(SessionProfile::robust());
        // ARQBW sets the host cap on every change; a wide one must not lift the profile cap.
        p.set_max_tx_level(Some(SpeedLevel::Sl14));
        for _ in 0..20 {
            p.apply_ack(AckType::AckUp);
        }
        assert_eq!(p.current_tx_level(), Some(SpeedLevel::Sl6));
        p.set_max_tx_level(None);
        let (ev, _) = p.apply_ack(AckType::AckUp);
        assert!(matches!(ev, RateEvent::Maintained));
        assert_eq!(p.current_tx_level(), Some(SpeedLevel::Sl6));
        assert!(
            p.defined_modes().iter().all(|(l, _)| *l <= SpeedLevel::Sl6),
            "the ARQBW mapping must not see rungs above the cap"
        );
    }

    #[test]
    fn a_narrower_host_cap_still_applies_under_robust() {
        let mut p = RateAdaptationPolicy::new();
        p.start_session(SessionProfile::robust());
        p.set_max_tx_level(Some(SpeedLevel::Sl4));
        for _ in 0..20 {
            p.apply_ack(AckType::AckUp);
        }
        assert_eq!(p.current_tx_level(), Some(SpeedLevel::Sl4));
    }

    #[test]
    fn the_floor_keeps_nack_exhaustion_off_sl1() {
        let mut p = RateAdaptationPolicy::new();
        p.start_session(SessionProfile::fast());
        p.set_min_tx_level(Some(SpeedLevel::Sl2));
        for _ in 0..12 {
            let (ev, _) = p.apply_ack(AckType::Nack);
            assert!(
                !matches!(ev, RateEvent::ChirpFallback),
                "fell to SL1: {ev:?}"
            );
        }
        assert_eq!(p.current_tx_level(), Some(SpeedLevel::Sl2));
        assert!(
            p.apply_snr_hint(-30.0).is_none(),
            "an SNR floor breach at the floor is no change"
        );
        assert_eq!(p.current_tx_level(), Some(SpeedLevel::Sl2));
    }

    #[test]
    fn without_a_floor_nack_exhaustion_reaches_sl1() {
        let mut p = RateAdaptationPolicy::new();
        p.start_session(SessionProfile::fast());
        let fell =
            (0..12).any(|_| matches!(p.apply_ack(AckType::Nack).0, RateEvent::ChirpFallback));
        assert!(
            fell,
            "control: the daemon-side ladder still has its SL1 rung"
        );
    }

    #[test]
    fn select_rx_ack_type_without_session_returns_ok() {
        let mut p = RateAdaptationPolicy::new();
        assert_eq!(p.select_rx_ack_type(10.0), AckType::AckOk);
    }

    #[test]
    fn record_and_read_rx_snr() {
        let mut p = RateAdaptationPolicy::new();
        assert_eq!(p.last_rx_snr_db(), None);
        p.record_rx_snr(12.5);
        assert_eq!(p.last_rx_snr_db(), Some(12.5));
    }

    #[test]
    fn a2_backlog_gate_suppresses_upgrade_until_enough_queued() {
        let mut p = RateAdaptationPolicy::new();
        p.start_session(SessionProfile::fast()); // starts at SL2
        p.set_min_backlog_for_upgrade(64);
        p.set_tx_backlog(10);
        let (ev, _) = p.apply_ack(AckType::AckUp);
        assert!(
            matches!(ev, RateEvent::Maintained),
            "low backlog must not spend upgrade airtime"
        );
        assert_eq!(p.current_tx_level(), Some(SpeedLevel::Sl2));
        // Enough queued now → the upgrade is acted on.
        p.set_tx_backlog(200);
        let (ev, _) = p.apply_ack(AckType::AckUp);
        assert!(matches!(ev, RateEvent::Increased(SpeedLevel::Sl3)));
    }

    #[test]
    fn a3_hold_suppresses_reupgrade_after_downgrade() {
        let mut p = RateAdaptationPolicy::new();
        p.start_session(SessionProfile::fast());
        p.set_upgrade_hold_frames(2);
        // Climb SL2 → SL4.
        p.apply_ack(AckType::AckUp);
        p.apply_ack(AckType::AckUp);
        assert_eq!(p.current_tx_level(), Some(SpeedLevel::Sl4));
        // A downgrade arms the anti-oscillation hold.
        let (ev, _) = p.apply_ack(AckType::AckDown);
        assert!(matches!(ev, RateEvent::Decreased(SpeedLevel::Sl3)));
        // The next two AckUps are suppressed.
        for _ in 0..2 {
            let (ev, _) = p.apply_ack(AckType::AckUp);
            assert!(matches!(ev, RateEvent::Maintained));
            assert_eq!(p.current_tx_level(), Some(SpeedLevel::Sl3));
        }
        // The hold has expired; the third AckUp is acted on.
        let (ev, _) = p.apply_ack(AckType::AckUp);
        assert!(matches!(ev, RateEvent::Increased(SpeedLevel::Sl4)));
    }

    #[test]
    fn gates_off_by_default_allow_immediate_reupgrade() {
        let mut p = RateAdaptationPolicy::new();
        p.start_session(SessionProfile::fast());
        p.apply_ack(AckType::AckUp);
        p.apply_ack(AckType::AckUp); // SL4
        p.apply_ack(AckType::AckDown); // SL3
        let (ev, _) = p.apply_ack(AckType::AckUp); // no hold configured → immediate
        assert!(matches!(ev, RateEvent::Increased(SpeedLevel::Sl4)));
    }
}
