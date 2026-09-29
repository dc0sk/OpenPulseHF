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
/// learns the LAST 131 s, still eight histories, so the floor is fully refreshed.
const MAX_HELD_SAMPLES: usize = 1 << 20;

/// Tracks the noise power the block RMS sees, for driving a squelch that follows the band.
#[derive(Debug, Clone)]
pub struct NoiseFloorTracker {
    /// Each window's one-sided bin powers (bins 1..=BINS), newest last.
    history: VecDeque<[f32; BINS]>,
    mean_sq: Option<f32>,
    /// Samples not yet consumed by a full analysis window (#1254). Bounded below `WINDOW`.
    carry: Vec<f32>,
    /// Audio kept aside while held; learned from on `commit`, dropped on `discard`.
    held: Option<Vec<f32>>,
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
        if let Some(held) = self.held.as_mut() {
            held.extend_from_slice(samples);
            if held.len() > MAX_HELD_SAMPLES {
                let excess = held.len() - MAX_HELD_SAMPLES;
                held.drain(..excess);
            }
            return self.mean_sq;
        }
        self.learn(samples);
        self.mean_sq
    }

    /// Stop learning: keep incoming audio aside until [`commit`](Self::commit) or
    /// [`discard`](Self::discard). A no-op if already held.
    pub fn hold(&mut self) {
        if self.held.is_none() {
            // The partial window before the hold goes with the held audio, not in the bin: a commit
            // learns it, a discard drops it. Clearing it instead dropped exactly the audio that
            // tripped the squelch — on a 250 Hz filter at 171-sample reads, where idle flickers ~9 %
            // of blocks, that biased the floor low (squelch/idle 1.18 instead of ~1.25, measured on a build committing flicker
            // without the carried window).
            self.held = Some(std::mem::take(&mut self.carry));
        }
    }

    /// How many samples are being kept aside (0 when not held).
    pub fn held_len(&self) -> usize {
        self.held.as_ref().map_or(0, Vec::len)
    }

    /// Whether audio is currently being kept aside.
    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }

    /// Learn from everything held (the burst was the band, not a transmission), and resume.
    pub fn commit(&mut self) {
        if let Some(held) = self.held.take() {
            self.learn(&held);
        }
    }

    /// Drop everything held (the burst was a transmission), and resume learning.
    pub fn discard(&mut self) {
        self.held = None;
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

    fn learn(&mut self, samples: &[f32]) {
        self.carry.extend_from_slice(samples);
        let windows = self.carry.len() / WINDOW;
        if windows == 0 {
            return;
        }
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(WINDOW);
        for w in 0..windows {
            let seg = &self.carry[w * WINDOW..(w + 1) * WINDOW];
            let mut buf: Vec<Complex32> = seg
                .iter()
                .enumerate()
                .map(|(n, &x)| {
                    let hann =
                        0.5 * (1.0 - (2.0 * std::f32::consts::PI * n as f32 / WINDOW as f32).cos());
                    Complex32::new(x * hann, 0.0)
                })
                .collect();
            fft.process(&mut buf);
            let mut powers = [0.0f32; BINS];
            for (p, c) in powers.iter_mut().zip(&buf[1..=BINS]) {
                *p = c.norm_sqr();
            }
            if self.history.len() == HISTORY_WINDOWS {
                self.history.pop_front();
            }
            self.history.push_back(powers);
        }
        self.carry.drain(..windows * WINDOW);
        self.mean_sq = Some(self.estimate());
    }

    /// `σ² = 2·Σ_k P̄_k / (N²·G)` over the one-sided bins, where `P̄_k` is bin k's noise power: for
    /// white noise of variance σ² a Hann periodogram bin has mean σ²·N·G, and the one-sided bins
    /// carry half the power each.
    fn estimate(&self) -> f32 {
        let count = self.history.len();
        let mut column = Vec::with_capacity(count);
        let mut total = 0.0f32;
        for k in 0..BINS {
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
            total += level;
        }
        2.0 * total / ((WINDOW * WINDOW) as f32 * HANN_POWER_GAIN)
    }
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

    /// White noise through a 4th-order band-pass centred at 1500 Hz: what a narrow receive filter
    /// leaves. Two cascaded RBJ band-pass biquads; `q` sets the width (q 3 ≈ 500 Hz at −3 dB).
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

    #[test]
    fn short_input_yields_no_estimate() {
        let mut t = NoiseFloorTracker::new();
        assert!(t.update(&[0.0; 100], 8_000.0).is_none());
    }
}
