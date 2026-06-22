//! HPX rate adaptation state machine.
//!
//! The ISS (Information Sending Station) maintains one [`RateAdapter`] per
//! session.  After each received ACK, call [`RateAdapter::apply_ack`] to
//! advance the rate state and obtain the [`RateEvent`] describing the action
//! to take.
//!
//! ## Speed levels
//!
//! | SL  | HPX profile | Mode (example mapping) |
//! |-----|-------------|------------------------|
//! | SL1 | Fallback    | Chirp / minimum rate   |
//! | SL2 | HPX500      | BPSK31                 |
//! | SL3 | HPX500      | BPSK63                 |
//! | SL4 | HPX500      | BPSK250                |
//! | SL5 | HPX500      | QPSK250                |
//! | SL6 | HPX500      | QPSK500                |
//! | SL7 | HPX500      | (reserved)             |
//! | SL8 | HPX2300     | QPSK500                |
//! | SL9 | HPX2300     | QPSK1000               |
//! | SL10| HPX2300     | (reserved)             |
//! | SL11| HPX2300     | 8PSK / maximum rate    |
//!
//! Exact per-level mode strings are assigned by the session profile in
//! Phase 2.2.  The numeric ordering is what matters for rate-step logic.

use serde::{Deserialize, Serialize};

use crate::ack::AckType;

// ── RateTrigger ───────────────────────────────────────────────────────────────

/// What triggered a [`RateEvent`] in the rate adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateTrigger {
    AckUp,
    AckDown,
    NackDecrement,
    ChirpFallback,
    /// SNR dropped below the per-level floor; proactive step-down fired.
    SnrFloor,
    /// SNR rose above the per-level ceiling; upgrade candidate flagged.
    SnrCeiling,
}

// ── SpeedLevel ────────────────────────────────────────────────────────────────

/// HPX adaptive rate speed level (SL1 = slowest / most robust, SL20 = fastest).
///
/// | SL  | HPX profile    | Example mode         |
/// |-----|----------------|----------------------|
/// | SL1 | Fallback       | Chirp / minimum rate |
/// | SL2–SL7  | HPX500/HF  | BPSK31 … 8PSK500    |
/// | SL8–SL11 | HPX2300    | QPSK500 … 8PSK1000  |
/// | SL12–SL14 | Wideband HD| 64QAM500/1000/2000 |
/// | SL15–SL20 | Reserved   | (future expansion)  |
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum SpeedLevel {
    Sl1 = 1,
    Sl2 = 2,
    Sl3 = 3,
    Sl4 = 4,
    Sl5 = 5,
    Sl6 = 6,
    Sl7 = 7,
    Sl8 = 8,
    Sl9 = 9,
    Sl10 = 10,
    Sl11 = 11,
    Sl12 = 12,
    Sl13 = 13,
    Sl14 = 14,
    Sl15 = 15,
    Sl16 = 16,
    Sl17 = 17,
    Sl18 = 18,
    Sl19 = 19,
    Sl20 = 20,
}

impl SpeedLevel {
    /// Increment by one step, clamping at `Sl20`.
    pub fn step_up(self) -> Self {
        match self {
            Self::Sl1 => Self::Sl2,
            Self::Sl2 => Self::Sl3,
            Self::Sl3 => Self::Sl4,
            Self::Sl4 => Self::Sl5,
            Self::Sl5 => Self::Sl6,
            Self::Sl6 => Self::Sl7,
            Self::Sl7 => Self::Sl8,
            Self::Sl8 => Self::Sl9,
            Self::Sl9 => Self::Sl10,
            Self::Sl10 => Self::Sl11,
            Self::Sl11 => Self::Sl12,
            Self::Sl12 => Self::Sl13,
            Self::Sl13 => Self::Sl14,
            Self::Sl14 => Self::Sl15,
            Self::Sl15 => Self::Sl16,
            Self::Sl16 => Self::Sl17,
            Self::Sl17 => Self::Sl18,
            Self::Sl18 => Self::Sl19,
            Self::Sl19 => Self::Sl20,
            Self::Sl20 => Self::Sl20,
        }
    }

