//! Periodic + end-of-exchange station-identification timer (REQ-REG-10).

/// Whether `callsign` is usable as a station identifier for on-air transmission (§97.119): non-empty
/// after trimming and not the placeholder `N0CALL`. A station must not key the transmitter under an
/// absent or placeholder call, so TNC / daemon TX paths gate keyed emissions on this.
pub fn callsign_is_valid(callsign: &str) -> bool {
    let trimmed = callsign.trim();
    !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("N0CALL")
}

/// How long before the interval ID is due the station starts waiting for a quiet channel
/// ([`due_reason`](StationIdTimer::due_reason)). Longer than the longest `hpx_hf` frame (SL2, 66.6 s),
/// so one frame of the other station's fits in it; the ID is still forced at the interval itself.
pub const ID_QUIET_WINDOW_MS: u64 = 90_000;

/// How long the channel must have been quiet before a station ID that is not forced goes out. Longer
/// than the turnaround after an ACK (about 1 s on air), when the channel is quiet only because the
/// sender has not started its next frame yet: on air (task 18, run a2b) an ID keyed 0.1 s after the
/// station's own ACK covered the start of the other station's next frame, which was lost.
pub const ID_MIN_QUIET_MS: u64 = 3_000;

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
///   with a non-zero sign-off idle, armed by TX since the last ID, and no TX of our own for
///   `signoff_idle_ms`. [`due_reason`](Self::due_reason) also waits for a quiet channel, so it is
///   not keyed over another station; a busy channel leaves it to the forced interval ID.
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
    /// Since when the channel has been quiet, as last reported to [`due_reason`](Self::due_reason).
    quiet_since_ms: Option<u64>,
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
            quiet_since_ms: None,
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
        // Our own transmission is not quiet, and the callers do not poll while it lasts: without this
        // a quiet stretch before it would count on the first poll after it (review, 2026-11-02).
        self.quiet_since_ms = None;
        if self.is_enabled() {
            self.tx_since_id = true;
            self.last_tx_ms = now_ms;
        }
    }

    /// Which ID to transmit now, given whether the channel is quiet at `now_ms` (no burst being
    /// gathered, carrier detect idle), or `None`. Called every poll: it also tracks for how long the
    /// channel has been quiet.
    ///
    /// - **interval:** from [`ID_QUIET_WINDOW_MS`] before the interval, once the channel has been quiet
    ///   for [`ID_MIN_QUIET_MS`]; at the interval itself, regardless, so a busy channel cannot push it
    ///   past §97.119's limit.
    /// - **sign-off:** only after [`ID_MIN_QUIET_MS`] of quiet. It used to fire 10 s after the station's
    ///   own last transmission, which on air was the middle of the other station's next frame (task 17).
    pub fn due_reason(&mut self, now_ms: u64, channel_quiet: bool) -> Option<&'static str> {
        self.quiet_since_ms = if channel_quiet {
            Some(self.quiet_since_ms.unwrap_or(now_ms))
        } else {
            None
        };
        if !self.is_enabled() || !self.tx_since_id {
            return None;
        }
        let settled = self
            .quiet_since_ms
            .is_some_and(|t| now_ms.saturating_sub(t) >= ID_MIN_QUIET_MS);
        let since_id = now_ms.saturating_sub(self.last_id_ms);
        // At most half the interval, so a short configured interval does not ID after every frame.
        let window = ID_QUIET_WINDOW_MS.min(self.interval_ms / 2);
        if since_id >= self.interval_ms || (settled && since_id >= self.interval_ms - window) {
            return Some("interval");
        }
        if settled && self.signoff_due(now_ms) {
            return Some("sign-off");
        }
        None
    }

    /// Whether an ID may ride in this station's own next transmission (before its data frame or its
    /// ACK, in the same keying): the interval's early window is open. An ID sent that way is never
    /// keyed over the other station, so it needs no quiet channel. In a continuous exchange that is the
    /// only slot an unforced ID ever gets (review 2026-11-02, round 2, finding 2).
    pub fn ride_along_due(&self, now_ms: u64) -> bool {
        let window = ID_QUIET_WINDOW_MS.min(self.interval_ms / 2);
        self.is_enabled()
            && self.tx_since_id
            && now_ms.saturating_sub(self.last_id_ms) >= self.interval_ms - window
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

    #[test]
    fn callsign_validity_rejects_empty_and_placeholder() {
        assert!(callsign_is_valid("DC0SK"));
        assert!(callsign_is_valid("W1AW"));
        assert!(
            callsign_is_valid("  KN4CRD  "),
            "surrounding space is trimmed"
        );
        assert!(!callsign_is_valid(""), "empty is invalid");
        assert!(!callsign_is_valid("   "), "whitespace-only is invalid");
        assert!(!callsign_is_valid("N0CALL"), "the placeholder is invalid");
        assert!(
            !callsign_is_valid("n0call"),
            "placeholder check is case-insensitive"
        );
        assert!(
            !callsign_is_valid("  N0CALL "),
            "placeholder is invalid after trim"
        );
    }

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

    // ── Waiting for a quiet channel (on-air tasks 17 and 18) ─────────────────────

    /// Poll every 50 ms from `from` to `to` (inclusive) with the channel quiet or not, as the daemon's
    /// receive tick does; the last answer.
    fn poll(t: &mut StationIdTimer, from: u64, to: u64, quiet: bool) -> Option<&'static str> {
        let mut last = None;
        let mut now = from;
        while now <= to {
            last = t.due_reason(now, quiet);
            if last.is_some() {
                return last;
            }
            now += 50;
        }
        last
    }

    #[test]
    fn the_signoff_id_waits_for_a_quiet_channel() {
        let mut t = StationIdTimer::new(TEN_MIN, 0).with_signoff_idle_ms(SIGNOFF);
        t.note_tx(1_000);
        assert_eq!(
            poll(&mut t, 1_000, 1_000 + SIGNOFF + 60_000, false),
            None,
            "the other station is transmitting: no sign-off over its frame"
        );
        let quiet_from = 1_000 + SIGNOFF + 60_050;
        assert_eq!(
            poll(&mut t, quiet_from, quiet_from + ID_MIN_QUIET_MS - 50, true),
            None,
            "quiet for less than ID_MIN_QUIET_MS: could be the turnaround after an ACK"
        );
        assert_eq!(
            t.due_reason(quiet_from + ID_MIN_QUIET_MS, true),
            Some("sign-off"),
            "the sign-off goes out once the channel has been quiet long enough"
        );
    }

    #[test]
    fn no_id_in_the_turnaround_after_an_ack() {
        // The interval's early window is open; our ACK has just gone out and the other station starts
        // its next frame 1 s later (task 18, run a2b).
        let mut t = StationIdTimer::new(TEN_MIN, 0);
        t.note_tx(TEN_MIN - 60_000);
        let ack_end = TEN_MIN - 60_000;
        assert_eq!(poll(&mut t, ack_end, ack_end + 1_000, true), None);
        assert_eq!(poll(&mut t, ack_end + 1_050, ack_end + 8_000, false), None);
    }

    #[test]
    fn the_interval_id_takes_a_quiet_moment_early_and_is_forced_at_the_interval() {
        let mut t = StationIdTimer::new(TEN_MIN, 0);
        t.note_tx(1_000);
        let window_opens = TEN_MIN - ID_QUIET_WINDOW_MS;
        assert_eq!(
            poll(&mut t, window_opens - 10_000, window_opens - 50, true),
            None
        );
        assert_eq!(
            t.due_reason(window_opens, true),
            Some("interval"),
            "quiet for 10 s when the window opens: it goes out"
        );

        let mut t = StationIdTimer::new(TEN_MIN, 0);
        t.note_tx(1_000);
        assert_eq!(
            poll(&mut t, window_opens, TEN_MIN - 50, false),
            None,
            "inside the window a busy channel defers it"
        );
        assert_eq!(
            t.due_reason(TEN_MIN, false),
            Some("interval"),
            "at the interval it goes out regardless (§97.119)"
        );
    }

    #[test]
    fn no_id_without_a_transmission_quiet_or_not() {
        let mut t = StationIdTimer::new(TEN_MIN, 0).with_signoff_idle_ms(SIGNOFF);
        assert_eq!(poll(&mut t, 0, TEN_MIN * 3, true), None);
        assert_eq!(poll(&mut t, TEN_MIN * 3, TEN_MIN * 4, false), None);
    }

    #[test]
    fn a_short_interval_waits_at_least_half_of_it_for_the_early_slot() {
        let mut t = StationIdTimer::new(60_000, 0);
        t.note_tx(1_000);
        assert_eq!(
            poll(&mut t, 1_000, 29_950, true),
            None,
            "a 60 s interval must not ID at the first quiet moment after every frame"
        );
        assert_eq!(t.due_reason(30_000, true), Some("interval"));
        let mut t = StationIdTimer::new(60_000, 0);
        t.note_tx(1_000);
        assert_eq!(t.due_reason(60_000, false), Some("interval"));
    }

    #[test]
    fn quiet_before_our_own_transmission_does_not_count_after_it() {
        // The sender polls quiet, then sends a frame and listens for the ACK (no polls for 30 s); the
        // first poll after that must start the quiet clock, not find it settled.
        let mut t = StationIdTimer::new(TEN_MIN, 0);
        t.note_tx(1_000);
        let window_opens = TEN_MIN - ID_QUIET_WINDOW_MS;
        assert_eq!(
            poll(&mut t, window_opens - 10_000, window_opens - 9_000, true),
            None
        );
        t.note_tx(window_opens + 20_000);
        assert_eq!(
            t.due_reason(window_opens + 21_000, true),
            None,
            "a quiet stretch before our own transmission is not quiet after it"
        );
        assert_eq!(
            poll(
                &mut t,
                window_opens + 21_050,
                window_opens + 21_000 + ID_MIN_QUIET_MS,
                true
            ),
            Some("interval")
        );
    }

    #[test]
    fn an_id_rides_along_once_the_early_window_opens() {
        let mut t = StationIdTimer::new(TEN_MIN, 0);
        assert!(
            !t.ride_along_due(TEN_MIN),
            "nothing transmitted: nothing to identify"
        );
        t.note_tx(1_000);
        assert!(!t.ride_along_due(TEN_MIN - ID_QUIET_WINDOW_MS - 1));
        assert!(t.ride_along_due(TEN_MIN - ID_QUIET_WINDOW_MS));
        t.mark_identified(TEN_MIN - 60_000);
        assert!(!t.ride_along_due(TEN_MIN - 59_000), "just identified");
    }
}
