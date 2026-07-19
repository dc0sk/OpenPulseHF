//! Wall-clock T/R scheduler (plan §6.3). JS8 slots are aligned to UTC (`:00/:15/:30/:45` for NORMAL);
//! this maps epoch milliseconds to a slot index/phase, carries a drift bias estimated from decode
//! `dt`s, and gates TX when the clock is too far off. Pure: it takes `now_ms` (UTC epoch millis) so
//! the daemon owns the `SystemTime` read and this stays testable.

use js8_plugin::submode::{params, Submode};

/// UTC-slot clock for one submode, plus a drift bias applied to every reading.
#[derive(Debug, Clone)]
pub struct Js8Clock {
    /// Decodes folded into `drift_bias_ms` so far; 0 means the clock is unmeasured, not good.
    observations: u32,
    submode: Submode,
    slot_len_ms: u64,
    start_delay_ms: u64,
    /// Bias added to `now_ms` (ms), estimated from the median decode `dt`. Positive = our clock is slow.
    drift_bias_ms: i64,
}

impl Js8Clock {
    /// A clock for `submode` with zero drift bias.
    pub fn new(submode: Submode) -> Self {
        let p = params(submode);
        Self {
            submode,
            slot_len_ms: p.slot_secs as u64 * 1000,
            start_delay_ms: p.start_delay_ms as u64,
            drift_bias_ms: 0,
            observations: 0,
        }
    }

    /// The submode this clock schedules.
    pub fn submode(&self) -> Submode {
        self.submode
    }

    /// Slot length in milliseconds (`15_000` for NORMAL).
    pub fn slot_len_ms(&self) -> u64 {
        self.slot_len_ms
    }

    /// Bias-corrected epoch time.
    fn corrected(&self, now_ms: u64) -> u64 {
        now_ms.saturating_add_signed(self.drift_bias_ms)
    }

    /// UTC slot index containing `now_ms`.
    pub fn slot_index(&self, now_ms: u64) -> u64 {
        self.corrected(now_ms) / self.slot_len_ms
    }

    /// Position within the current slot, in milliseconds (`0..slot_len_ms`).
    pub fn phase_ms(&self, now_ms: u64) -> u64 {
        self.corrected(now_ms) % self.slot_len_ms
    }

    /// Epoch time of the next slot boundary at or after `now_ms`.
    pub fn next_slot_start_ms(&self, now_ms: u64) -> u64 {
        let phase = self.phase_ms(now_ms);
        now_ms + (self.slot_len_ms - phase)
    }

    /// Set the drift bias (ms) — the running median of decode `dt`s.
    pub fn set_drift_bias_ms(&mut self, bias_ms: i64) {
        self.drift_bias_ms = bias_ms;
    }

    /// Largest skew this estimator can ever observe, in ms.
    ///
    /// `dt_ms` comes from where a decode landed inside the decoder's slot-start search window, which
    /// `runtime::decode_slot` sets at ±0.75 s. An EWMA of values bounded by ±750 ms can never leave
    /// ±750 ms, so **a `max_skew_ms` above this can never refuse TX** — the configured ±2 s tolerance
    /// is structurally unreachable (audit 2026-07-19, #10). Widening it means widening the decode
    /// search window, which costs real CPU on a Pi; the alternative is to accept that the gate covers
    /// sub-second skew only, which is what it actually does today.
    pub const OBSERVABLE_SKEW_BOUND_MS: u64 = 750;

    /// Fold one decoded frame's timing error into the drift-bias estimate (EWMA, α = 1/8).
    ///
    /// `dt_ms` is `start_delay_ms − observed_start_ms`: a conforming station transmits `start_delay_ms`
    /// into the UTC slot, so if our decode places it *later* than that our clock is fast and the bias
    /// goes negative (and vice-versa). Smoothing averages out per-station timing error + capture jitter,
    /// leaving our systematic offset. The magnitude drives the ±2 s TX-skew gate (`tx_allowed`); the
    /// observable range is bounded by the decoder's slot-start search window (see `runtime::decode_slot`).
    pub fn observe_dt_ms(&mut self, dt_ms: i64) {
        // Integer EWMA: bias += (dt − bias) / 8.
        self.drift_bias_ms += (dt_ms - self.drift_bias_ms) / 8;
        self.observations = self.observations.saturating_add(1);
    }

