//! The receiver's keyed-NACK budget (#1456).
//!
//! Caps a keyed NACK storm — two OTA-active ends answering each other's ACK or QRM bursts — and a
//! §97 babbling transmitter on repetitive co-channel QRM. Before #1456 the count reset only on a
//! decode, so three failed bursts during idle muted the receiver until the next decode: the peer's
//! first failed frame got silence and its ISS abandoned after two silent windows. The count now also
//! leaks one per `LEAK_SAMPLES` of listening time.

/// Keyed NACKs allowed in a row with no decode and no leak.
pub(crate) const OTA_NACK_BUDGET: u32 = 3;

/// Listening time per leaked NACK: 10 min at 8 kHz. Bounds idle keying at 3 + 6 per hour.
pub(crate) const LEAK_SAMPLES: u64 = 10 * 60 * 8_000;

/// NACKs keyed since the last decode, leaking with listening time.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct NackBudget {
    failures: u32,
    leaked_to: u64,
}

impl NackBudget {
    /// Account one ladder burst heard at `listening` samples; true when its ACK may be keyed.
    pub(crate) fn on_ladder_burst(&mut self, decoded: bool, listening: u64) -> bool {
        let leaks = listening.saturating_sub(self.leaked_to) / LEAK_SAMPLES;
        self.failures = self
            .failures
            .saturating_sub(u32::try_from(leaks).unwrap_or(u32::MAX));
        self.leaked_to += leaks * LEAK_SAMPLES;
        if decoded {
            self.failures = 0;
            self.leaked_to = listening;
            return true;
        }
        if self.failures == 0 {
            self.leaked_to = listening;
        }
        // A failure heard while muted is not added: it keyed nothing, and a debt that outlasts the
        // leak would let steady QRM mute the receiver for good, which is the defect this fixes.
        if self.failures >= OTA_NACK_BUDGET {
            return false;
        }
        self.failures += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_failures_key_and_the_fourth_does_not() {
        let mut b = NackBudget::default();
        let keyed: Vec<bool> = (0..4)
            .map(|i| b.on_ladder_burst(false, i * 8_000))
            .collect();
        assert_eq!(keyed, [true, true, true, false]);
    }

    #[test]
    fn a_decode_resets_the_count() {
        let mut b = NackBudget::default();
        for i in 0..4 {
            b.on_ladder_burst(false, i);
        }
        assert!(b.on_ladder_burst(true, 10));
        assert!(b.on_ladder_burst(false, 11));
    }

    #[test]
    fn listening_time_leaks_one_failure_per_period() {
        let mut b = NackBudget::default();
        for i in 0..4 {
            b.on_ladder_burst(false, i);
        }
        assert!(
            !b.on_ladder_burst(false, LEAK_SAMPLES - 1),
            "not yet leaked"
        );
        assert!(
            b.on_ladder_burst(false, LEAK_SAMPLES + 4),
            "one leak ends the mute"
        );
        assert!(
            !b.on_ladder_burst(false, LEAK_SAMPLES + 5),
            "and allows one NACK"
        );
    }

    #[test]
    fn idle_keying_is_bounded_by_the_leak() {
        let mut b = NackBudget::default();
        let hour = 6 * LEAK_SAMPLES;
        let keyed = (0..hour)
            .step_by(8_000 * 10)
            .filter(|&t| b.on_ladder_burst(false, t))
            .count();
        assert!(
            keyed <= 3 + 6,
            "{keyed} NACKs keyed in an hour of failures every 10 s"
        );
    }
}
