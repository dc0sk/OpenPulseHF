//! Noise-floor estimation for the carrier detect: the noise power the block RMS sees.
//!
//! **What it estimates.** The squelch compares a block's total RMS against this floor, so the floor
//! must be the total noise power in the same samples — every frequency bin, in the proportions the
//! receive filter actually passes. Each bin's noise level is tracked separately, over time, and the
//! levels are summed (#1452).
//!
//! **Why per bin, over time.** The previous estimator took a low percentile *across* bins of the
//! 300–2700 Hz band and scaled it as if the noise were white across that band. That fails from both
//! sides: behind a narrow receive filter most of those bins are stopband, so the percentile read the
//! stopband and the squelch collapsed to its clamp (#1452); and a wideband signal filling most of the
//! band pulled the percentile up to signal level (#1304). Across time, one bin's periodogram powers
//! ARE independent draws from one exponential distribution while the noise is stationary, which is
//! the assumption the quantile correction needs; across bins they were not.
//!
//! **Why it is not poisoned by frames.** A time-domain estimate follows a long transmission up. Here
//! the caller holds the tracker while it is gathering a burst ([`NoiseFloorTracker::hold`]): the audio
//! is kept aside, and learned from only if the burst turns out to be the band rather than a
//! transmission ([`NoiseFloorTracker::commit`], a runaway-cap flush) — otherwise discarded
//! ([`NoiseFloorTracker::discard`], an ordinary carrier drop). A steady interferer that never stops
//! does end up in the floor, which is right: it is in every block's RMS too.
//!
//! It is waveform-agnostic: a noise floor is a property of the band, not of the mode received.

use std::collections::VecDeque;

use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

/// Analysis window length — and the contract callers depend on: **the adaptive squelch engages
/// after this many samples of audio, however the caller chunked them** (#1254).
///
/// 512 samples at 8 kHz is 64 ms and 15.6 Hz per bin.
pub const WINDOW: usize = 512;

/// Bins tracked: every one-sided bin except DC and Nyquist (the seam's DC block has removed the
/// former; the latter carries nothing at an 8 kHz audio rate).
const BINS: usize = WINDOW / 2 - 1;

/// How many windows of history each bin's quantile is taken over: 256 × 64 ms ≈ 16 s.
///
/// A response-time choice, not a frame-length one: a frame enters the history only while the tracker
/// is cold, when its burst reached the runaway cap, or when it was a sub-preamble flicker (see the
/// module doc), so the window only sets how fast the floor follows a genuine change of band level.
const HISTORY_WINDOWS: usize = 256;

/// Below this many windows of history a per-bin quantile is too coarse an order statistic, so the
/// per-bin mean is used instead. The mean of exponential powers is unbiased at any count.
const COLD_WINDOWS: usize = 16;

/// Percentile of one bin's powers over time taken as its noise level, before bias correction. A low
/// percentile keeps a short transient (a click, a burst the caller did not hold) out of the level.
const QUANTILE: f32 = 0.25;

/// Hann window power gain, `Σw²/N` = 3/8.
const HANN_POWER_GAIN: f32 = 0.375;

/// `-ln(1 - QUANTILE)`: the 25th-percentile point of an exponential distribution in units of its
/// own mean. One bin's periodogram powers over time are exponentially distributed for stationary
/// Gaussian noise, so the quantile sits at this fraction of the mean and is divided back out.
/// **Derived, not fitted** — the tests check the chain against noise of known variance, white and
/// band-limited, not against a recording.
const EXP_QUANTILE_SCALE: f32 = 0.287_682_07;

/// A bin's powers above this multiple of its quantile-derived level are outliers — a transient the
/// caller did not hold — and are left out of the bin's mean.
///
/// Why a trimmed mean at all, rather than the quantile divided by `EXP_QUANTILE_SCALE`: that
/// correction is exact only for exponentially distributed powers, i.e. noise. A steady tone's bin
/// power is constant, so the correction inflated it by 1/0.288 ≈ 3.5× — a strong heterodyne in the
/// passband would have set the squelch ~4× above the real block level (measured: 3.38× on a carrier
/// 17 dB over the noise). The trimmed mean is exact for a constant bin and, for noise, reads
/// [`TRIMMED_EXP_MEAN`] of the mean, which is divided back out.
const TRIM_FACTOR: f32 = 5.0;

/// The mean of an exponential distribution truncated at `TRIM_FACTOR` × its mean, in units of that
/// mean: `(1 − (1+T)e^{−T}) / (1 − e^{−T})` at T = 5. **Derived, not fitted.**
const TRIMMED_EXP_MEAN: f32 = 0.966_081;

/// Most audio held aside while a burst is gathered: 2^20 samples, ~131 s at 8 kHz — below the slow
/// rungs' burst caps (up to 320 s), so beyond it the oldest held audio is dropped; a commit then
/// learns the LAST 131 s, still eight histories, so the floor is fully refreshed. Held audio is kept
/// as window periodograms, so this is a count of windows × `WINDOW`.
const MAX_HELD_SAMPLES: usize = 1 << 20;

/// Spectral busy test (#1454): a band is this many adjacent bins (62.5 Hz), sliding one bin at a time.
const S_BAND_BINS: usize = 4;

