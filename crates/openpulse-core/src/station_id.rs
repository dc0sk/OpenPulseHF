//! Periodic + end-of-exchange station-identification timer (REQ-REG-10).

/// Decides when a station must next transmit its identification.
///
/// Regulatory rules (e.g. FCC §97.119(a)) require a station to identify **at least
/// every 10 minutes during** a communication **and at the end** of it — and only
/// when it has actually transmitted (a pure-receive station need not key up). This
/// timer captures both triggers:
///
/// - **Interval ID** ([`id_due`](Self::id_due)): enabled, armed by TX since the last
///   ID, and `interval_ms` elapsed since the last ID.
/// - **End-of-exchange (sign-off) ID** ([`signoff_due`](Self::signoff_due)): enabled
///   with a non-zero sign-off idle, armed by TX since the last ID, and the channel has
///   been quiet (no TX) for `signoff_idle_ms` — i.e. the exchange has wound down.
///
/// Both share one `mark_identified` reset, so after any ID neither fires again until
/// the station transmits anew. Pure state machine over an injected `now_ms` clock so
/// it is deterministic and unit-testable; the daemon feeds it a monotonic millisecond
/// timestamp and polls each tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationIdTimer {
    interval_ms: u64,
    signoff_idle_ms: u64,
    last_id_ms: u64,
    last_tx_ms: u64,
    tx_since_id: bool,
}

impl StationIdTimer {
    /// `interval_ms == 0` disables the timer entirely (no auto-ID of either kind).
    /// `now_ms` seeds the clock so the first interval ID is not due until `interval_ms`
    /// later. Sign-off ID is off until enabled via [`with_signoff_idle_ms`](Self::with_signoff_idle_ms).
    pub fn new(interval_ms: u64, now_ms: u64) -> Self {
        Self {
            interval_ms,
            signoff_idle_ms: 0,
            last_id_ms: now_ms,
            last_tx_ms: now_ms,
            tx_since_id: false,
        }
    }

    /// Enable the end-of-exchange (sign-off) ID: once the station has transmitted, a
    /// final ID is due after `signoff_idle_ms` of no further TX. `0` leaves it disabled
    /// (interval ID only). Ignored unless the timer itself is enabled (`interval_ms > 0`).
    pub fn with_signoff_idle_ms(mut self, signoff_idle_ms: u64) -> Self {
        self.signoff_idle_ms = signoff_idle_ms;
        self
    }

    /// Whether auto-ID is active at all (non-zero interval). Gates both triggers.
    pub fn is_enabled(&self) -> bool {
        self.interval_ms > 0
    }

    /// Record that the station transmitted (data / ACK / retransmit / …) at `now_ms` —
    /// arms both triggers and stamps the last-TX time the sign-off idle measures from.
    /// No-op when disabled. The station's own ID frame must NOT be reported here (else an
    /// otherwise-idle station would re-ID forever); see the daemon wiring.
    pub fn note_tx(&mut self, now_ms: u64) {
        if self.is_enabled() {
            self.tx_since_id = true;
            self.last_tx_ms = now_ms;
        }
    }

    /// True when an **interval** ID is due: enabled, armed by TX since the last ID, and
    /// at least `interval_ms` elapsed since the last ID.
    pub fn id_due(&self, now_ms: u64) -> bool {
        self.is_enabled()
            && self.tx_since_id
            && now_ms.saturating_sub(self.last_id_ms) >= self.interval_ms
    }

    /// True when an **end-of-exchange (sign-off)** ID is due: enabled, sign-off configured,
    /// armed by TX since the last ID, and the channel has been quiet for `signoff_idle_ms`
    /// since the last TX (the exchange has wound down).
    pub fn signoff_due(&self, now_ms: u64) -> bool {
        self.is_enabled()
            && self.signoff_idle_ms > 0
            && self.tx_since_id
            && now_ms.saturating_sub(self.last_tx_ms) >= self.signoff_idle_ms
    }

    /// Record that an ID was just transmitted (either kind): restart the interval and
    /// disarm both triggers until the station transmits again.
    pub fn mark_identified(&mut self, now_ms: u64) {
        self.last_id_ms = now_ms;
        self.tx_since_id = false;
    }

    /// Milliseconds until the next **interval** ID is due, or `None` when disabled or
    /// unarmed (no TX since the last ID). Returns `Some(0)` when already due.
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
    const SIGNOFF: u64 = 10_000; // 10 s idle

    fn timer() -> StationIdTimer {
        StationIdTimer::new(TEN_MIN, 0).with_signoff_idle_ms(SIGNOFF)
    }

    #[test]
    fn zero_interval_is_disabled_and_never_due() {
        let mut t = StationIdTimer::new(0, 0).with_signoff_idle_ms(SIGNOFF);
        assert!(!t.is_enabled());
        t.note_tx(0);
        assert!(!t.id_due(u64::MAX), "disabled timer must never interval-ID");
        assert!(
            !t.signoff_due(u64::MAX),
            "disabled timer must never sign-off"
        );
        assert_eq!(t.ms_until_due(u64::MAX), None);
    }