    /// Decrement by one step, clamping at `Sl1`.
    pub fn step_down(self) -> Self {
        match self {
            Self::Sl1 => Self::Sl1,
            Self::Sl2 => Self::Sl1,
            Self::Sl3 => Self::Sl2,
            Self::Sl4 => Self::Sl3,
            Self::Sl5 => Self::Sl4,
            Self::Sl6 => Self::Sl5,
            Self::Sl7 => Self::Sl6,
            Self::Sl8 => Self::Sl7,
            Self::Sl9 => Self::Sl8,
            Self::Sl10 => Self::Sl9,
            Self::Sl11 => Self::Sl10,
            Self::Sl12 => Self::Sl11,
            Self::Sl13 => Self::Sl12,
            Self::Sl14 => Self::Sl13,
            Self::Sl15 => Self::Sl14,
            Self::Sl16 => Self::Sl15,
            Self::Sl17 => Self::Sl16,
            Self::Sl18 => Self::Sl17,
            Self::Sl19 => Self::Sl18,
            Self::Sl20 => Self::Sl19,
        }
    }

    /// Wire code (1–20). Inverse of [`SpeedLevel::from_u8`].
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Human/config name, e.g. `"SL8"`.
    pub fn name(self) -> String {
        format!("SL{}", self as u8)
    }

    /// Parse a name like `"SL8"` / `"sl8"` / `"8"` into a [`SpeedLevel`].
    pub fn from_name(s: &str) -> Option<Self> {
        let t = s.trim();
        let digits = t
            .strip_prefix("SL")
            .or_else(|| t.strip_prefix("sl"))
            .unwrap_or(t);
        digits.parse::<u8>().ok().and_then(Self::from_u8)
    }

    /// Decode a wire code (1–20) into a [`SpeedLevel`]; `None` if out of range.
    pub fn from_u8(v: u8) -> Option<Self> {
        use SpeedLevel::*;
        Some(match v {
            1 => Sl1,
            2 => Sl2,
            3 => Sl3,
            4 => Sl4,
            5 => Sl5,
            6 => Sl6,
            7 => Sl7,
            8 => Sl8,
            9 => Sl9,
            10 => Sl10,
            11 => Sl11,
            12 => Sl12,
            13 => Sl13,
            14 => Sl14,
            15 => Sl15,
            16 => Sl16,
            17 => Sl17,
            18 => Sl18,
            19 => Sl19,
            20 => Sl20,
            _ => return None,
        })
    }
}

// ── RateEvent ─────────────────────────────────────────────────────────────────

/// Outcome of a single [`RateAdapter::apply_ack`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RateEvent {
    /// Speed level is unchanged.
    Maintained,
    /// Speed level stepped up to the enclosed level.
    Increased(SpeedLevel),
    /// Speed level stepped down to the enclosed level.
    Decreased(SpeedLevel),
    /// NACK received below threshold; caller must retransmit at current level.
    Retransmit,
    /// NACK threshold exhausted; speed level decremented to the enclosed level.
    NackDecrement(SpeedLevel),
    /// Three consecutive NACKs at SL2 — fell back to SL1 (chirp mode).
    ChirpFallback,
    /// IRS requests ISS/IRS role swap.
    BreakRequested,
    /// Remote requests retransmission of the last data frame.
    Req,
    /// Graceful session end requested by remote.
    Qrt,
    /// Abnormal teardown requested by remote.
    Abort,
}

// ── RateAdapter ───────────────────────────────────────────────────────────────

