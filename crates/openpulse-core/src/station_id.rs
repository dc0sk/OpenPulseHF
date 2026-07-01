//! Periodic station-identification timer (REQ-REG-10).

/// Decides when a station must next transmit its identification.
///
/// Regulatory rules (e.g. FCC §97.119(a): identify at least every 10 minutes
/// *while transmitting*, and at the end of the exchange) require ID only when the
/// station has actually transmitted — a pure-receive station need not key up. This
/// timer captures exactly that: it fires when it is enabled, the station has
/// transmitted since the last ID, and at least `interval_ms` has elapsed.
///
/// Pure state machine over an injected `now_ms` clock so it is deterministic and
/// unit-testable; the daemon feeds it a monotonic millisecond timestamp and polls
/// [`id_due`](Self::id_due) each tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationIdTimer {
    interval_ms: u64,
    last_id_ms: u64,
    tx_since_id: bool,
}

impl StationIdTimer {
    /// `interval_ms == 0` disables the timer entirely (no auto-ID). `now_ms` seeds
    /// the interval clock so the first ID is not due until `interval_ms` later.
    pub fn new(interval_ms: u64, now_ms: u64) -> Self {
        Self {
            interval_ms,
            last_id_ms: now_ms,
            tx_since_id: false,
        }
    }

    /// Whether auto-ID is active (non-zero interval).
    pub fn is_enabled(&self) -> bool {
        self.interval_ms > 0
    }

    /// Record that the station transmitted (data / ACK / retransmit / …) — this
    /// arms the timer so the next elapsed interval triggers an ID. No-op when
    /// disabled. The station's own ID frame must NOT be reported here (else an
    /// otherwise-idle station would re-ID forever); see the daemon wiring.
    pub fn note_tx(&mut self) {
        if self.is_enabled() {
            self.tx_since_id = true;
        }
    }

    /// True when an ID is due: enabled, armed by TX since the last ID, and at
    /// least `interval_ms` elapsed since the last ID.
    pub fn id_due(&self, now_ms: u64) -> bool {
        self.is_enabled()
            && self.tx_since_id
            && now_ms.saturating_sub(self.last_id_ms) >= self.interval_ms
    }

    /// Record that an ID was just transmitted: restart the interval and disarm
    /// until the station transmits again.
    pub fn mark_identified(&mut self, now_ms: u64) {
        self.last_id_ms = now_ms;
        self.tx_since_id = false;
    }

    /// Milliseconds until the next ID is due, or `None` when disabled or unarmed
    /// (no TX since the last ID). Returns `Some(0)` when already due.
    pub fn ms_until_due(&self, now_ms: u64) -> Option<u64> {
        if !self.is_enabled() || !self.tx_since_id {
            return None;
        }
        Some(
            self.interval_ms
                .saturating_sub(now_ms.saturating_sub(self.last_id_ms)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEN_MIN: u64 = 600_000;

    #[test]
    fn zero_interval_is_disabled_and_never_due() {
        let mut t = StationIdTimer::new(0, 0);
        assert!(!t.is_enabled());
        t.note_tx();
        assert!(!t.id_due(u64::MAX), "disabled timer must never be due");
        assert_eq!(t.ms_until_due(u64::MAX), None);
    }

    #[test]
    fn not_due_without_any_transmission() {
        // A pure-receive station: interval elapses but nothing was sent → no ID.
        let t = StationIdTimer::new(TEN_MIN, 0);
        assert!(
            !t.id_due(TEN_MIN * 5),
            "must not ID if we never transmitted"
        );
        assert_eq!(t.ms_until_due(TEN_MIN * 5), None);
    }

    #[test]
    fn not_due_before_interval_even_after_tx() {
        let mut t = StationIdTimer::new(TEN_MIN, 0);
        t.note_tx();
        assert!(!t.id_due(TEN_MIN - 1), "not due one ms early");
        assert_eq!(t.ms_until_due(TEN_MIN - 1), Some(1));
    }

    #[test]
    fn due_at_interval_after_tx() {
        let mut t = StationIdTimer::new(TEN_MIN, 0);
        t.note_tx();
        assert!(t.id_due(TEN_MIN), "due exactly at the interval");
        assert!(t.id_due(TEN_MIN * 3), "stays due past the interval");
        assert_eq!(t.ms_until_due(TEN_MIN), Some(0));
    }

    #[test]
    fn mark_identified_resets_and_disarms() {
        let mut t = StationIdTimer::new(TEN_MIN, 0);
        t.note_tx();
        assert!(t.id_due(TEN_MIN));
        t.mark_identified(TEN_MIN);
        // Interval restarts and the timer is disarmed until the next TX.
        assert!(!t.id_due(TEN_MIN), "not due immediately after IDing");
        assert!(
            !t.id_due(TEN_MIN * 2),
            "not due after another interval with no further TX"
        );
        assert_eq!(t.ms_until_due(TEN_MIN * 2), None);
    }

    #[test]
    fn rearms_and_is_due_again_after_next_interval() {
        let mut t = StationIdTimer::new(TEN_MIN, 0);
        t.note_tx();
        t.mark_identified(TEN_MIN);
        // Station keeps transmitting; a full interval later a second ID is due.
        t.note_tx();
        assert!(
            !t.id_due(TEN_MIN + TEN_MIN - 1),
            "not due before the 2nd interval"
        );
        assert!(
            t.id_due(TEN_MIN * 2),
            "second ID due one interval after the first"
        );
    }

    #[test]
    fn repeated_tx_within_an_interval_does_not_advance_the_deadline() {
        // Many transmissions still yield exactly one ID per interval.
        let mut t = StationIdTimer::new(TEN_MIN, 0);
        for ms in [10, 100, 1_000, 300_000] {
            let _ = ms; // TX times within the interval
            t.note_tx();
        }
        assert!(!t.id_due(TEN_MIN - 1));
        assert!(t.id_due(TEN_MIN));
    }
}