/// The spectral test's OPEN judges the last this many windows (256 ms).
const S_LOOKBACK: usize = 4;

/// The spectral test OPENS when one band's power is at least `.1` times its floor in at least `.0` of
/// the last [`S_LOOKBACK`] windows of either analysis phase. Measured on the three recorded idles with
/// both phases (#1454 round 7): no 3-of-4 at 4.0 in either phase (687–688 judged windows per 45 s
/// capture, so the rate is only bounded, < 0.44 %/window at 95 %); 4.5 is two half-steps above the
/// last non-zero idle cell (3.5).
const S_OPEN: (usize, f32) = (3, 4.5);

/// The spectral test's HOLD averages over the last this many windows (512 ms).
///
/// A count over four windows could not hold a BPSK31 frame: where the window grid puts its reversal
/// nulls under the Hann peak half the windows read 0.38 of the band power, in runs, and 2-of-4 at 3.0
/// split 11/16 of +8 dB frames (#1454 round 6). Memory length is what bridges those runs.
const S_HOLD_WINDOWS: usize = 8;

/// While a burst is open, the spectral test HOLDS it when one band's power, averaged over the last
/// [`S_HOLD_WINDOWS`] windows with each window's ratio capped at the open threshold, is at least this
/// many times its floor. The cap is what keeps the tail short: uncapped, one window ≥ 13× the floor
/// carried the mean for seven more windows, so a strong frame — which total power had already ended —
/// held its burst ~0.55 s and swallowed the next transmission (#1454 round 9). Capped, four of eight
/// windows at the open threshold are needed: the tail is at most ~4 windows at any level, and a click
/// train must run at 8 Hz or more to extend an armed burst. Measured with the cap: every BPSK31
/// placement at +6…+10 dB held as uncapped; longest idle hold run 0 / 1 / 0 windows (wide / 500 Hz /
/// 250 Hz); two frames 0.4 s apart are two bursts.
const S_HOLD_MEAN: f32 = 2.5;

/// A band is judged only if its mean per-bin floor is at least this fraction of the 95th percentile of
/// the per-bin floor over the analysed band. Behind a narrow filter the stopband floor is 60–70 dB below
/// the passband, so a click or a dropout reads as a 10³–10⁵ excess there (#1454 round 3).
const S_MASK_REL: f32 = 1e-3;

/// First and last bin-array index of the analysed band. Index `k` holds FFT bin `k + 1`, so this is
/// bins 20..=173, 312.5–2 703 Hz — the exact range the #1454 probe measured.
const S_LO: usize = 19;
const S_HI: usize = 172;

/// Number of band starts over the analysed band.
const S_BANDS: usize = S_HI + 2 - S_BAND_BINS - S_LO;

/// The spectral test's verdict over the windows one block completed (#1454).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpectralVerdict {
    /// Windows this block completed; with none, the caller keeps its last verdict.
    pub windows: usize,
    /// Some completed window satisfied the open test.
    pub open: bool,
    /// Some completed window satisfied the hold test.
    pub hold: bool,
}

/// Tracks the noise power the block RMS sees, for driving a squelch that follows the band.
#[derive(Debug, Clone)]
pub struct NoiseFloorTracker {
    /// Each window's one-sided bin powers (bins 1..=BINS), newest last.
    history: VecDeque<[f32; BINS]>,
    mean_sq: Option<f32>,
    /// Samples not yet consumed by a full analysis window (#1254). Bounded below `WINDOW`.
    carry: Vec<f32>,
    /// Window periodograms kept aside while held; learned from on `commit`, dropped on `discard`.
    held: Option<Vec<[f32; BINS]>>,
    /// Periodograms of the windows the last `judge` completed, awaiting the caller's decision.
    pending: Vec<[f32; BINS]>,
    /// Each bin's current floor level (periodogram units), set with `mean_sq`.
    levels: Option<[f32; BINS]>,
    /// Which band starts are judged (the passband mask), recomputed with `levels`.
    judged: [bool; S_BANDS],
    /// Band power / band floor for the last `S_HOLD_WINDOWS` windows, while warm — on the window grid.
    recent_a: VecDeque<[f32; S_BANDS]>,
    /// The same for the windows straddling each pair of grid windows (offset by half a window). The
    /// alternating preamble dips at one parity only, so one of the two phases always sees it (#1454
    /// round 7). Judged separately and OR'd: a max over the phases' ratios takes the larger of two
    /// noise draws per window and raised the idle tail.
    recent_b: VecDeque<[f32; S_BANDS]>,
    /// The last half-window of audio, the first half of the next straddling window.
    prev_half: Vec<f32>,
}

impl Default for NoiseFloorTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl NoiseFloorTracker {
    /// An empty tracker; it estimates nothing until one full window has arrived.
    pub fn new() -> Self {
        Self {
            history: VecDeque::with_capacity(HISTORY_WINDOWS),
            mean_sq: None,
            carry: Vec::new(),
            held: None,
            pending: Vec::new(),
            levels: None,
            judged: [false; S_BANDS],
            recent_a: VecDeque::with_capacity(S_HOLD_WINDOWS),
            recent_b: VecDeque::with_capacity(S_HOLD_WINDOWS),
            prev_half: Vec::with_capacity(WINDOW / 2),
        }
    }

