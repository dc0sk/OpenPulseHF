//! Runtime calibration of the preamble-correlation threshold to the station's own noise (#1060).
//!
//! The shipped per-mode threshold (BPSK250: 0.40) was derived from that mode's decode cliff and
//! validated on two SSB-bandwidth idle captures. Measured on a real IC-9700 2026-08-17, the idle ρ
//! ceiling moves with **receive bandwidth**: per-window p50 0.131 at 2470 Hz, 0.236 at 554 Hz,
//! 0.351 at 309 Hz — and at the narrowest, 18.8 % of noise windows clear 0.40, so the veto
//! corroborates settles on noise, which is the failure mode it exists to prevent.
//!
//! **The design is CFAR, not a bandwidth estimator.** There is no honest signal-free sample source:
//! in the hot-floor regime the energy gate fires continuously, which is exactly when calibration is
//! needed. So this estimates a robust *location* of the statistic the veto already computes and
//! reaches the decision level through a measured shape factor — the standard cell-averaging CFAR
//! move, for the standard reason.
//!
//! Three properties make it safe:
//!
//! * **The samples cost nothing.** Every ρ the veto computes at a settle attempt is a sample. No
//!   extra correlation runs — and correlation cost is what #1138 got wrong by assuming it.
//! * **The anchor is a median**, so a *rare* frame cannot move it - the same poison-resistance
//!   `NoiseFloorTracker` relies on for energy. It is **not** immune in general: a station carrying
//!   heavy traffic that fails to decode pushes many high samples, and that is the #1060 user rather
//!   than a hypothetical. What contains it is the direction of the failure - contamination drives
//!   the derived level *up* into stand-down, i.e. back to energy-only detection, rather than into
//!   silently over-vetoing the traffic that caused it.
//! * **It can only raise the threshold, never lower it.** The published constant encodes
//!   decode-cliff knowledge the noise statistics know nothing about, so it stays a floor and the
//!   change is monotone-safe: never weaker than the shipped behaviour.

use std::collections::VecDeque;

/// Samples retained. At one sample per settle attempt this is minutes of history — long enough for
/// a stable median, short enough to follow an operator changing the filter.
const CAPACITY: usize = 512;

/// Samples required before the derived threshold is used at all. Below this the published constant
/// stands unchanged, so a cold receiver behaves exactly as it does today.
const MIN_SAMPLES: usize = 64;

/// Minimum onset advance, in samples, between two accepted samples.
///
/// Consecutive settle queries advance ~129 samples over a ~1000-sample window, so they overlap
/// ~87 % and are not independent draws: 64 of them can be one condemnation streak across a second of
/// atypical audio. Thinning to one sample per window length makes `MIN_SAMPLES` a statement about
/// the *audio* observed rather than about how many times one second of it was re-correlated.
const MIN_ONSET_ADVANCE: usize = 1_024;

/// Release margin for the stand-down latch.
///
/// Without it, a median wandering near `bound / FAMILY_FACTOR` flaps the veto on and off - and each
/// flip is an operator-visible warning about a receiver doing nothing unusual. Engage at the bound;
/// release only once the derived level has fallen a clear step below it.
const STAND_DOWN_RELEASE_MARGIN: f32 = 0.02;

/// Multiplier from the median of the ρ stream to the decision level.
///
/// **Measured, not fitted to one artifact** (`f11_quantile_ratio_across_bandwidth`): across five
/// recorded captures and four synthetic bands — a 4.3× move in p50, from 0.093 to 0.396 — the
/// distribution's *shape* held at p99/p50 = 1.29-1.50. 1.8 is that ratio's upper end with margin,
/// so the derived level sits above roughly the 99.9th percentile of the noise stream.
///
/// **It is a rate, not a ceiling.** Stating it as "above every observed maximum" would be a
/// duration-scoped claim dressed as a bound: a max over N windows is an extreme value that grows
/// with N, so the maxima checked against (1.51-1.75 x p50 over 1360 windows) describe those
/// captures' lengths, not the population. What the constant promises is a *bounded exceedance rate*
/// per query, which is what a CFAR threshold can promise and a ceiling is not.
///
/// **What would falsify it:** a noise population whose p99/p50 exceeds 1.8. Every sample behind this
/// number is *stationary* noise; impulsive interference (QRN, key clicks) has a heavier tail and is
/// untested. If that regime moves the ratio, this constant is scoped to stationary noise and the
/// design needs a heavier-tailed family.
const FAMILY_FACTOR: f32 = 1.8;