/// Tracks the HPX speed level and responds to received ACK types.
///
/// Create one per session on the ISS side.  Feed each received [`AckType`] to
/// [`RateAdapter::apply_ack`] and act on the returned [`RateEvent`].
pub struct RateAdapter {
    current: SpeedLevel,
    consecutive_nack: u8,
    /// Number of consecutive NACKs that triggers a speed-level decrease.
    /// Default: 3.
    pub nack_threshold: u8,
    /// Set when SNR crosses the per-level ceiling; cleared on the next ACK-UP step.
    snr_upgrade_candidate: bool,
}

impl RateAdapter {
    /// Create a new adapter starting at `initial` speed level.
    pub fn new(initial: SpeedLevel) -> Self {
        Self {
            current: initial,
            consecutive_nack: 0,
            nack_threshold: 3,
            snr_upgrade_candidate: false,
        }
    }

    /// Current speed level.
    pub fn speed_level(&self) -> SpeedLevel {
        self.current
    }

    /// Apply a received ACK type and return the required action.
    ///
    /// Per the HPX rate adaptation protocol:
    /// - `AckOk`   → maintain SL; reset NACK counter.
    /// - `AckUp`   → step SL up (clamp at SL11).
    /// - `AckDown` → step SL down (floor at SL2; SL1 only via NACK exhaustion).
    /// - `Nack`    → retransmit; after `nack_threshold` consecutive NACKs,
    ///   decrement SL (or fall back to SL1 if currently at SL2).
    pub fn apply_ack(&mut self, ack: AckType) -> RateEvent {
        match ack {
            AckType::AckOk => {
                self.consecutive_nack = 0;
                RateEvent::Maintained
            }
            AckType::AckUp => {
                self.consecutive_nack = 0;
                self.snr_upgrade_candidate = false;
                let next = self.current.step_up();
                if next != self.current {
                    self.current = next;
                    RateEvent::Increased(self.current)
                } else {
                    RateEvent::Maintained
                }
            }
            AckType::AckDown => {
                self.consecutive_nack = 0;
                // SL1 (chirp) is only reached via NACK exhaustion, not ACK-DOWN.
                if self.current > SpeedLevel::Sl2 {
                    self.current = self.current.step_down();
                    RateEvent::Decreased(self.current)
                } else {
                    RateEvent::Maintained
                }
            }
            AckType::Nack => {
                self.consecutive_nack = self.consecutive_nack.saturating_add(1);
                if self.consecutive_nack < self.nack_threshold {
                    RateEvent::Retransmit
                } else {
                    self.consecutive_nack = 0;
                    if self.current == SpeedLevel::Sl2 {
                        self.current = SpeedLevel::Sl1;
                        RateEvent::ChirpFallback
                    } else if self.current > SpeedLevel::Sl1 {
                        self.current = self.current.step_down();
                        RateEvent::NackDecrement(self.current)
                    } else {
                        // Already at SL1; can't go lower.
                        RateEvent::Retransmit
                    }
                }
            }
            AckType::Break => RateEvent::BreakRequested,
            AckType::Req => RateEvent::Req,
            AckType::Qrt => RateEvent::Qrt,
            AckType::Abort => RateEvent::Abort,
        }
    }

    /// Reset the NACK counter (call after a successful retransmit acknowledged).
    pub fn reset_nack_counter(&mut self) {
        self.consecutive_nack = 0;
    }