    /// Fold captured audio into the estimate — or, while held, keep it aside — and return the
    /// current floor if one exists.
    ///
    /// Accepts any block length: samples are buffered across calls and consumed one `WINDOW` at a
    /// time, so the result depends only on the sample stream and not on how the caller chunked it
    /// (#1254). `sample_rate` is accepted for the caller's convenience; the estimate is in amplitude
    /// units and does not depend on it.
    pub fn update(&mut self, samples: &[f32], sample_rate: f32) -> Option<f32> {
        if sample_rate <= 0.0 {
            return self.mean_sq;
        }
        self.judge(samples);
        if self.held.is_some() {
            self.hold_pending();
        } else {
            self.learn_pending();
        }
        self.mean_sq
    }

    /// Window the block, keep each completed window's periodogram pending, and return the spectral
    /// test's verdict over those windows — judged against the floor from BEFORE this block (#1454). The
    /// caller then decides: [`learn_pending`](Self::learn_pending), [`hold_pending`](Self::hold_pending)
    /// or [`drop_pending`](Self::drop_pending). Inert (all false) until the tracker is warm.
    pub fn judge(&mut self, samples: &[f32]) -> SpectralVerdict {
        self.carry.extend_from_slice(samples);
        let windows = self.carry.len() / WINDOW;
        let mut verdict = SpectralVerdict {
            windows,
            ..SpectralVerdict::default()
        };
        if windows == 0 {
            return verdict;
        }
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(WINDOW);
        for w in 0..windows {
            let window = &self.carry[w * WINDOW..(w + 1) * WINDOW];
            let powers = periodogram(window, fft.as_ref());
            let straddle = (self.prev_half.len() == WINDOW / 2).then(|| {
                let mut x = self.prev_half.clone();
                x.extend_from_slice(&window[..WINDOW / 2]);
                periodogram(&x, fft.as_ref())
            });
            self.prev_half.clear();
            self.prev_half.extend_from_slice(&window[WINDOW / 2..]);
            match (self.is_warm(), self.levels) {
                (true, Some(levels)) => {
                    push_ratios(&mut self.recent_a, &powers, &levels);
                    // No straddle only on the first window after a `discard`: that phase skips one
                    // window, and neither history is cleared — a discard does not move the floor, so
                    // their ratios stand, and clearing them blinded S for nine windows after every
                    // burst.
                    if let Some(straddle) = straddle {
                        push_ratios(&mut self.recent_b, &straddle, &levels);
                    }
                    verdict.open |= self.opens(&self.recent_a) || self.opens(&self.recent_b);
                    verdict.hold |= self.holds(&self.recent_a) || self.holds(&self.recent_b);
                }
                _ => {
                    self.recent_a.clear();
                    self.recent_b.clear();
                }
            }
            self.pending.push(powers);
        }
        self.carry.drain(..windows * WINDOW);
        verdict
    }

    /// Whether some judged band met [`S_OPEN`] over the last `S_LOOKBACK` windows of one phase. Inert
    /// until the phase's history is full, so the open and the hold start together.
    fn opens(&self, recent: &VecDeque<[f32; S_BANDS]>) -> bool {
        let (m, t) = S_OPEN;
        recent.len() == S_HOLD_WINDOWS
            && (0..S_BANDS).any(|b| {
                self.judged[b]
                    && recent
                        .iter()
                        .skip(S_HOLD_WINDOWS - S_LOOKBACK)
                        .filter(|w| w[b] >= t)
                        .count()
                        >= m
            })
    }

    /// Whether some judged band's ratio averaged at least [`S_HOLD_MEAN`] over one phase's last
    /// `S_HOLD_WINDOWS` windows.
    fn holds(&self, recent: &VecDeque<[f32; S_BANDS]>) -> bool {
        recent.len() == S_HOLD_WINDOWS
            && (0..S_BANDS).any(|b| {
                self.judged[b]
                    && recent.iter().map(|w| w[b].min(S_OPEN.1)).sum::<f32>()
                        >= S_HOLD_MEAN * S_HOLD_WINDOWS as f32
            })
    }

