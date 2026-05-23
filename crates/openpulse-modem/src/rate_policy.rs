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
}

impl RateAdaptationPolicy {
    pub fn new() -> Self {
        Self {
            rate_adapter: None,
            session_profile: None,
            last_rx_snr_db: None,
        }
    }

    pub fn start_session(&mut self, profile: SessionProfile) {
        let initial = profile.initial_level;
        let threshold = profile.nack_threshold;
        self.rate_adapter = Some(BiDirRateAdapter::new(initial, threshold));
        self.session_profile = Some(profile);
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
        let rate_event = self.decide_rate_change(ack, hold_ack_up);
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
        let profile = self.session_profile.clone();
        let Some(adapter) = self.rate_adapter.as_mut() else {
            return RateEvent::Maintained;
        };
        if hold_ack_up {
            return RateEvent::Maintained;
        }
        if ack != AckType::AckUp {
            return adapter.apply_ack(ack);
        }
        let Some(profile) = profile.as_ref() else {
            return adapter.apply_ack(ack);
        };
        let current = adapter.tx_level();
        let Some(target) = Self::next_mapped_level_above(profile, current) else {
            return RateEvent::Maintained;
        };
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
        if snr_event.is_some() {
            AckType::AckDown
        } else if adapter.rx.is_snr_upgrade_candidate() {
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

    /// Estimate receive-path SNR (dB) from LLR magnitudes.
    ///
    /// Uses `mean(|llr|) / 2` as a linear SNR proxy; clamps to [-5, 40] dB.
    pub fn snr_from_llrs(llrs: &[f32]) -> f32 {
        if llrs.is_empty() {
            return 0.0;
        }
        let mean_abs = llrs.iter().map(|l| l.abs()).sum::<f32>() / llrs.len() as f32;
        (10.0 * (mean_abs / 2.0).max(1e-6).log10()).clamp(-5.0, 40.0)
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
        let profile = SessionProfile::hpx500();
        let expected = profile.initial_level;
        p.start_session(profile);
        assert_eq!(p.current_tx_level(), Some(expected));
    }

    #[test]
    fn ack_up_skips_unmapped_levels() {
        // hpx_wideband: SL8=QPSK500, SL9=QPSK1000, (SL10 unmapped), SL11=8PSK1000.
        // Starting at SL9, AckUp must jump to SL11 — proving gap-skipping.
        let mut p = RateAdaptationPolicy::new();
        p.start_session(SessionProfile::hpx_wideband());
        // Advance from SL8 to SL9 first.
        let (_ev, _payload) = p.apply_ack(AckType::AckUp);
        assert_eq!(p.current_tx_level(), Some(SpeedLevel::Sl9));
        // Next AckUp must skip SL10 and land on SL11.
        let (ev, _payload) = p.apply_ack(AckType::AckUp);
        assert_eq!(p.current_tx_level(), Some(SpeedLevel::Sl11));
        assert!(matches!(ev, RateEvent::Increased(SpeedLevel::Sl11)));
    }

    #[test]
    fn select_rx_ack_type_without_session_returns_ok() {
        let mut p = RateAdaptationPolicy::new();
        assert_eq!(p.select_rx_ack_type(10.0), AckType::AckOk);
    }

    #[test]
    fn snr_from_llrs_empty_is_zero() {
        assert_eq!(RateAdaptationPolicy::snr_from_llrs(&[]), 0.0);
    }

    #[test]
    fn snr_from_llrs_clamps_range() {
        // Tiny LLRs floor at -5 dB.
        let lo = RateAdaptationPolicy::snr_from_llrs(&[0.0; 16]);
        assert!(lo >= -5.0 - 1e-3 && lo <= -5.0 + 1e-3);
        // Huge LLRs ceiling at 40 dB.
        let hi = RateAdaptationPolicy::snr_from_llrs(&[1e9; 4]);
        assert!((hi - 40.0).abs() < 1e-3);
    }

    #[test]
    fn record_and_read_rx_snr() {
        let mut p = RateAdaptationPolicy::new();
        assert_eq!(p.last_rx_snr_db(), None);
        p.record_rx_snr(12.5);
        assert_eq!(p.last_rx_snr_db(), Some(12.5));
    }
}
