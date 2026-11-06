//! A level-independent carrier detect for the OFDM52 rungs: open on the preamble, confirm and hold on
//! the cyclic prefix (`docs/dev/design/ofdm-preamble-detect.md`).
//!
//! Under a receiver AGC an OFDM frame is not louder than the gap hiss (total power 1.15–1.22× of it,
//! per 62.5 Hz band a median 1.2×), so no level statistic can gather it. Both statistics here are
//! normalised, so the receive level never enters:
//! - **ρ**, the preamble correlation over a ±50 Hz grid, opens a candidate;
//! - **γ₄**, the cyclic-prefix correlation of data symbols 1–4 on the grid that preamble sets,
//!   confirms it (a tone or a narrowband frame reads ρ ≤ 0.42 but γ up to 0.99, so CP never opens);
//! - **γ₈**, the same over the last 8 symbols, holds it.
//!
//! Everything is counted in samples, never by clock, so the verdicts depend only on the sample stream.

use std::collections::VecDeque;

use num_complex::Complex32;
use openpulse_core::plugin::{CarrierDetector, CarrierVerdict};
use openpulse_dsp::acquisition::{pick_separated_peaks, IqMatchedFilter};

use crate::acquire::{acquire, COARSE_SPAN_HZ, COARSE_STEP_HZ};
use crate::modulate::clipped_preamble;
use crate::params::{OfdmParams, CP, FFT_SIZE, SAMPLE_RATE, SYM_LEN};

/// ρ at which a preamble candidate is taken to the confirm. Fitted to: noise max 0.475 over 3 512 s on
/// air and 459 s of corpus; decoded minimum 0.593 (`moderate_f1`, 6 dB), 0.638 (9 dB), 0.697 (on air).
/// Falsified by a decoded frame under it on a channel the rung exists for, or a negative at or above
/// it that also passes the confirm (design note, the constants).
pub const RHO_OPEN: f32 = 0.55;

/// γ₄ at which a candidate opens. Decoded minimum 0.661 (fade); 0 of 3 697 wide-hiss trials reached
/// it. It rejects broadband noise only: narrowband audio reads up to 0.99 and is ρ's job.
pub const GAMMA_CONFIRM: f32 = 0.50;

/// γ₈ under which a held frame is released. Decoded in-frame minimum 0.431 (fade); hiss falls under it
/// within 8 symbols of a frame's end on air.
pub const GAMMA_RELEASE: f32 = 0.30;

/// Energy floor of the ρ search, as a share of the searched span's mean (the #1049 veto's value).
const ENERGY_FLOOR: f32 = 0.05;

/// Data symbols the confirm spans: the shortest OFDM frame sent (SL14, 1 B) is the preamble and 4.
const CONFIRM_SYMS: usize = 4;

/// Symbols the hold averages over.
const HOLD_SYMS: usize = 8;

/// Peaks taken from one read's ρ profile. A read holds at most a few preambles; the rest are lobes.
const MAX_PEAKS: usize = 16;

/// Taps of the Hilbert FIR that builds the analytic signal γ is computed on (odd; Hamming window).
/// At 8 kHz its transition band is about 200 Hz, inside OFDM52's 500–2 531 Hz band edge.
const HILBERT_TAPS: usize = 127;

/// The FIR's group delay, in samples.
const HILBERT_DELAY: usize = HILBERT_TAPS / 2;

/// Lag between a cyclic prefix and the body samples it copies.
const CP_LAG: usize = FFT_SIZE;

/// A preamble candidate waiting for its confirm.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    /// Absolute sample index of the preamble's first sample.
    onset: u64,
    rho: f32,
}

/// The frame being held.
#[derive(Debug, Clone)]
struct Held {
    /// Absolute index of the held frame's preamble: the symbol grid.
    grid: u64,
    /// Next data symbol (1-based) to score.
    next_sym: u64,
    /// `(Σ z·z*₂₅₆, Σ ½(|z|²+|z₂₅₆|²))` of the last scored symbols, newest last.
    stats: VecDeque<(Complex32, f32)>,
}