    /// Learn from the windows the last `judge` completed.
    pub fn learn_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        for powers in std::mem::take(&mut self.pending) {
            self.push_history(powers);
        }
        self.mean_sq = Some(self.estimate());
    }

    /// Keep the windows the last `judge` completed aside with the held burst (starting a hold).
    pub fn hold_pending(&mut self) {
        let held = self.held.get_or_insert_with(Vec::new);
        held.append(&mut self.pending);
        let max = MAX_HELD_SAMPLES / WINDOW;
        if held.len() > max {
            let excess = held.len() - max;
            held.drain(..excess);
        }
    }

    /// Drop the windows the last `judge` completed without learning them.
    pub fn drop_pending(&mut self) {
        self.pending.clear();
    }

    /// Stop learning: keep incoming audio aside until [`commit`](Self::commit) or
    /// [`discard`](Self::discard). A no-op if already held. The partial window already buffered stays
    /// buffered and goes with the held audio when it completes — dropping it instead biased the floor
    /// low on a 250 Hz filter at 171-sample reads (squelch/idle 1.18 instead of ~1.25, measured on a
    /// build committing flicker without the carried window).
    pub fn hold(&mut self) {
        if self.held.is_none() {
            self.held = Some(Vec::new());
        }
    }

    /// How many samples are being kept aside (0 when not held): the held windows plus the partial
    /// window still buffered.
    pub fn held_len(&self) -> usize {
        self.held
            .as_ref()
            .map_or(0, |h| h.len() * WINDOW + self.carry.len())
    }

    /// Whether audio is currently being kept aside.
    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }

    /// Learn from everything held (the burst was the band, not a transmission), and resume.
    pub fn commit(&mut self) {
        if let Some(held) = self.held.take() {
            if held.is_empty() {
                return;
            }
            // A commit long enough to refill the ratio histories moves the floor they were judged
            // against (a cap flush after a step up in the band): stale ratios would open and hold a
            // phantom spectral burst for several windows. A shorter flicker commit moves the floor by
            // under 8/256 and the ratios stand.
            if held.len() >= S_HOLD_WINDOWS {
                self.recent_a.clear();
                self.recent_b.clear();
            }
            for powers in held {
                self.push_history(powers);
            }
            self.mean_sq = Some(self.estimate());
        }
    }

    /// Drop everything held (the burst was a transmission), and resume learning. The buffered partial
    /// window goes too: it is the transmission's tail, and would otherwise be learned into the next
    /// window.
    pub fn discard(&mut self) {
        if self.held.take().is_some() {
            self.carry.clear();
            // Else the first straddling window after the burst would span the discarded gap.
            self.prev_half.clear();
        }
    }

    /// Whether the floor rests on at least `COLD_WINDOWS` windows of history (#1452). Stricter than
    /// [`mean_sq`](Self::mean_sq) being `Some`, which holds from the first window: below this the
    /// estimate is a plain per-bin mean over what little was heard, which may be all one occupant.
    pub fn is_warm(&self) -> bool {
        self.history.len() >= COLD_WINDOWS
    }

    /// Current floor as mean-square, or `None` before the first full window.
    pub fn mean_sq(&self) -> Option<f32> {
        self.mean_sq
    }

    /// Current floor as RMS amplitude — the unit a squelch threshold is expressed in.
    pub fn rms(&self) -> Option<f32> {
        self.mean_sq.map(|m| m.sqrt())
    }

    fn push_history(&mut self, powers: [f32; BINS]) {
        if self.history.len() == HISTORY_WINDOWS {
            self.history.pop_front();
        }
        self.history.push_back(powers);
    }

    /// `σ² = 2·Σ_k P̄_k / (N²·G)` over the one-sided bins, where `P̄_k` is bin k's noise power: for
    /// white noise of variance σ² a Hann periodogram bin has mean σ²·N·G, and the one-sided bins
    /// carry half the power each.
    fn estimate(&mut self) -> f32 {
        let count = self.history.len();
        let mut column = Vec::with_capacity(count);
        let mut total = 0.0f32;
        let mut levels = [0.0f32; BINS];
        for (k, slot) in levels.iter_mut().enumerate() {
            column.clear();
            column.extend(self.history.iter().map(|w| w[k]));
            let level = if count < COLD_WINDOWS {
                column.iter().sum::<f32>() / count as f32
            } else {
                let idx = ((count - 1) as f32 * QUANTILE) as usize;
                let (_, q, _) = column.select_nth_unstable_by(idx, f32::total_cmp);
                let limit = TRIM_FACTOR * *q / EXP_QUANTILE_SCALE;
                let (sum, n) = column
                    .iter()
                    .filter(|&&p| p <= limit)
                    .fold((0.0f32, 0usize), |(s, n), &p| (s + p, n + 1));
                if n == 0 {
                    0.0
                } else {
                    sum / n as f32 / TRIMMED_EXP_MEAN
                }
            };
            *slot = level;
            total += level;
        }
        self.set_levels(levels);
        2.0 * total / ((WINDOW * WINDOW) as f32 * HANN_POWER_GAIN)
    }

    /// Store the per-bin floor and recompute the passband mask from it.
    fn set_levels(&mut self, levels: [f32; BINS]) {
        let mut band: Vec<f32> = levels[S_LO..=S_HI].to_vec();
        band.sort_by(f32::total_cmp);
        let p95 = band[(band.len() - 1) * 95 / 100];
        for (b, judged) in self.judged.iter_mut().enumerate() {
            let s = S_LO + b;
            let mean = levels[s..s + S_BAND_BINS].iter().sum::<f32>() / S_BAND_BINS as f32;
            *judged = mean >= S_MASK_REL * p95;
        }
        self.levels = Some(levels);
    }
}

/// Append one window's band ratios `Σ_B P / Σ_B F` to a phase's history, capped at `S_HOLD_WINDOWS`.
fn push_ratios(recent: &mut VecDeque<[f32; S_BANDS]>, powers: &[f32; BINS], levels: &[f32; BINS]) {
    let mut ratios = [0.0f32; S_BANDS];
    for (b, r) in ratios.iter_mut().enumerate() {
        let s = S_LO + b;
        let num: f32 = powers[s..s + S_BAND_BINS].iter().sum();
        let den: f32 = levels[s..s + S_BAND_BINS].iter().sum();
        *r = if den > 0.0 { num / den } else { 0.0 };
    }
    if recent.len() == S_HOLD_WINDOWS {
        recent.pop_front();
    }
    recent.push_back(ratios);
}