    /// How many decodes have contributed to the drift estimate.
    pub fn observations(&self) -> u32 {
        self.observations
    }

    /// Whether the drift estimate rests on any actual measurement.
    ///
    /// `drift_bias_ms` starts at 0, which is indistinguishable from "measured, and perfect". Before
    /// the first decode the clock is **unverified**, not good — a caller that reports skew to an
    /// operator must say which of the two it is (audit 2026-07-19, #10).
    pub fn clock_verified(&self) -> bool {
        self.observations > 0
    }

    /// Whether `max_skew_ms` is large enough that [`Js8Clock::tx_allowed`] can never refuse.
    ///
    /// Returns true when the configured tolerance exceeds what the estimator can observe, i.e. the
    /// gate is inert. Callers should surface this rather than let an operator believe a ±2 s
    /// tolerance is being enforced.
    pub fn skew_gate_is_inert(max_skew_ms: u64) -> bool {
        max_skew_ms >= Self::OBSERVABLE_SKEW_BOUND_MS
    }

    /// Current drift bias (ms).
    pub fn drift_bias_ms(&self) -> i64 {
        self.drift_bias_ms
    }

    /// Whether TX is permitted: the clock must be within `max_skew_ms` of UTC (else RX-only degrade,
    /// plan D5 — JS8's published ±2 s tolerance).
    ///
    /// **Two limits worth knowing** (audit 2026-07-19, #10):
    /// 1. The estimate is bounded by [`Js8Clock::OBSERVABLE_SKEW_BOUND_MS`], so any `max_skew_ms` at
    ///    or above that can never refuse — see [`Js8Clock::skew_gate_is_inert`].
    /// 2. With **no observations** this returns `true`, because `drift_bias_ms` starts at 0. That is
    ///    deliberate: a station on a quiet band has heard nothing to measure against, and refusing to
    ///    beacon would stop it ever being discovered. But "unmeasured" is not "verified" — use
    ///    [`Js8Clock::clock_verified`] before reporting the clock as good.
    pub fn tx_allowed(&self, max_skew_ms: u64) -> bool {
        self.drift_bias_ms.unsigned_abs() <= max_skew_ms
    }

    /// Epoch time of this slot's TX start (slot boundary + the submode's start delay).
    pub fn tx_start_ms(&self, now_ms: u64) -> u64 {
        let slot_start = now_ms - self.phase_ms(now_ms);
        slot_start + self.start_delay_ms
    }
}

/// Fires once each time the UTC slot advances, so the daemon can close out the previous dwell window.
#[derive(Debug, Clone)]
pub struct SlotTracker {
    last_slot: Option<u64>,
}

impl Default for SlotTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl SlotTracker {
    /// A tracker that has not yet seen a slot.
    pub fn new() -> Self {
        Self { last_slot: None }
    }