/// OFDM52's carrier detector over [`IqMatchedFilter`] and the cyclic prefix.
pub struct OfdmCarrierDetector {
    p: OfdmParams,
    /// Offset the receiver believes, Hz from 1500 (`center_frequency − 1500`, as `acquire` takes it).
    told: f32,
    template: Vec<f32>,
    filter: IqMatchedFilter,
    freqs: Vec<f32>,
    hilbert: Vec<f32>,
    /// The previous read's last `SYM_LEN − 1` samples: each ρ offset is scored exactly once.
    carry: Vec<f32>,
    /// The last `HILBERT_TAPS − 1` raw samples, for the FIR.
    fir_hist: VecDeque<f32>,
    /// Analytic signal from absolute index `z_start`.
    z: VecDeque<Complex32>,
    z_start: u64,
    /// Samples observed so far.
    pos: u64,
    pending: Vec<Candidate>,
    held: Option<Held>,
    /// Onset of the latest confirmed open, so a later read's lobe of the same preamble is not re-taken.
    last_open: Option<u64>,
    opens: u64,
    confirm_rejections: u64,
    last_gamma4: Option<f32>,
}

impl OfdmCarrierDetector {
    /// A detector for the OFDM52 layout `p` at a receiver centred `told` Hz from 1500.
    pub fn new(p: OfdmParams, told: f32) -> Self {
        let template = clipped_preamble(&p);
        let steps = (COARSE_SPAN_HZ / COARSE_STEP_HZ).round() as i32;
        let freqs = (-steps..=steps)
            .map(|k| told + k as f32 * COARSE_STEP_HZ)
            .collect();
        Self {
            p,
            told,
            filter: IqMatchedFilter::new(template.clone()),
            template,
            freqs,
            hilbert: hilbert_taps(),
            carry: Vec::new(),
            fir_hist: VecDeque::from(vec![0.0; HILBERT_TAPS - 1]),
            z: VecDeque::new(),
            z_start: 0,
            pos: 0,
            pending: Vec::new(),
            held: None,
            last_open: None,
            opens: 0,
            confirm_rejections: 0,
            last_gamma4: None,
        }
    }

    /// Confirmed opens since the detector was built.
    pub fn opens(&self) -> u64 {
        self.opens
    }

    /// Candidates with ρ ≥ [`RHO_OPEN`] that the confirm refused (γ₄ < [`GAMMA_CONFIRM`]).
    pub fn confirm_rejections(&self) -> u64 {
        self.confirm_rejections
    }

    /// γ₄ of the latest candidate judged, opened or refused.
    pub fn last_gamma4(&self) -> Option<f32> {
        self.last_gamma4
    }

    /// Extend the analytic signal by `read`: the FIR's output for absolute index `t` is ready once
    /// sample `t + HILBERT_DELAY` is in, so the signal ends `HILBERT_DELAY` samples behind the input.
    fn extend_analytic(&mut self, read: &[f32]) {
        let n_taps = self.hilbert.len();
        for (i, &x) in read.iter().enumerate() {
            self.fir_hist.push_back(x);
            if self.pos + (i as u64) < HILBERT_DELAY as u64 {
                // The FIR is still filling: its output would be for an index before sample 0.
                self.fir_hist.pop_front();
                continue;
            }
            // fir_hist holds the last HILBERT_TAPS samples, oldest first; the output sits at the centre.
            let mut q = 0.0f32;
            // Only taps at an odd distance from the centre are non-zero; the centre is odd.
            for (k, &h) in self.hilbert.iter().enumerate().step_by(2) {
                q += h * self.fir_hist[n_taps - 1 - k];
            }
            let re = self.fir_hist[n_taps - 1 - HILBERT_DELAY];
            self.z.push_back(Complex32::new(re, q));
            self.fir_hist.pop_front();
        }
    }

    /// One past the last absolute index the analytic signal covers.
    fn z_end(&self) -> u64 {
        self.z_start + self.z.len() as u64
    }

    /// CP statistics of the symbol starting at absolute index `s`, or `None` if it is not all in.
    fn symbol_stats(&self, s: u64) -> Option<(Complex32, f32)> {
        if s < self.z_start || s + SYM_LEN as u64 > self.z_end() {
            return None;
        }
        let i = (s - self.z_start) as usize;
        let mut num = Complex32::new(0.0, 0.0);
        let mut den = 0.0f32;
        for n in i..i + CP {
            let (a, b) = (self.z[n], self.z[n + CP_LAG]);
            num += a * b.conj();
            den += 0.5 * (a.norm_sqr() + b.norm_sqr());
        }
        Some((num, den))
    }

