//! Wall-clock T/R scheduler (plan §6.3). JS8 slots are aligned to UTC (`:00/:15/:30/:45` for NORMAL);
//! this maps epoch milliseconds to a slot index/phase, carries a drift bias estimated from decode
//! `dt`s, and gates TX when the clock is too far off. Pure: it takes `now_ms` (UTC epoch millis) so
//! the daemon owns the `SystemTime` read and this stays testable.

use js8_plugin::submode::{params, Submode};

/// UTC-slot clock for one submode, plus a drift bias applied to every reading.
#[derive(Debug, Clone)]
pub struct Js8Clock {
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
    }

    /// Current drift bias (ms).
    pub fn drift_bias_ms(&self) -> i64 {
        self.drift_bias_ms
    }

    /// Whether TX is permitted: the clock must be within `max_skew_ms` of UTC (else RX-only degrade,
    /// plan D5 — JS8's published ±2 s tolerance).
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
    fn observe_dt_converges_toward_the_offset_and_can_trip_the_gate() {
        let mut c = Js8Clock::new(Submode::Normal);
        assert_eq!(c.drift_bias_ms(), 0);
        assert!(c.tx_allowed(2000), "gate open at zero drift");

        // A steady stream of decodes each showing our clock ~600 ms fast (dt = −600) converges the EWMA
        // toward −600, staying within the ±2 s tolerance (gate stays open).
        for _ in 0..40 {
            c.observe_dt_ms(-600);
        }
        assert!(
            (c.drift_bias_ms() + 600).abs() < 30,
            "EWMA converged near −600, got {}",
            c.drift_bias_ms()
        );
        assert!(c.tx_allowed(2000), "600 ms skew is within tolerance");

        // A sustained large skew beyond the tolerance trips the gate (RX-only degrade, D5).
        for _ in 0..80 {
            c.observe_dt_ms(-2500);
        }
        assert!(
            !c.tx_allowed(2000),
            "a sustained >2 s skew must refuse TX, got {}",
            c.drift_bias_ms()
        );
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