    /// Update with the current slot; returns the slot that just *completed* when a boundary is crossed
    /// (i.e. one less than the new slot on the first advance), else `None`.
    pub fn advance(&mut self, current_slot: u64) -> Option<u64> {
        match self.last_slot.replace(current_slot) {
            Some(prev) if current_slot > prev => Some(current_slot - 1),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_slot_is_15s_and_indexes_utc() {
        let c = Js8Clock::new(Submode::Normal);
        assert_eq!(c.slot_len_ms(), 15_000);
        assert_eq!(c.slot_index(0), 0);
        assert_eq!(c.slot_index(14_999), 0);
        assert_eq!(c.slot_index(15_000), 1);
        assert_eq!(c.slot_index(45_000), 3); // :45
        assert_eq!(c.phase_ms(15_500), 500);
    }

    #[test]
    fn next_slot_and_tx_start() {
        let c = Js8Clock::new(Submode::Normal);
        assert_eq!(c.next_slot_start_ms(15_500), 30_000);
        // TX starts 500 ms into the slot (NORMAL start delay).
        assert_eq!(c.tx_start_ms(15_500), 15_500); // slot start 15000 + 500
        assert_eq!(c.tx_start_ms(20_000), 15_500);
    }

    #[test]
    fn drift_bias_shifts_the_slot_and_gates_tx() {
        let mut c = Js8Clock::new(Submode::Normal);
        // Without bias, 14_900 is still slot 0.
        assert_eq!(c.slot_index(14_900), 0);
        // +200 ms bias pushes it into slot 1.
        c.set_drift_bias_ms(200);
        assert_eq!(c.slot_index(14_900), 1);
        // TX gate: within ±2 s ok, beyond it refused.
        assert!(c.tx_allowed(2000));
        c.set_drift_bias_ms(-2500);
        assert!(!c.tx_allowed(2000));
        assert!(c.tx_allowed(3000));
    }

    #[test]
    fn observe_dt_converges_toward_the_offset() {
        let mut c = Js8Clock::new(Submode::Normal);
        assert_eq!(c.drift_bias_ms(), 0);

        // A steady stream of decodes each showing our clock ~600 ms fast (dt = −600) converges the
        // EWMA toward −600.
        for _ in 0..40 {
            c.observe_dt_ms(-600);
        }
        assert!(
            (c.drift_bias_ms() + 600).abs() < 30,
            "EWMA converged near −600, got {}",
            c.drift_bias_ms()
        );
        assert!(c.tx_allowed(2000), "600 ms skew is within tolerance");
    }

    /// The ±2 s tolerance the plan specifies CANNOT refuse TX, because the estimate is bounded by the
    /// decoder's ±0.75 s search window. The previous test claimed to prove the gate could trip, but
    /// did so by feeding `observe_dt_ms(-2500)` — a value production can never produce.
    #[test]
    fn the_configured_two_second_skew_gate_is_structurally_inert() {
        assert!(
            Js8Clock::skew_gate_is_inert(2000),
            "a ±2 s tolerance is above the observable bound, so it can never refuse TX"
        );
        assert!(
            !Js8Clock::skew_gate_is_inert(500),
            "a tolerance inside the observable range can still refuse"
        );

        // Drive it with the largest value the decoder can actually report, sustained.
        let mut c = Js8Clock::new(Submode::Normal);
        for _ in 0..200 {
            c.observe_dt_ms(-(Js8Clock::OBSERVABLE_SKEW_BOUND_MS as i64));
        }
        assert!(
            c.tx_allowed(2000),
            "even a saturated estimate stays inside ±2 s — the gate cannot fire in production, \
             got {} ms",
            c.drift_bias_ms()
        );
        // ... but a tolerance set inside the observable range does fire on the same data.
        assert!(
            !c.tx_allowed(500),
            "a 500 ms tolerance must refuse at a saturated 750 ms estimate, got {} ms",
            c.drift_bias_ms()
        );
    }

    /// A zero drift bias means "never measured", not "measured and perfect". `tx_allowed` stays open
    /// on purpose (a quiet-band station must still be able to beacon), so the distinction has to be
    /// available separately or an operator is told the clock is good when nothing checked it.
    #[test]
    fn an_unmeasured_clock_is_not_reported_as_verified() {
        let mut c = Js8Clock::new(Submode::Normal);
        assert_eq!(c.observations(), 0);
        assert!(
            !c.clock_verified(),
            "no decodes yet — nothing has been measured"
        );
        assert!(
            c.tx_allowed(2000),
            "TX stays permitted so a station on a quiet band can still announce itself"
        );

        c.observe_dt_ms(-100);
        assert_eq!(c.observations(), 1);
        assert!(c.clock_verified(), "one decode is a measurement");
    }

    #[test]
    fn slot_tracker_fires_once_per_boundary() {
        let mut t = SlotTracker::new();
        assert_eq!(t.advance(5), None); // first observation, no completed slot
        assert_eq!(t.advance(5), None); // same slot
        assert_eq!(t.advance(6), Some(5)); // slot 5 just completed
        assert_eq!(t.advance(6), None);
        assert_eq!(t.advance(7), Some(6));
    }
}