    /// γ of the symbols `1..=k` after a preamble at `onset`, or `None` if any is not yet in.
    fn gamma_after(&self, onset: u64, k: usize) -> Option<f32> {
        let mut num = Complex32::new(0.0, 0.0);
        let mut den = 0.0f32;
        for j in 1..=k as u64 {
            let (n, d) = self.symbol_stats(onset + j * SYM_LEN as u64)?;
            num += n;
            den += d;
        }
        Some(ratio(num, den))
    }

    /// Score this read's ρ profile and add its peaks to the pending candidates.
    fn search(&mut self, read: &[f32]) {
        let base = self.pos - self.carry.len() as u64;
        let mut buf = std::mem::take(&mut self.carry);
        buf.extend_from_slice(read);
        if buf.len() >= SYM_LEN {
            let profile = self.filter.rho_profile_over_frequency(
                &buf,
                usize::MAX,
                ENERGY_FLOOR,
                SAMPLE_RATE as f32,
                &self.freqs,
            );
            for d in pick_separated_peaks(&profile, MAX_PEAKS, SYM_LEN) {
                let rho = profile[d];
                if rho < RHO_OPEN {
                    continue;
                }
                let onset = base + d as u64;
                if self
                    .last_open
                    .is_some_and(|o| o.abs_diff(onset) < SYM_LEN as u64)
                {
                    continue;
                }
                match self
                    .pending
                    .iter_mut()
                    .find(|c| c.onset.abs_diff(onset) < SYM_LEN as u64)
                {
                    // A lobe of the same preamble scored across a read boundary: keep the better one.
                    Some(c) if rho > c.rho => *c = Candidate { onset, rho },
                    Some(_) => {}
                    None => self.pending.push(Candidate { onset, rho }),
                }
            }
        }
        let keep = buf.len().min(SYM_LEN - 1);
        self.carry = buf.split_off(buf.len() - keep);
    }
}

impl CarrierDetector for OfdmCarrierDetector {
    fn observe(&mut self, read: &[f32]) -> CarrierVerdict {
        let read_start = self.pos;
        let held_at_start = self.held.is_some();
        self.search(read);
        self.extend_analytic(read);
        self.pos += read.len() as u64;
        let mut first_open: Option<u64> = None;
        let mut released = false;
        self.pending.sort_by_key(|c| c.onset);
        loop {
            // The next decidable event, in time order: a candidate's confirm, or the held frame's next
            // symbol. Each becomes decidable once its last sample is in the analytic signal.
            let confirm_at = self
                .pending
                .first()
                .map(|c| c.onset + ((CONFIRM_SYMS + 1) * SYM_LEN) as u64);
            let hold_at = self
                .held
                .as_ref()
                .map(|h| h.grid + (h.next_sym + 1) * SYM_LEN as u64);
            let z_end = self.z_end();
            let next = match (confirm_at, hold_at) {
                (Some(c), Some(h)) => c.min(h),
                (Some(c), None) => c,
                (None, Some(h)) => h,
                (None, None) => break,
            };
            if next > z_end {
                break;
            }
            if confirm_at == Some(next) {
                let c = self.pending.remove(0);
                let Some(g4) = self.gamma_after(c.onset, CONFIRM_SYMS) else {
                    // Its symbols were trimmed away (cannot happen while reads are processed in
                    // order); drop it rather than stall.
                    continue;
                };
                self.last_gamma4 = Some(g4);
                if g4 < GAMMA_CONFIRM {
                    self.confirm_rejections += 1;
                    continue;
                }
                let stats = (1..=CONFIRM_SYMS as u64)
                    .filter_map(|j| self.symbol_stats(c.onset + j * SYM_LEN as u64))
                    .collect();
                self.held = Some(Held {
                    grid: c.onset,
                    next_sym: CONFIRM_SYMS as u64 + 1,
                    stats,
                });
                self.last_open = Some(c.onset);
                self.opens += 1;
                first_open.get_or_insert(c.onset);
                continue;
            }
            // The held frame's next symbol.
            let Some(h) = self.held.as_mut() else {
                break;
            };
            let s = h.grid + h.next_sym * SYM_LEN as u64;
            h.next_sym += 1;
            let stats = match self.symbol_stats(s) {
                Some(st) => st,
                None => continue,
            };
            let Some(h) = self.held.as_mut() else {
                break;
            };
            h.stats.push_back(stats);
            if h.stats.len() > HOLD_SYMS {
                h.stats.pop_front();
            }
            let (num, den) = h
                .stats
                .iter()
                .fold((Complex32::new(0.0, 0.0), 0.0f32), |(n, d), &(a, b)| {
                    (n + a, d + b)
                });
            if ratio(num, den) < GAMMA_RELEASE {
                self.held = None;
                released = true;
            }
        }
        // Keep only the analytic signal a pending candidate or the held frame can still need.
        let mut keep_from = self.z_end().saturating_sub((2 * SYM_LEN) as u64);
        if let Some(c) = self.pending.first() {
            keep_from = keep_from.min(c.onset + SYM_LEN as u64);
        }
        if let Some(h) = &self.held {
            keep_from = keep_from.min(h.grid + h.next_sym * SYM_LEN as u64);
        }
        if keep_from > self.z_start {
            let drop = ((keep_from - self.z_start) as usize).min(self.z.len());
            self.z.drain(..drop);
            self.z_start += drop as u64;
        }
        match first_open {
            Some(o) => CarrierVerdict::Open {
                onset: (o as i64 - read_start as i64) as isize,
            },
            None if self.held.is_some() => CarrierVerdict::Held,
            None if held_at_start || released => CarrierVerdict::Released,
            None => CarrierVerdict::Idle,
        }
    }