/// One-sided Hann periodogram of one window, bins 1..=BINS.
fn periodogram(seg: &[f32], fft: &dyn rustfft::Fft<f32>) -> [f32; BINS] {
    let mut buf: Vec<Complex32> = seg
        .iter()
        .enumerate()
        .map(|(n, &x)| {
            let hann = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * n as f32 / WINDOW as f32).cos());
            Complex32::new(x * hann, 0.0)
        })
        .collect();
    fft.process(&mut buf);
    let mut powers = [0.0f32; BINS];
    for (p, c) in powers.iter_mut().zip(&buf[1..=BINS]) {
        *p = c.norm_sqr();
    }
    powers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white(n: usize, sigma: f32, seed: u64) -> Vec<f32> {
        let mut s = seed | 1;
        (0..n)
            .map(|_| {
                // xorshift + a sum of uniforms → near-Gaussian, deterministic.
                let mut acc = 0.0f32;
                for _ in 0..4 {
                    s ^= s >> 12;
                    s ^= s << 25;
                    s ^= s >> 27;
                    let u =
                        (s.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64;
                    acc += u as f32 - 0.5;
                }
                acc * sigma * 1.732
            })
            .collect()
    }

    /// Warm exactly at `COLD_WINDOWS` windows of history — later than the floor's first estimate.
    #[test]
    fn warm_at_cold_windows_not_at_the_first_estimate() {
        let mut t = NoiseFloorTracker::new();
        t.update(&white((COLD_WINDOWS - 1) * WINDOW, 0.1, 7), 8000.0);
        assert!(
            t.mean_sq().is_some(),
            "the floor estimates from its first window"
        );
        assert!(!t.is_warm(), "warm one window before COLD_WINDOWS");
        t.update(&white(WINDOW, 0.1, 8), 8000.0);
        assert!(t.is_warm(), "not warm at COLD_WINDOWS windows");
    }

    /// White noise through a 4th-order band-pass centred at 1500 Hz: what a narrow receive filter
    /// leaves. Two cascaded RBJ band-pass biquads; `q` sets the width (q 3 ≈ 500 Hz at −3 dB).
    fn band_limited(n: usize, sigma: f32, q: f32, seed: u64) -> Vec<f32> {
        let x = white(n, sigma, seed);
        let w0 = 2.0 * std::f32::consts::PI * 1500.0 / 8000.0;
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        let (b0, b2) = (alpha / a0, -alpha / a0);
        let (a1, a2) = (-2.0 * w0.cos() / a0, (1.0 - alpha) / a0);
        let mut y = x;
        for _ in 0..2 {
            let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for v in y.iter_mut() {
                let x0 = *v;
                let y0 = b0 * x0 + b2 * x2 - a1 * y1 - a2 * y2;
                x2 = x1;
                x1 = x0;
                y2 = y1;
                y1 = y0;
                *v = y0;
            }
        }
        y
    }

    fn mean_sq(x: &[f32]) -> f32 {
        x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32
    }

    fn settled(audio: &[f32]) -> f32 {
        let mut t = NoiseFloorTracker::new();
        t.update(audio, 8000.0);
        t.mean_sq().expect("warm")
    }

    /// THE #1254 GATE: the floor is a function of the AUDIO, not of how the caller chunked it.
    ///
    /// The daemon's rx tick hands over one `read()` — period-quantized, ~200-600 samples at 8 kHz —
    /// so a tracker that consumed only whole reads would depend on the driver's period.
    #[test]
    fn the_floor_is_invariant_to_the_caller_s_chunking() {
        let audio = white(512 * 20, 0.05, 7);
        let reference = settled(&audio);
        // 200 is cpal's default ALSA period at 8 kHz; 400 is one nominal 50 ms daemon tick; 600 is a
        // tick that straddled two periods; 4096 is the read after a blocking decode (#1301).
        for chunk in [200usize, 400, 512, 600, 800, 4096] {
            let mut t = NoiseFloorTracker::new();
            let mut last = None;
            for block in audio.chunks(chunk) {
                last = t.update(block, 8000.0);
            }
            let got = last.expect("every chunking must warm on 20 windows of audio");
            assert_eq!(
                got.to_bits(),
                reference.to_bits(),
                "chunk {chunk}: floor {got:e} != one-shot {reference:e} — the estimate depends on \
                 the caller's block size, so the driver's period decides the squelch"
            );
        }
    }

    /// The daemon's own read size must warm the tracker at all — the branch #1254 was filed on.
    #[test]
    fn a_sub_window_read_warms_the_tracker_once_enough_audio_has_arrived() {
        let audio = white(512 * 4, 0.05, 11);
        let mut t = NoiseFloorTracker::new();
        let mut blocks = audio.chunks(400);
        assert!(
            t.update(blocks.next().expect("first"), 8000.0).is_none(),
            "400 samples is under one window, so nothing can be estimated yet"
        );
        for b in blocks {
            t.update(b, 8000.0);
        }
        assert!(
            t.mean_sq().is_some(),
            "four windows of audio delivered in 400-sample reads left the tracker cold — the \
             adaptive squelch never engages"
        );
    }

    /// The estimator recovers a KNOWN variance of white noise, cold (the per-bin mean) and settled
    /// (the per-bin quantile). This is what makes the constants derived rather than fitted.
    #[test]
    fn the_floor_recovers_a_known_white_variance() {
        for sigma in [0.003f32, 0.01, 0.05, 0.2] {
            for windows in [8usize, HISTORY_WINDOWS] {
                let x = white(512 * windows, sigma, 0xC0FFEE);
                let ratio = settled(&x) / mean_sq(&x);
                assert!(
                    (0.85..1.15).contains(&ratio),
                    "sigma {sigma}, {windows} windows: floor/true mean-square {ratio:.3} — the \
                     periodogram or quantile scaling is wrong"
                );
            }
        }
    }

    /// THE #1452 GATE: behind a narrow receive filter the floor is still the noise power the block
    /// RMS sees. The previous estimator read the stopband there and collapsed to the squelch clamp
    /// (measured on the recorded IC-9700 500 Hz and 250 Hz captures: 0.0001 against an idle of
    /// 0.071 / 0.045 RMS); this is the synthetic form of the same filter.
    #[test]
    fn the_floor_recovers_band_limited_noise_behind_a_narrow_filter() {
        for q in [3.0f32, 6.0] {
            let x = band_limited(512 * HISTORY_WINDOWS, 0.05, q, 0xF117);
            let ratio = settled(&x) / mean_sq(&x);
            assert!(
                (0.85..1.15).contains(&ratio),
                "band-pass q {q}: floor/true mean-square {ratio:.3} — the floor does not follow \
                 coloured noise, which is #1452"
            );
        }
    }

    /// A held burst does not move the floor when discarded, and does when committed.
    ///
    /// This is what keeps a long transmission from raising the squelch until it closes its own
    /// burst (#1304's shape, which a history of W windows alone does not prevent once the frame is
    /// longer than a quarter of it).
    #[test]
    fn a_held_burst_moves_the_floor_only_when_committed() {
        let quiet = white(512 * 64, 0.01, 0xA1);
        let loud = white(512 * 256, 0.2, 0xB2);

        let mut t = NoiseFloorTracker::new();
        t.update(&quiet, 8000.0);
        let before = t.mean_sq().expect("warm");
        t.hold();
        assert!(t.is_held());
        t.update(&loud, 8000.0);
        assert_eq!(
            t.mean_sq().expect("still warm").to_bits(),
            before.to_bits(),
            "held audio moved the floor"
        );
        t.discard();
        assert!(!t.is_held());
        assert_eq!(
            t.mean_sq().expect("warm").to_bits(),
            before.to_bits(),
            "a discarded burst moved the floor"
        );

        t.hold();
        t.update(&loud, 8000.0);
        t.commit();
        let after = t.mean_sq().expect("warm");
        assert!(
            after > before * 100.0,
            "a committed 16 s of a 20x louder band left the floor at {after:e} from {before:e}"
        );
    }

    /// A steady carrier that never stops ends up in the floor — deliberately: it is in every block's
    /// RMS too, so a floor that excluded it would read the channel permanently busy (#1452's shape
    /// from the other side). A short one does not, because it does not reach a quarter of the
    /// history.
    #[test]
    fn a_steady_carrier_joins_the_floor_and_a_short_one_does_not() {
        let noise = white(512 * HISTORY_WINDOWS, 0.01, 0x5EED);
        let carrier = |x: &[f32], from: usize, to: usize| -> Vec<f32> {
            x.iter()
                .enumerate()
                .map(|(n, &v)| {
                    let on = (from..to).contains(&n);
                    let c = 0.1 * (2.0 * std::f32::consts::PI * 1_500.0 * n as f32 / 8_000.0).cos();
                    v + if on { c } else { 0.0 }
                })
                .collect()
        };
        let clean = settled(&noise);

        let steady = carrier(&noise, 0, noise.len());
        let ratio = settled(&steady) / mean_sq(&steady);
        assert!(
            (0.85..1.15).contains(&ratio),
            "a steady carrier: floor/true {ratio:.3} — the floor must be what the block RMS sees"
        );

        let short = carrier(&noise, 0, noise.len() / 10);
        let lifted = settled(&short) / clean;
        assert!(
            lifted < 1.2,
            "a carrier for a tenth of the history lifted the floor {lifted:.2}x"
        );
    }

    /// A genuine change of band level is followed within the history, in both directions.
    #[test]
    fn the_floor_follows_the_band_up_and_down() {
        let mut t = NoiseFloorTracker::new();
        t.update(&white(512 * HISTORY_WINDOWS, 0.01, 0xA1), 8000.0);
        let low = t.rms().expect("floor");
        t.update(&white(512 * HISTORY_WINDOWS, 0.1, 0xB2), 8000.0);
        let high = t.rms().expect("floor");
        assert!(
            high > low * 8.0,
            "floor did not follow a 10x level rise: {low:.5} → {high:.5}"
        );
        t.update(&white(512 * HISTORY_WINDOWS, 0.01, 0xC3), 8000.0);
        let back = t.rms().expect("floor");
        assert!(
            back < low * 1.2,
            "floor did not come back down: {low:.5} → {high:.5} → {back:.5}"
        );
    }

    /// Feed `x` in `chunk`-sample blocks, learning every block; return every verdict that completed a
    /// window, in order.
    fn verdicts(t: &mut NoiseFloorTracker, x: &[f32], chunk: usize) -> Vec<SpectralVerdict> {
        let mut out = Vec::new();
        for b in x.chunks(chunk) {
            let v = t.judge(b);
            t.learn_pending();
            if v.windows > 0 {
                out.push(v);
            }
        }
        out
    }

    fn tone(n: usize, amp: f32, hz: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * hz * i as f32 / 8000.0).cos())
            .collect()
    }

    fn add(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b).map(|(x, y)| x + y).collect()
    }

    /// The spectral test is inert while the tracker is cold, even on a strong narrowband signal (#1454):
    /// a cold floor may be the occupant itself.
    #[test]
    fn the_spectral_test_is_inert_while_cold() {
        let x = add(
            &white(WINDOW * (COLD_WINDOWS - 1), 0.01, 3),
            &tone(WINDOW * (COLD_WINDOWS - 1), 0.1, 1500.0),
        );
        let mut t = NoiseFloorTracker::new();
        let v = verdicts(&mut t, &x, 400);
        assert!(
            v.iter().all(|v| !v.open && !v.hold),
            "a cold tracker opened S"
        );
    }

    /// A narrowband signal well above its band's floor opens the spectral test, and the hold follows,
    /// while total power barely moves — the case #1454 exists for.
    #[test]
    fn a_narrowband_signal_opens_the_spectral_test() {
        let sigma = 0.01f32;
        let idle = white(WINDOW * 64, sigma, 5);
        // Per-bin noise power of white noise is sigma²·N·G; a tone of amplitude A puts ≈ (A·N/4)²
        // in its bin. A = 0.02 is ~10 dB over the per-bin floor at this sigma, and 4·(0.02)²/2 over
        // sigma² is a total-power rise of under 1 dB.
        let n = WINDOW * 16;
        let sig = add(&white(n, sigma, 6), &tone(n, 0.02, 1500.0));
        let mut t = NoiseFloorTracker::new();
        verdicts(&mut t, &idle, 400);
        let v = verdicts(&mut t, &sig, 400);
        assert!(
            v.iter().any(|v| v.open),
            "a tone ~10 dB over its bin never opened S"
        );
        assert!(
            v.iter().filter(|v| v.hold).count() >= v.len() - S_HOLD_WINDOWS,
            "the hold did not follow the tone"
        );
    }

    /// Feed `x` as a held burst (the seam's order: judge, then keep the windows aside).
    fn held_verdicts(t: &mut NoiseFloorTracker, x: &[f32]) -> Vec<SpectralVerdict> {
        x.chunks(WINDOW)
            .map(|b| {
                let v = t.judge(b);
                t.hold_pending();
                v
            })
            .collect()
    }

    /// The spectral test re-opens promptly after a burst ends (#1454 round 8): a `discard` does not
    /// move the floor, so the ratio histories survive it. Clearing them — which the first window
    /// after a discard used to do, having no straddle — left S blind for nine windows after every
    /// burst, so a reply starting right after one opened late.
    #[test]
    fn the_spectral_test_reopens_promptly_after_a_burst() {
        let sigma = 0.01f32;
        let mut t = NoiseFloorTracker::new();
        verdicts(&mut t, &white(WINDOW * 64, sigma, 21), WINDOW);
        let occupant = |n, seed| add(&white(n, sigma, seed), &tone(n, 0.02, 1500.0));
        t.hold();
        held_verdicts(&mut t, &occupant(WINDOW * 12, 22));
        // The burst ends the way the accumulator ends it: once S is neither open nor holding.
        let mut quiet = 0;
        for seed in 23..60 {
            let v = held_verdicts(&mut t, &white(WINDOW, sigma, seed));
            quiet += 1;
            if !v[0].open && !v[0].hold {
                break;
            }
        }
        assert!(quiet < 30, "S never let go of the burst");
        t.discard();
        verdicts(&mut t, &white(WINDOW * 2, sigma, 61), WINDOW);
        let v = verdicts(&mut t, &occupant(WINDOW * 12, 62), WINDOW);
        let first = v.iter().position(|v| v.open);
        assert!(
            first.is_some_and(|w| w < 3),
            "the returning occupant opened S at window {first:?}, not within 3"
        );
    }

    /// A commit that moves the floor clears the ratio histories (#1454 round 8). After a cap flush
    /// commits a step up in the band, ratios judged against the old floor would open and hold a
    /// phantom spectral burst on the new level, which is now the band.
    #[test]
    fn a_floor_moving_commit_does_not_leave_a_phantom_open() {
        let sigma = 0.01f32;
        let mut t = NoiseFloorTracker::new();
        verdicts(&mut t, &white(WINDOW * 64, sigma, 31), WINDOW);
        t.hold();
        // A whole history's worth, so the committed level IS the new floor — with less, the floor
        // only part-adapts and the band honestly still reads above it (#1455).
        let step = held_verdicts(&mut t, &white(WINDOW * HISTORY_WINDOWS, 3.0 * sigma, 32));
        assert!(
            step.iter().any(|v| v.open),
            "the step up never opened S: the fixture is vacuous"
        );
        t.commit();
        let after = verdicts(&mut t, &white(WINDOW * 8, 3.0 * sigma, 33), WINDOW);
        assert!(
            after.iter().all(|v| !v.open),
            "S opened on the band it had just learned"
        );
    }

    /// White noise does not open the spectral test over a long run (sample-limited: 2 000 windows).
    #[test]
    fn white_noise_does_not_open_the_spectral_test() {
        let mut t = NoiseFloorTracker::new();
        verdicts(&mut t, &white(WINDOW * 64, 0.01, 7), 400);
        let v = verdicts(&mut t, &white(WINDOW * 2000, 0.01, 8), 400);
        let opens = v.iter().filter(|v| v.open).count();
        assert_eq!(
            opens,
            0,
            "white noise opened S in {opens} of {} blocks",
            v.len()
        );
    }

    /// An event inside one window of each phase cannot open the spectral test: it needs `S_OPEN.0` of
    /// the last `S_LOOKBACK` windows of ONE phase. The event sits in the first half of a grid window, so
    /// it also lies in exactly one straddling window. (A whole grid window is NOT such an event: it
    /// fills half of two straddling windows, and one more noise window then opens that phase — #1454
    /// round 7 measured 29/120 opens for 512-sample bursts at +29.5 dB on real idle, 17/120 single-phase.)
    #[test]
    fn an_event_inside_one_window_of_each_phase_does_not_open_the_spectral_test() {
        let mut t = NoiseFloorTracker::new();
        verdicts(&mut t, &white(WINDOW * 64, 0.01, 9), WINDOW);
        let mut x = white(WINDOW * 8, 0.01, 10);
        for v in x[WINDOW * 3 + WINDOW / 8..WINDOW * 3 + 3 * WINDOW / 8].iter_mut() {
            *v *= 30.0;
        }
        let v = verdicts(&mut t, &x, WINDOW);
        assert!(v.iter().all(|v| !v.open), "a single loud window opened S");
    }

    /// Behind a narrow filter the stopband is not judged: a broadband burst lasting several windows,
    /// −40 dB re the passband noise but far above the stopband floor, does not open the spectral test —
    /// and the control shows it WOULD have, in a band the mask excludes.
    #[test]
    fn the_stopband_is_not_judged_behind_a_narrow_filter() {
        let mut t = NoiseFloorTracker::new();
        verdicts(
            &mut t,
            &band_limited(WINDOW * HISTORY_WINDOWS, 0.05, 6.0, 11),
            400,
        );
        let judged = t.judged.iter().filter(|&&j| j).count();
        assert!(
            judged < S_BANDS,
            "the mask judged every band behind a narrow filter"
        );
        let base = band_limited(WINDOW * 8, 0.05, 6.0, 12);
        let burst = white(WINDOW * 8, 0.0005, 13);
        let x: Vec<f32> = base
            .iter()
            .zip(&burst)
            .enumerate()
            .map(|(i, (b, n))| {
                b + if (WINDOW * 2..WINDOW * 5).contains(&i) {
                    *n
                } else {
                    0.0
                }
            })
            .collect();
        let mut masked_would_open = false;
        for block in x.chunks(WINDOW) {
            let v = t.judge(block);
            t.learn_pending();
            assert!(!v.open, "a stopband burst opened S through the mask");
            if let Some(last) = t.recent_a.back() {
                masked_would_open |= (0..S_BANDS).any(|b| !t.judged[b] && last[b] >= S_OPEN.1);
            }
        }
        assert!(
            masked_would_open,
            "no masked band exceeded the open ratio — the burst never reached the stopband, so this \
             proves nothing about the mask"
        );
    }

    /// The verdict stream depends only on the audio, not on how it was chunked (#1254's rule, for S).
    #[test]
    fn the_spectral_verdicts_are_invariant_to_chunking() {
        let idle = white(WINDOW * 64, 0.01, 14);
        let n = WINDOW * 24;
        let sig = add(&white(n, 0.01, 15), &tone(n, 0.015, 1200.0));
        let run = |chunk: usize| {
            let mut t = NoiseFloorTracker::new();
            verdicts(&mut t, &idle, chunk);
            // Expand to one entry per window so different chunkings are comparable.
            let mut per_window = Vec::new();
            for b in sig.chunks(chunk) {
                let v = t.judge(b);
                t.learn_pending();
                for _ in 0..v.windows {
                    per_window.push((v.open, v.hold));
                }
            }
            per_window
        };
        let reference = run(WINDOW);
        assert!(
            reference.iter().any(|&(o, _)| o),
            "the fixture never opened S — vacuous"
        );
        for chunk in [171usize, 400, 4096] {
            let got = run(chunk);
            assert_eq!(got.len(), reference.len());
            // A block completing several windows reports the OR over them, so compare per block-end
            // window only where each block completes exactly one window.
            if chunk <= WINDOW {
                assert_eq!(
                    got, reference,
                    "chunk {chunk}: the verdicts depend on the read size"
                );
            }
        }
    }

    #[test]
    fn short_input_yields_no_estimate() {
        let mut t = NoiseFloorTracker::new();
        assert!(t.update(&[0.0; 100], 8_000.0).is_none());
    }
}