    /// Apply a raw SNR hint for proactive rate adaptation.
    ///
    /// If `snr_db < floor_db` the adapter steps down immediately (before any NACK)
    /// and returns `Some(event)`.  If `snr_db > ceiling_db` the upgrade-candidate
    /// flag is set (the next [`apply_ack`](Self::apply_ack) call with `AckUp` will
    /// clear it) and `None` is returned — no immediate level change.  Otherwise
    /// `None` is returned.
    ///
    /// Pass `floor_db = f32::NEG_INFINITY` and/or `ceiling_db = f32::INFINITY` to
    /// disable the respective check for a given level.
    pub fn apply_snr_hint(
        &mut self,
        snr_db: f32,
        floor_db: f32,
        ceiling_db: f32,
    ) -> Option<RateEvent> {
        if snr_db < floor_db {
            self.consecutive_nack = 0;
            self.snr_upgrade_candidate = false;
            if self.current == SpeedLevel::Sl2 {
                self.current = SpeedLevel::Sl1;
                Some(RateEvent::ChirpFallback)
            } else if self.current > SpeedLevel::Sl1 {
                self.current = self.current.step_down();
                Some(RateEvent::Decreased(self.current))
            } else {
                None
            }
        } else if snr_db > ceiling_db {
            self.snr_upgrade_candidate = true;
            None
        } else {
            None
        }
    }

    /// Returns `true` if the SNR ceiling has been crossed and an upgrade is ready.
    pub fn is_snr_upgrade_candidate(&self) -> bool {
        self.snr_upgrade_candidate
    }
}

// ── BiDirRateAdapter ──────────────────────────────────────────────────────────

/// Bidirectional rate adapter: independent TX and RX speed-level tracking.
///
/// On HF, SNR is rarely symmetric.  A→B and B→A paths each adapt independently
/// based on their own quality feedback.  `tx` tracks our outgoing path quality
/// (as reported by the peer via `AckType`); `rx` tracks the incoming path quality
/// (as reported by the peer via `AckFrame::reverse_ack`).
pub struct BiDirRateAdapter {
    /// Outgoing path (our TX → peer RX) rate adapter.
    pub tx: RateAdapter,
    /// Incoming path (peer TX → our RX) rate adapter.
    pub rx: RateAdapter,
}

impl BiDirRateAdapter {
    /// Create a new bidirectional adapter with both directions starting at `initial`.
    pub fn new(initial: SpeedLevel, nack_threshold: u8) -> Self {
        let make = |level| {
            let mut a = RateAdapter::new(level);
            a.nack_threshold = nack_threshold;
            a
        };
        Self {
            tx: make(initial),
            rx: make(initial),
        }
    }

    /// Apply the peer's assessment of *our* TX path and return the rate event.
    pub fn apply_ack(&mut self, ack: AckType) -> RateEvent {
        self.tx.apply_ack(ack)
    }

    /// Apply the peer's self-reported RX quality (from `AckFrame::reverse_ack`).
    pub fn apply_reverse_ack(&mut self, ack: AckType) -> RateEvent {
        self.rx.apply_ack(ack)
    }

    /// Current TX speed level.
    pub fn tx_level(&self) -> SpeedLevel {
        self.tx.speed_level()
    }