    fn template(&self) -> &[f32] {
        &self.template
    }

    fn burst_coherence(&self, burst: &[f32]) -> Option<f32> {
        acquire(burst, &self.p, self.told).map(|a| a.coherence)
    }
}

/// `|num| / den`, 0 on an empty denominator.
fn ratio(num: Complex32, den: f32) -> f32 {
    if den > 0.0 {
        num.norm() / den
    } else {
        0.0
    }
}

/// Hamming-windowed Hilbert transformer: `2/(πn)` on odd `n`, zero on even, centred.
fn hilbert_taps() -> Vec<f32> {
    let c = HILBERT_DELAY as isize;
    (0..HILBERT_TAPS as isize)
        .map(|i| {
            let n = i - c;
            if n % 2 == 0 {
                return 0.0;
            }
            let w = 0.54
                - 0.46 * (2.0 * std::f32::consts::PI * i as f32 / (HILBERT_TAPS - 1) as f32).cos();
            w * 2.0 / (std::f32::consts::PI * n as f32)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{OFDM52, OFDM52_16QAM};
    use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

    /// Deterministic Gaussian noise (Box–Muller over an LCG).
    fn noise(n: usize, sigma: f32, seed: u32) -> Vec<f32> {
        let mut s = seed.wrapping_mul(2_654_435_761).max(1);
        let mut u = || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((s >> 8) as f32 + 1.0) / 16_777_217.0
        };
        (0..n)
            .map(|_| {
                let (a, b) = (u(), u());
                sigma * (-2.0 * a.ln()).sqrt() * (std::f32::consts::TAU * b).cos()
            })
            .collect()
    }

    fn frame(mode: &str, payload: &[u8]) -> Vec<f32> {
        let cfg = ModulationConfig {
            mode: mode.into(),
            ..ModulationConfig::default()
        };
        crate::OfdmPlugin::new().modulate(payload, &cfg).unwrap()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// Feed `x` in `read`-sample reads; return every verdict with the read's start.
    fn run(det: &mut OfdmCarrierDetector, x: &[f32], read: usize) -> Vec<(usize, CarrierVerdict)> {
        x.chunks(read)
            .enumerate()
            .map(|(i, c)| (i * read, det.observe(c)))
            .collect()
    }

    /// A frame under an AGC — total power 1.15× the gap hiss, the hiss under it 12 dB down — opens at
    /// its preamble, within a few samples, at every read size the daemon uses.
    #[test]
    fn a_frame_under_the_hiss_level_opens_at_its_preamble() {
        let tx = frame("OFDM52", &[0x5Au8; 64]);
        let lead = 20_011;
        for read in [400usize, 800, 4096, 19_200] {
            let mut x = noise(lead + tx.len() + 40_000, 0.05, 7);
            let under = noise(tx.len(), 0.05 / 4.0, 9);
            let g = (1.15f32 * 0.05 * 0.05 - (0.05f32 / 4.0).powi(2)).sqrt() / rms(&tx);
            for (i, s) in tx.iter().enumerate() {
                x[lead + i] = s * g + under[i];
            }
            let total = rms(&x[lead..lead + tx.len()]) / rms(&x[..lead]);
            assert!(total < 1.25, "the frame's total level is {total}× the hiss");
            let mut det = OfdmCarrierDetector::new(OFDM52, 0.0);
            let v = run(&mut det, &x, read);
            let opens: Vec<isize> = v
                .iter()
                .filter_map(|&(at, v)| match v {
                    CarrierVerdict::Open { onset } => Some(at as isize + onset),
                    _ => None,
                })
                .collect();
            assert_eq!(opens.len(), 1, "read {read}: opens {opens:?}");
            assert!(
                (opens[0] - lead as isize).abs() <= 8,
                "read {read}: opened at {} for a preamble at {lead}",
                opens[0]
            );
            // The last read P was up in ends past the frame, and within 8 symbols and a read of it.
            let end = lead + tx.len();
            let last_up = v
                .iter()
                .rfind(|&&(_, v)| v != CarrierVerdict::Idle)
                .map(|&(at, _)| at + read)
                .expect("P was up");
            assert!(
                last_up >= end && last_up <= end + HOLD_SYMS * SYM_LEN + read,
                "read {read}: P up until {last_up}, frame ends at {end}"
            );
            assert_eq!(v.last().map(|&(_, v)| v), Some(CarrierVerdict::Idle));
        }
    }

    /// Hiss alone never opens, and the confirm refuses a candidate the grid made out of it.
    #[test]
    fn hiss_never_opens() {
        let x = noise(8000 * 60, 0.05, 11);
        let mut det = OfdmCarrierDetector::new(OFDM52, 0.0);
        let v = run(&mut det, &x, 800);
        assert!(v.iter().all(|&(_, v)| v == CarrierVerdict::Idle));
        assert_eq!(det.opens(), 0);
    }

    /// A steady tone after a genuine preamble keeps γ₈ near 1: the detector holds, so only the engine's
    /// bound can end it (gate 5's fixture).
    #[test]
    fn a_tone_after_a_preamble_holds() {
        let pre = clipped_preamble(&OFDM52);
        let g = 0.05 / rms(&pre);
        let mut x = noise(8000, 0.02, 3);
        x.extend(pre.iter().map(|s| s * g));
        let n0 = x.len();
        x.extend(
            (0..8000 * 6)
                .map(|i| 0.05 * (std::f32::consts::TAU * 1500.0 * i as f32 / 8000.0).sin()),
        );
        for (i, s) in noise(x.len() - n0, 0.005, 5).into_iter().enumerate() {
            x[n0 + i] += s;
        }
        let mut det = OfdmCarrierDetector::new(OFDM52, 0.0);
        let v = run(&mut det, &x, 800);
        assert_eq!(det.opens(), 1);
        assert_eq!(v.last().map(|&(_, v)| v), Some(CarrierVerdict::Held));
    }

    /// The detector for every OFDM52 rung correlates against one and the same preamble.
    #[test]
    fn one_template_serves_the_ofdm52_family() {
        assert_eq!(
            OfdmCarrierDetector::new(OFDM52, 0.0).template(),
            OfdmCarrierDetector::new(OFDM52_16QAM, 0.0).template()
        );
    }

    /// The FIR's analytic signal turns a tone in band into a constant-modulus phasor: the quadrature
    /// branch is the Hilbert transform, so γ cannot collapse with the carrier offset as a real-valued
    /// lag product does.
    #[test]
    fn the_analytic_signal_is_a_phasor_in_band() {
        for f in [600.0f32, 1500.0, 2400.0] {
            let x: Vec<f32> = (0..4000)
                .map(|i| (std::f32::consts::TAU * f * i as f32 / 8000.0).cos())
                .collect();
            let mut det = OfdmCarrierDetector::new(OFDM52, 0.0);
            det.extend_analytic(&x);
            let mags: Vec<f32> = det.z.iter().skip(200).map(|z| z.norm()).collect();
            let (lo, hi) = mags
                .iter()
                .fold((f32::MAX, 0.0f32), |(l, h), &m| (l.min(m), h.max(m)));
            assert!(lo > 0.97 && hi < 1.03, "{f} Hz: |z| in {lo}..{hi}");
        }
    }
}