/// Rolling calibration of the correlation threshold from the veto's own query stream.
#[derive(Debug, Clone)]
pub struct RhoCalibration {
    samples: VecDeque<f32>,
    /// Onset of the last accepted sample, so overlapping queries over the same audio are thinned.
    last_onset: Option<usize>,
    /// Whether the veto is currently standing down, so the decision has hysteresis rather than
    /// tracking a wandering median.
    standing_down: bool,
}

impl Default for RhoCalibration {
    fn default() -> Self {
        Self::new()
    }
}

impl RhoCalibration {
    /// An empty calibration: until it fills, every published threshold is returned unchanged.
    pub fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(CAPACITY),
            last_onset: None,
            standing_down: false,
        }
    }

    /// Record one rho the veto computed at `onset`, unless it overlaps the last accepted sample.
    ///
    /// Returns whether the sample was kept, so a test asserts on thinning directly rather than
    /// inferring it from a count that could be short for either reason.
    pub fn push_at(&mut self, rho: f32, onset: usize) -> bool {
        if !rho.is_finite() {
            return false;
        }
        if let Some(prev) = self.last_onset {
            if onset >= prev && onset - prev < MIN_ONSET_ADVANCE {
                return false;
            }
        }
        self.last_onset = Some(onset);
        if self.samples.len() == CAPACITY {
            self.samples.pop_front();
        }
        self.samples.push_back(rho);
        true
    }

    /// Record a sample with no onset information (unit tests, and callers with no position).
    pub fn push(&mut self, rho: f32) {
        let next = self.last_onset.map_or(0, |p| p + MIN_ONSET_ADVANCE);
        self.push_at(rho, next);
    }

    /// How many samples are held.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether no sample has been recorded.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Median of the retained stream, or `None` before `MIN_SAMPLES`.
    ///
    /// A median rather than a mean: the stream contains real frames as well as noise, and a mean
    /// tracks them where a median does not.
    pub fn anchor(&self) -> Option<f32> {
        if self.samples.len() < MIN_SAMPLES {
            return None;
        }
        let mut v: Vec<f32> = self.samples.iter().copied().collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Some(v[v.len() / 2])
    }

    /// The threshold to compare against, given the mode's published constant.
    ///
    /// `max(published, anchor × FAMILY_FACTOR)` — the calibration raises the bar when the station's
    /// own noise demands it and never lowers it below what the mode's decode cliff established.
    pub fn effective_threshold(&self, published: f32) -> f32 {
        match self.anchor() {
            Some(a) => published.max(a * FAMILY_FACTOR),
            None => published,
        }
    }

    /// Whether the veto must stand down: the derived threshold has climbed above the ρ a delivered
    /// frame is known to reach, so vetoing on it would discard frames the channel delivered.
    ///
    /// Standing down means energy-only frame detection — pre-#1049 behaviour, which is what
    /// narrow-filter stations silently get today. The point of deciding it explicitly is that it can
    /// be counted and announced instead.
    ///
    /// With no published bound there is nothing to compare against and the veto never stands down;
    /// that is the state every mode is in until a bound is measured for it.
    pub fn stands_down(&mut self, published: f32, delivered_frame_bound: Option<f32>) -> bool {
        let Some(bound) = delivered_frame_bound else {
            self.standing_down = false;
            return false;
        };
        let derived = self.effective_threshold(published);
        // Hysteresis: engage at the bound, release only below it by a clear margin.
        self.standing_down = if self.standing_down {
            derived > bound - STAND_DOWN_RELEASE_MARGIN
        } else {
            derived > bound
        };
        self.standing_down
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(c: &mut RhoCalibration, v: f32, n: usize) {
        for _ in 0..n {
            c.push(v);
        }
    }

    #[test]
    fn below_min_samples_the_published_constant_is_returned_unchanged() {
        let mut c = RhoCalibration::new();
        fill(&mut c, 0.35, MIN_SAMPLES - 1);
        assert!(c.anchor().is_none());
        assert_eq!(c.effective_threshold(0.40), 0.40);
    }

    #[test]
    fn a_quiet_station_is_never_weakened_below_the_published_floor() {
        // Wide-filter measured p50 = 0.131; 0.131 * 1.8 = 0.236, well under the published 0.40.
        let mut c = RhoCalibration::new();
        fill(&mut c, 0.131, MIN_SAMPLES);
        assert_eq!(c.effective_threshold(0.40), 0.40);
    }

    #[test]
    fn a_narrow_filter_raises_the_threshold_above_its_measured_noise_ceiling() {
        // 500 Hz measured p50 = 0.236, max over 45 s = 0.413.
        let mut c = RhoCalibration::new();
        fill(&mut c, 0.236, MIN_SAMPLES);
        let t = c.effective_threshold(0.40);
        assert!(
            t > 0.413,
            "derived {t} must clear the measured 500 Hz ceiling"
        );
    }

    #[test]
    fn the_median_anchor_is_not_moved_by_frames_in_the_stream() {
        let mut c = RhoCalibration::new();
        fill(&mut c, 0.236, 200);
        let quiet = c.effective_threshold(0.40);
        // A 10 % duty cycle of delivered frames, far above any real HF link.
        fill(&mut c, 0.95, 20);
        let contaminated = c.effective_threshold(0.40);
        assert!(
            (contaminated - quiet).abs() < 0.01,
            "median moved from {quiet} to {contaminated} under frame contamination"
        );
    }

    #[test]
    fn overlapping_queries_over_the_same_audio_are_thinned() {
        let mut c = RhoCalibration::new();
        assert!(c.push_at(0.2, 0));
        // Inside one window length of the last accepted sample: the same audio, re-correlated.
        assert!(!c.push_at(0.9, 100));
        assert!(!c.push_at(0.9, MIN_ONSET_ADVANCE - 1));
        assert!(c.push_at(0.2, MIN_ONSET_ADVANCE));
        assert_eq!(c.len(), 2);
    }

    /// The poison case that matters, and it is not hypothetical: a station carrying heavy traffic
    /// that FAILS to decode is the #1060 user, and every one of those settle queries pushes a high
    /// rho. The median cannot hold there — what must hold is the DIRECTION of the failure.
    #[test]
    fn heavy_undecodable_traffic_drives_stand_down_rather_than_over_vetoing() {
        let mut c = RhoCalibration::new();
        // 60 % duty cycle of frame-like correlation values over a quiet noise floor.
        for i in 0..200 {
            c.push(if i % 5 < 3 { 0.85 } else { 0.13 });
        }
        let derived = c.effective_threshold(0.40);
        assert!(
            derived > 0.50,
            "contaminated stream derived {derived}, which is still under the delivered-frame bound"
        );
        assert!(
            c.stands_down(0.40, Some(0.50)),
            "contamination raised the threshold without standing the veto down — that is the \
             failure direction this design exists to avoid: over-vetoing the very traffic that \
             poisoned the estimate"
        );
    }

    #[test]
    fn the_stand_down_latch_has_hysteresis() {
        let mut c = RhoCalibration::new();
        fill(&mut c, 0.290, MIN_SAMPLES); // derived 0.522 — engages against a 0.50 bound
        assert!(c.stands_down(0.40, Some(0.50)));
        // Drift back to just under the bound: still down, because release needs a clear margin.
        let mut d = RhoCalibration::new();
        fill(&mut d, 0.290, MIN_SAMPLES);
        assert!(d.stands_down(0.40, Some(0.50)));
        fill(&mut d, 0.275, 512); // derived 0.495 — under the bound but inside the margin
        assert!(
            d.stands_down(0.40, Some(0.50)),
            "released inside the margin — the veto will flap"
        );
        fill(&mut d, 0.260, 512); // derived 0.468 — clearly below
        assert!(!d.stands_down(0.40, Some(0.50)));
    }

    #[test]
    fn stand_down_engages_only_when_the_derived_level_passes_the_delivered_frame_bound() {
        let mut c = RhoCalibration::new();
        fill(&mut c, 0.236, MIN_SAMPLES); // 500 Hz class: derived 0.425
        assert!(!c.stands_down(0.40, Some(0.50)));
        let mut d = RhoCalibration::new();
        fill(&mut d, 0.351, MIN_SAMPLES); // 250 Hz class: derived 0.632
        assert!(d.stands_down(0.40, Some(0.50)));
        let mut d = RhoCalibration::new();
        fill(&mut d, 0.351, MIN_SAMPLES);
        // No published bound: nothing to compare against, so no stand-down.
        assert!(!d.stands_down(0.40, None));
    }
}