    /// Current RX speed level.
    pub fn rx_level(&self) -> SpeedLevel {
        self.rx.speed_level()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ack_up_increases_speed_level() {
        let mut a = RateAdapter::new(SpeedLevel::Sl3);
        assert_eq!(
            a.apply_ack(AckType::AckUp),
            RateEvent::Increased(SpeedLevel::Sl4)
        );
        assert_eq!(a.speed_level(), SpeedLevel::Sl4);
    }

    #[test]
    fn ack_up_clamps_at_sl20() {
        let mut a = RateAdapter::new(SpeedLevel::Sl20);
        assert_eq!(a.apply_ack(AckType::AckUp), RateEvent::Maintained);
        assert_eq!(a.speed_level(), SpeedLevel::Sl20);
    }

    #[test]
    fn ack_down_decreases_speed_level() {
        let mut a = RateAdapter::new(SpeedLevel::Sl5);
        assert_eq!(
            a.apply_ack(AckType::AckDown),
            RateEvent::Decreased(SpeedLevel::Sl4)
        );
    }

    #[test]
    fn ack_down_stops_at_sl2() {
        let mut a = RateAdapter::new(SpeedLevel::Sl2);
        assert_eq!(a.apply_ack(AckType::AckDown), RateEvent::Maintained);
        assert_eq!(a.speed_level(), SpeedLevel::Sl2);
    }

    #[test]
    fn ack_ok_resets_nack_counter() {
        let mut a = RateAdapter::new(SpeedLevel::Sl4);
        a.apply_ack(AckType::Nack);
        a.apply_ack(AckType::Nack);
        a.apply_ack(AckType::AckOk);
        // Counter reset: two more NACKs should not yet trigger decrement.
        assert_eq!(a.apply_ack(AckType::Nack), RateEvent::Retransmit);
        assert_eq!(a.apply_ack(AckType::Nack), RateEvent::Retransmit);
        assert!(matches!(
            a.apply_ack(AckType::Nack),
            RateEvent::NackDecrement(_)
        ));
    }

    #[test]
    fn three_consecutive_nack_decrements_sl() {
        let mut a = RateAdapter::new(SpeedLevel::Sl5);
        assert_eq!(a.apply_ack(AckType::Nack), RateEvent::Retransmit);
        assert_eq!(a.apply_ack(AckType::Nack), RateEvent::Retransmit);
        assert_eq!(
            a.apply_ack(AckType::Nack),
            RateEvent::NackDecrement(SpeedLevel::Sl4)
        );
        assert_eq!(a.speed_level(), SpeedLevel::Sl4);
    }

    #[test]
    fn nack_exhaustion_at_sl2_triggers_chirp_fallback() {
        let mut a = RateAdapter::new(SpeedLevel::Sl2);
        a.apply_ack(AckType::Nack);
        a.apply_ack(AckType::Nack);
        assert_eq!(a.apply_ack(AckType::Nack), RateEvent::ChirpFallback);
        assert_eq!(a.speed_level(), SpeedLevel::Sl1);
    }

    #[test]
    fn nack_at_sl1_retransmits_without_further_decrease() {
        let mut a = RateAdapter::new(SpeedLevel::Sl1);
        for _ in 0..10 {
            let ev = a.apply_ack(AckType::Nack);
            assert!(
                matches!(ev, RateEvent::Retransmit),
                "expected Retransmit at SL1, got {ev:?}"
            );
            assert_eq!(a.speed_level(), SpeedLevel::Sl1);
        }
    }

    #[test]
    fn control_ack_types_pass_through_unchanged() {
        let mut a = RateAdapter::new(SpeedLevel::Sl4);
        assert_eq!(a.apply_ack(AckType::Break), RateEvent::BreakRequested);
        assert_eq!(a.apply_ack(AckType::Req), RateEvent::Req);
        assert_eq!(a.apply_ack(AckType::Qrt), RateEvent::Qrt);
        assert_eq!(a.apply_ack(AckType::Abort), RateEvent::Abort);
        assert_eq!(a.speed_level(), SpeedLevel::Sl4);
    }

    #[test]
    fn bidir_tx_and_rx_adapt_independently() {
        let mut bd = BiDirRateAdapter::new(SpeedLevel::Sl5, 3);
        // TX path gets NACKs → steps down.
        bd.apply_ack(AckType::Nack);
        bd.apply_ack(AckType::Nack);
        bd.apply_ack(AckType::Nack);
        // RX path gets ACK-UP → steps up.
        bd.apply_reverse_ack(AckType::AckUp);
        bd.apply_reverse_ack(AckType::AckUp);
        assert!(bd.tx_level() < bd.rx_level(), "TX should be lower than RX");
    }

    #[test]
    fn bidir_tx_nack_does_not_affect_rx() {
        let mut bd = BiDirRateAdapter::new(SpeedLevel::Sl6, 3);
        bd.apply_ack(AckType::Nack);
        bd.apply_ack(AckType::Nack);
        bd.apply_ack(AckType::Nack);
        assert_eq!(
            bd.rx_level(),
            SpeedLevel::Sl6,
            "RX should be unaffected by TX NACKs"
        );
    }
}