    #[test]
    fn not_due_without_any_transmission() {
        // A pure-receive station: interval elapses but nothing was sent → no ID of either kind.
        let t = timer();
        assert!(
            !t.id_due(TEN_MIN * 5),
            "must not interval-ID if we never transmitted"
        );
        assert!(
            !t.signoff_due(TEN_MIN * 5),
            "must not sign-off if we never transmitted"
        );
        assert_eq!(t.ms_until_due(TEN_MIN * 5), None);
    }

    #[test]
    fn not_due_before_interval_even_after_tx() {
        let mut t = timer();
        t.note_tx(0);
        assert!(!t.id_due(TEN_MIN - 1), "interval ID not due one ms early");
        assert_eq!(t.ms_until_due(TEN_MIN - 1), Some(1));
    }

    #[test]
    fn due_at_interval_after_tx() {
        let mut t = timer();
        t.note_tx(0);
        assert!(t.id_due(TEN_MIN), "interval ID due exactly at the interval");
        assert!(t.id_due(TEN_MIN * 3), "stays due past the interval");
        assert_eq!(t.ms_until_due(TEN_MIN), Some(0));
    }

    #[test]
    fn mark_identified_resets_and_disarms_both() {
        let mut t = timer();
        t.note_tx(0);
        assert!(t.id_due(TEN_MIN));
        t.mark_identified(TEN_MIN);
        assert!(
            !t.id_due(TEN_MIN),
            "interval not due immediately after IDing"
        );
        assert!(
            !t.id_due(TEN_MIN * 2),
            "not due after another interval with no further TX"
        );
        assert!(
            !t.signoff_due(TEN_MIN * 2),
            "sign-off disarmed after IDing until the next TX"
        );
        assert_eq!(t.ms_until_due(TEN_MIN * 2), None);
    }

    #[test]
    fn rearms_and_is_due_again_after_next_interval() {
        let mut t = timer();
        t.note_tx(0);
        t.mark_identified(TEN_MIN);
        // Station keeps transmitting; a full interval later a second interval ID is due.
        t.note_tx(TEN_MIN);
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
    fn repeated_tx_within_an_interval_does_not_advance_the_interval_deadline() {
        // Many transmissions still yield exactly one interval ID per interval.
        let mut t = timer();
        for ms in [10, 100, 1_000, 300_000] {
            t.note_tx(ms);
        }
        assert!(!t.id_due(TEN_MIN - 1));
        assert!(t.id_due(TEN_MIN));
    }

    // ── End-of-exchange (sign-off) ID ────────────────────────────────────────────

    #[test]
    fn signoff_disabled_when_idle_is_zero() {
        let mut t = StationIdTimer::new(TEN_MIN, 0); // no with_signoff_idle_ms → 0
        t.note_tx(0);
        assert!(
            !t.signoff_due(u64::MAX),
            "sign-off must never fire when the idle is 0 (interval-only)"
        );
    }

    #[test]
    fn signoff_due_after_idle_following_tx() {
        let mut t = timer();
        t.note_tx(5_000); // transmitted at t=5s
        assert!(
            !t.signoff_due(5_000 + SIGNOFF - 1),
            "not due one ms before the idle elapses"
        );
        assert!(
            t.signoff_due(5_000 + SIGNOFF),
            "sign-off due once quiet for the idle period"
        );
    }

    #[test]
    fn later_tx_pushes_the_signoff_deadline_out() {
        // A fresh transmission mid-idle restarts the quiet window (still one exchange).
        let mut t = timer();
        t.note_tx(0);
        assert!(!t.signoff_due(SIGNOFF - 1));
        t.note_tx(SIGNOFF - 1); // more traffic before it went quiet
        assert!(
            !t.signoff_due(SIGNOFF),
            "deadline measured from the latest TX, not the first"
        );
        assert!(
            t.signoff_due((SIGNOFF - 1) + SIGNOFF),
            "due an idle-period after the last TX"
        );
    }

    #[test]
    fn signoff_disarms_after_identifying() {
        let mut t = timer();
        t.note_tx(1_000);
        assert!(t.signoff_due(1_000 + SIGNOFF));
        t.mark_identified(1_000 + SIGNOFF);
        assert!(
            !t.signoff_due(1_000 + SIGNOFF + SIGNOFF * 10),
            "no repeat sign-off without a new transmission"
        );
        // A new exchange re-arms it.
        t.note_tx(100_000);
        assert!(t.signoff_due(100_000 + SIGNOFF));
    }

    #[test]
    fn interval_and_signoff_coexist_on_a_long_then_quiet_exchange() {
        // Continuous TX every 8 s keeps the channel from going quiet, so sign-off never fires
        // during the exchange, but the 10-min interval ID still triggers.
        let mut t = timer();
        let mut clock = 0;
        while clock < TEN_MIN {
            t.note_tx(clock);
            assert!(
                !t.signoff_due(clock + 1),
                "no sign-off mid-exchange (gaps < idle)"
            );
            clock += 8_000;
        }
        assert!(
            t.id_due(TEN_MIN),
            "interval ID fires during the long exchange"
        );
        t.mark_identified(TEN_MIN);
        // Exchange ends: one last TX, then the channel goes quiet → sign-off ID.
        t.note_tx(TEN_MIN + 1_000);
        assert!(
            t.signoff_due(TEN_MIN + 1_000 + SIGNOFF),
            "sign-off ID at the end of the exchange"
        );
    }
}
