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

// ── SpeedLevel ────────────────────────────────────────────────────────────────

/// HPX adaptive rate speed level (SL1 = slowest / most robust, SL11 = fastest).
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
}

impl SpeedLevel {
    /// Increment by one step, clamping at `Sl11`.
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
            Self::Sl11 => Self::Sl11,
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
        }
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
}

impl RateAdapter {
    /// Create a new adapter starting at `initial` speed level.
    pub fn new(initial: SpeedLevel) -> Self {
        Self {
            current: initial,
            consecutive_nack: 0,
            nack_threshold: 3,
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
    fn ack_up_clamps_at_sl11() {
        let mut a = RateAdapter::new(SpeedLevel::Sl11);
        assert_eq!(a.apply_ack(AckType::AckUp), RateEvent::Maintained);
        assert_eq!(a.speed_level(), SpeedLevel::Sl11);
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
}
