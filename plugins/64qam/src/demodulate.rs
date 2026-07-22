//! 64QAM demodulator.
//!
//! Pipeline: downmix → rectangular integration or RRC matched-filter + Gardner
//! → nearest-point decision → bit extraction.

use std::f32::consts::PI;

use num_complex::Complex32;
use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, PulseShape};
use openpulse_dsp::acquisition::{estimate_cfo_data_aided, preamble_corr_sq};
use openpulse_dsp::constellation::{
    constellation_points, estimate_decision_noise_var, symbol_llrs,
};
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::rrc::generate_rrc_coefficients;

use crate::modulate::{
    preamble_symbols, samples_per_symbol, PAM8_SCALE, PREAMBLE_SYMS, RRC_SPAN_SYMBOLS, TAIL_SYMS,
};
use crate::parse_baud_rate;

/// Data-aided AFC estimation against the known corner preamble.
///
/// The blind 4th-power method previously used here has heavy self-noise on a
/// non-constant-modulus 64QAM symbol stream (the data symbols sit at angles
/// that are not multiples of 90°, so the 4th power does not strip them
/// cleanly — this also rules out a 4th-power Goertzel coarse stage).  The 16
/// corner preamble symbols ARE constant-modulus, so the data-aided estimator
/// is both wide-range (±baud/2 = ±250 Hz at 64QAM500 up to ±1000 Hz at
/// 64QAM2000) and free of constellation self-noise.
pub fn afc_estimate_hz(samples: &[f32], config: &ModulationConfig) -> Option<f32> {
    let baud = parse_baud_rate(&config.mode).ok()?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud).ok()?;

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return None;
    }

    let offset = find_timing_offset(samples, n, fc, fs);
    let (i_syms, q_syms) = demodulate_iq(samples, n, fc, fs, offset);
    if i_syms.len() < 2 {
        return None;
    }

    estimate_cfo_data_aided(&i_syms, &q_syms, &preamble_symbols(), baud)
}

// ── Nearest-point decision ────────────────────────────────────────────────────

/// Inverse PAM-8 Gray map: normalised amplitude → 3-bit Gray code (0–7).
///
/// Quantises `amp` to the nearest of the 8 levels {±1,±3,±5,±7}×scale,
/// then returns the corresponding Gray code.
fn pam8_decide(amp: f32) -> u8 {
    // Thresholds are midpoints between adjacent amplitude levels.
    // Levels (scaled): ±0.154, ±0.463, ±0.772, ±1.080 (PAM8_SCALE × odd integers)
    let a = amp / PAM8_SCALE; // un-scale to ±{1,3,5,7} range
    let level: i8 = if a <= -6.0 {
        -7
    } else if a <= -4.0 {
        -5
    } else if a <= -2.0 {
        -3
    } else if a <= 0.0 {
        -1
    } else if a <= 2.0 {
        1
    } else if a <= 4.0 {
        3
    } else if a <= 6.0 {
        5
    } else {
        7
    };
    // Level → Gray code (inverse of pam8_amplitude).
    match level {
        -7 => 0b000,
        -5 => 0b001,
        -3 => 0b011,
        -1 => 0b010,
        1 => 0b110,
        3 => 0b111,
        5 => 0b101,
        7 => 0b100,
        _ => unreachable!(),
    }
}

/// Hard-decision: map (I,Q) to the nearest 64QAM constellation point bits (6 bits).
fn qam64_decide_bits(i: f32, q: f32) -> u8 {
    (pam8_decide(i) << 3) | pam8_decide(q)
}

/// Nearest PAM-8 amplitude {±1,±3,±5,±7}×scale to `amp`.
fn pam8_nearest_amplitude(amp: f32) -> f32 {
    let a = (amp / PAM8_SCALE).round();
    // Snap to the nearest odd integer in [-7, 7].
    let odd = (((a - 1.0) / 2.0).round() * 2.0 + 1.0).clamp(-7.0, 7.0);
    odd * PAM8_SCALE
}

/// Decision-directed carrier tracking loop for 64QAM, seeded with an initial loop
/// frequency; also returns the final loop frequency (rad/symbol) for the two-pass
/// variant.
///
/// 64QAM is not constant-modulus, so an M-PSK Costas loop cannot be used.  This
/// second-order loop instead derives its phase error from the decision: for each
/// symbol it de-rotates by the running phase estimate, decides the nearest
/// constellation point, and uses Im(r·conj(decision)) as the error.  `carrier_phase_correct`
/// has already removed the static offset, so the loop only has to track the residual
/// frequency drift (e.g. the difference in USB-audio crystal frequencies between two
/// stations), which static correction cannot follow across a frame.
fn dd_carrier_track_seeded(
    i_syms: &[f32],
    q_syms: &[f32],
    loop_bw: f32,
    init_freq: f32,
) -> (Vec<f32>, Vec<f32>, f32) {
    let alpha = loop_bw;
    let beta = loop_bw * loop_bw * 0.25;
    let mut phase = 0.0f32;
    let mut freq = init_freq;
    let mut i_out = Vec::with_capacity(i_syms.len());
    let mut q_out = Vec::with_capacity(q_syms.len());
    for (&i, &q) in i_syms.iter().zip(q_syms.iter()) {
        let (s, c) = (-phase).sin_cos();
        let di = i * c - q * s;
        let dq = i * s + q * c;
        let pi = pam8_nearest_amplitude(di);
        let pq = pam8_nearest_amplitude(dq);
        let denom = pi * pi + pq * pq;
        let err = if denom > 1e-6 {
            (dq * pi - di * pq) / denom
        } else {
            0.0
        };
        i_out.push(di);
        q_out.push(dq);
        freq += beta * err;
        phase += freq + alpha * err;
    }
    (i_out, q_out, freq)
}

/// Two-pass decision-directed carrier tracking.
///
/// A single forward loop seeded at zero spends most of a short frame *acquiring* a
/// residual frequency offset — e.g. the ~`eps·fc` term a TX/RX sample-rate offset
/// imposes — leaving the early symbols rotated and erroring on the dense 64QAM grid.
/// Pass 1 converges the offset over the whole frame; pass 2 re-runs the loop seeded
/// with that frequency so every symbol, including the first, is de-rotated at the
/// correct rate. On an offset-free (clean/AWGN) frame pass 1 converges to ~0, so the
/// second pass is a no-op and nothing regresses. Cuts 64QAM500 byte errors at
/// 100 ppm sample-rate offset from ~6.2 % to ~2.1 % (inside soft-FEC capacity).
fn dd_carrier_track_2pass(i_syms: &[f32], q_syms: &[f32], loop_bw: f32) -> (Vec<f32>, Vec<f32>) {
    let (_, _, freq) = dd_carrier_track_seeded(i_syms, q_syms, loop_bw, 0.0);
    let (i_out, q_out, _) = dd_carrier_track_seeded(i_syms, q_syms, loop_bw, freq);
    (i_out, q_out)
}

/// Data-aided AGC: scale the symbol stream so the corner-preamble power matches
/// the transmitted constellation scale.
///
/// The soft LLR engine (fixed `noise_var`), the absolute PAM-8 thresholds, and the
/// decision-directed carrier loop (whose error uses decided amplitudes) all assume
/// the transmitted amplitude scale. But the symbol integrators are proportional to
/// input amplitude and do not normalise, so inter-station level spread and QSB
/// fading on HF — exactly the level variation an AGC exists to remove — mis-scale
/// every symbol and break the dense-grid demap. The known equal-magnitude corner
/// preamble is the amplitude reference. At a matched (loopback) level the gain is
/// ≈ 1, so this is a no-op there and cannot regress the existing loopback paths.
fn normalize_to_constellation(i_syms: &[f32], q_syms: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let expected = preamble_symbols();
    let n = expected.len().min(i_syms.len()).min(q_syms.len());
    let gain = if n == 0 {
        1.0
    } else {
        let ref_pow: f32 = expected[..n]
            .iter()
            .map(|(i, q)| i * i + q * q)
            .sum::<f32>()
            / n as f32;
        let recv_pow: f32 = (0..n)
            .map(|k| i_syms[k] * i_syms[k] + q_syms[k] * q_syms[k])
            .sum::<f32>()
            / n as f32;
        if recv_pow > 1e-12 && ref_pow > 0.0 {
            (ref_pow / recv_pow).sqrt()
        } else {
            1.0
        }
    };
    (
        i_syms.iter().map(|&x| x * gain).collect(),
        q_syms.iter().map(|&x| x * gain).collect(),
    )
}

/// Half-width, in symbols, of the window each local level estimate is taken over.
const GAIN_TRACK_HALF_WIN: usize = 48;

/// How many standard errors of the level estimator a correction must clear before any of it is applied.
const GAIN_TRACK_SHRINK_SIGMAS: f32 = 2.5;

/// Track the amplitude reference *across* the frame, not just at its start.
///
/// `normalize_to_constellation` fits ONE scalar gain from the 16-symbol preamble and applies it to
/// every symbol. That is right for inter-station level spread, which is constant, and wrong for
/// anything that moves the level *during* a frame — a capture-side soundcard AGC riding its own
/// attack/decay, or HF QSB. 64QAM carries three of its six bits per axis in amplitude, so a level
/// that drifts mid-frame slides the outer PAM-8 rings across their decision boundaries while the
/// phase is still perfect. The carrier loop already tracks phase mid-frame; nothing tracked
/// amplitude.
///
/// Ablated with **no AWGN at all**, so no cell below is a noise limitation. `64QAM500` byte error
/// rate under sinusoidal wander, before → after this pass:
///
/// | wander | depth 0.05 | 0.15 | 0.30 |
/// |---|---|---|---|
/// | gain, 0.5 Hz | 0.000 → 0.000 | 0.000 → 0.000 | 0.341 → 0.341 |
/// | gain, 2 Hz | 0.000 → 0.000 | **0.102 → 0.000** | **0.318 → 0.094** |
/// | phase, 0.5 Hz | 0.000 | 0.000 | 0.027 |
/// | phase, 2 Hz | 0.000 | 0.125 | 0.447 |
///
/// The phase row is the control that made this worth writing: at the slow rates a soundcard AGC
/// actually produces, gain wander is an order of magnitude more damaging than the same fractional
/// phase wander (0.341 vs 0.027 at 0.5 Hz), and it was the one nothing corrected.
///
/// **What this does not fix**, and why it is left alone. The pass removes the *variation* in level
/// across the frame; it does not move the frame's absolute scale, so it cannot help when the
/// preamble happens to sit at a different level from the frame average — the 0.5 Hz / 0.30 column,
/// where a frame spanning less than one wander cycle is uniformly mis-scaled. Re-anchoring
/// afterwards (re-running the static preamble fit on the now-flat frame) does fix that column, and
/// was tried: it broke `window_arq_engine_path_across_mode_families` with an RS `TooManyErrors` on a
/// clean loopback, because on a matched level the static fit is only *approximately* unity and
/// applying it twice compounds the residual into a systematic scale error the dense grid cannot
/// absorb. A correction that costs a clean frame to rescue an extreme one is the wrong trade.
///
/// Blind rather than decision-directed, which was also measured: a DD version (project each symbol
/// onto its decided point, smooth the ratios) helps where decisions are already mostly right and
/// *amplifies* the error where they are not, because the wrong decisions feed straight back into the
/// gain — it took `64QAM2000-RRC` at 2 Hz/0.15 from 0.012 to 0.102. A level estimate must not depend
/// on the decisions it is about to correct.
fn track_gain_across_frame(i_syms: &[f32], q_syms: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let n = i_syms.len();
    // The level is read from the DATA symbols only. The preamble is equal-magnitude corner symbols
    // and the tail is not payload either, so both sit far off the 64QAM average power — letting them
    // into the windows makes the estimator read a level step at each frame edge and "correct" it,
    // which is worse than not tracking at all.
    if n <= PREAMBLE_SYMS + TAIL_SYMS {
        return (i_syms.to_vec(), q_syms.to_vec());
    }
    let (lo_d, hi_d) = (PREAMBLE_SYMS, n - TAIL_SYMS);
    let m = hi_d - lo_d;
    if m < 2 * GAIN_TRACK_HALF_WIN + 1 {
        return (i_syms.to_vec(), q_syms.to_vec());
    }

    let pow: Vec<f32> = (lo_d..hi_d)
        .map(|k| i_syms[k] * i_syms[k] + q_syms[k] * q_syms[k])
        .collect();
    let mut cum = vec![0.0f32; m + 1];
    for k in 0..m {
        cum[k + 1] = cum[k] + pow[k];
    }
    let frame_mean = cum[m] / m as f32;
    if frame_mean <= 1e-9 {
        return (i_syms.to_vec(), q_syms.to_vec());
    }

    // Windowed mean at data index `j`; symbols outside the data range take the nearest edge value.
    let window_mean = |j: usize| {
        let j = j.min(m - 1);
        let lo = j.saturating_sub(GAIN_TRACK_HALF_WIN);
        let hi = (j + GAIN_TRACK_HALF_WIN + 1).min(m);
        (cum[hi] - cum[lo]) / (hi - lo) as f32
    };

    // Noise floor of a windowed level estimate, from the spread of each symbol's power about its OWN
    // window mean. About the window mean and not the frame mean on purpose: the frame-mean spread
    // also contains the wander being tracked, so using it makes the deadband grow with the very
    // signal it is meant to let through, and the correction vanishes exactly when it is needed.
    let resid_var = (0..m)
        .map(|j| (pow[j] - window_mean(j)).powi(2))
        .sum::<f32>()
        / m as f32;
    let win = (2 * GAIN_TRACK_HALF_WIN + 1) as f32;
    // Standard error of a windowed POWER mean as a fraction; halved to convert to amplitude.
    let se_amp = 0.5 * resid_var.sqrt() / (frame_mean * win.sqrt());
    let deadband = GAIN_TRACK_SHRINK_SIGMAS * se_amp;

    let mut i_out = Vec::with_capacity(n);
    let mut q_out = Vec::with_capacity(n);
    for k in 0..n {
        let mean = window_mean(k.saturating_sub(lo_d));
        // Referenced to the frame mean, not to the constellation's nominal power: this pass only
        // de-trends the level ACROSS the frame. An absolute reference would fold the noise power
        // into the estimate and systematically shrink the constellation at low SNR — a 4.7 % squeeze
        // at 10 dB, a third of the outer ring's decision margin — moving the LLR calibration with it.
        let g = if mean > 1e-9 {
            (frame_mean / mean).sqrt()
        } else {
            1.0
        };
        let d = g - 1.0;
        let shrunk = (1.0 + d.signum() * (d.abs() - deadband).max(0.0)).clamp(0.75, 1.25);
        i_out.push(i_syms[k] * shrunk);
        q_out.push(q_syms[k] * shrunk);
    }
    // NOT re-anchored, deliberately — see the "what this does not fix" note on the function.
    (i_out, q_out)
}

// ── IQ demodulation (rectangular integration path) ───────────────────────────
//
// Deliberately rectangular, NOT the half-Hann window used by the PSK plugins:
// the modulator's non-RRC path emits rectangular-windowed symbols (see
// modulate.rs — the Hann crossfade would blur 8 distinct amplitude levels per
// axis across symbol boundaries), so rectangular integration IS the matched
// filter here.

fn demodulate_iq(
    samples: &[f32],
    n: usize,
    fc: f32,
    fs: f32,
    offset: usize,
) -> (Vec<f32>, Vec<f32>) {
    let two_pi = 2.0 * PI;
    let mut i_syms = Vec::new();
    let mut q_syms = Vec::new();
    let start = offset;
    let mut sym_start = start;

    while sym_start + n <= samples.len() {
        let mut i_acc = 0.0f32;
        let mut q_acc = 0.0f32;
        for k in 0..n {
            let t = (sym_start + k) as f32 / fs;
            let s = samples[sym_start + k];
            i_acc += s * (two_pi * fc * t).cos();
            q_acc -= s * (two_pi * fc * t).sin();
        }
        i_syms.push(i_acc * 2.0 / n as f32);
        q_syms.push(q_acc * 2.0 / n as f32);
        sym_start += n;
    }
    (i_syms, q_syms)
}

fn find_timing_offset(samples: &[f32], n: usize, fc: f32, fs: f32) -> usize {
    let expected_syms = preamble_symbols();
    let mut best_off = 0usize;
    let mut best_score = f32::NEG_INFINITY;
    for off in 0..n {
        if samples.len() < off + n * PREAMBLE_SYMS {
            break;
        }
        // Demodulate ONLY the preamble span at this offset (the full slice may
        // be multi-second; demodulating all of it per offset is O(offsets × N)).
        let span_end = (off + n * PREAMBLE_SYMS).min(samples.len());
        let (i_v, q_v) = demodulate_iq(&samples[..span_end], n, fc, fs, off);
        if i_v.len() < PREAMBLE_SYMS || q_v.len() < PREAMBLE_SYMS {
            continue;
        }
        // Squared magnitude of the complex preamble correlation Σ r_k·conj(e_k).  The
        // signed real part collapses to 0 when the unknown carrier phase is near
        // 90°/270°, letting a wrong offset win; |·|² is rotation-invariant.
        let received: Vec<(f32, f32)> = i_v
            .iter()
            .zip(q_v.iter())
            .take(PREAMBLE_SYMS)
            .map(|(&i, &q)| (i, q))
            .collect();
        let score = preamble_corr_sq(&received, &expected_syms);
        if score > best_score {
            best_score = score;
            best_off = off;
        }
    }
    best_off
}

// ── RRC demodulation path ─────────────────────────────────────────────────────

fn qam64_demodulate_rrc(
    samples: &[f32],
    n: usize,
    baud: f32,
    fc: f32,
    fs: f32,
    alpha: f32,
) -> (Vec<f32>, Vec<f32>) {
    let two_pi = 2.0 * PI;
    let num_taps = RRC_SPAN_SYMBOLS * n + 1;
    let coeffs = generate_rrc_coefficients(fs, baud, alpha, num_taps);
    let group_delay = (num_taps - 1) / 2;

    let i_mix: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(k, &s)| s * (two_pi * fc * k as f32 / fs).cos() * 2.0)
        .collect();
    let q_mix: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(k, &s)| -s * (two_pi * fc * k as f32 / fs).sin() * 2.0)
        .collect();

    let rrc_filter = |mix: Vec<f32>| -> Vec<f32> {
        let padded: Vec<f32> = mix
            .iter()
            .copied()
            .chain(std::iter::repeat_n(0.0, group_delay))
            .collect();
        let mut fir = FirFilter::new(coeffs.clone());
        let filtered = fir.apply(&padded);
        filtered[group_delay..].to_vec()
    };

    let i_bb = rrc_filter(i_mix);
    let q_bb = rrc_filter(q_mix);

    let initial_timing = find_timing_offset_bb(&i_bb, &q_bb, n);

    // De-rotate the whole baseband to the correct carrier phase BEFORE Gardner.  The
    // Gardner TED runs on the I channel; an uncorrected carrier phase mixes I and Q,
    // mistiming the loop and corrupting every sample (a downstream phase correction
    // cannot recover mistimed symbols).  The corner preamble is a known pilot.
    let phase_0 = coarse_baseband_phase(&i_bb, &q_bb, n, initial_timing);
    let (sin0, cos0) = (-phase_0).sin_cos();

    // Fixed-stride sampling: see the QPSK/8PSK RRC paths for why an
    // interpolating timing loop is deliberately NOT used on the short
    // high-baud frames (multipath-biased Gardner error vs negligible SRO).
    let start = initial_timing.min(i_bb.len());
    let mut i_out = Vec::new();
    let mut q_out = Vec::new();
    let mut pos = start;
    while pos < i_bb.len() {
        let raw_i = i_bb[pos];
        let raw_q = q_bb.get(pos).copied().unwrap_or(0.0);
        i_out.push(raw_i * cos0 - raw_q * sin0);
        q_out.push(raw_i * sin0 + raw_q * cos0);
        pos += n;
    }
    (i_out, q_out)
}

/// Coarse carrier phase from the corner preamble samples of the RRC baseband.
fn coarse_baseband_phase(i_bb: &[f32], q_bb: &[f32], n: usize, timing: usize) -> f32 {
    let expected = preamble_symbols();
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (s, &(ei, eq)) in expected.iter().enumerate().take(PREAMBLE_SYMS) {
        let idx = timing + s * n;
        if idx >= i_bb.len() {
            break;
        }
        let (ri, rq) = (i_bb[idx], q_bb[idx]);
        re += ri * ei + rq * eq;
        im += rq * ei - ri * eq;
    }
    im.atan2(re)
}

fn find_timing_offset_bb(i_bb: &[f32], q_bb: &[f32], n: usize) -> usize {
    let expected = preamble_symbols();
    let mut best_off = 0usize;
    let mut best_score = f32::NEG_INFINITY;
    let mut received = vec![(0.0f32, 0.0f32); PREAMBLE_SYMS];
    for off in 0..n {
        if i_bb.len() < off + n * PREAMBLE_SYMS {
            break;
        }
        // Squared magnitude of the complex correlation — invariant to the residual
        // carrier phase left after downmix.
        for (s, slot) in received.iter_mut().enumerate() {
            *slot = (i_bb[off + s * n], q_bb[off + s * n]);
        }
        let score = preamble_corr_sq(&received, &expected);
        if score > best_score {
            best_score = score;
            best_off = off;
        }
    }
    best_off
}

// ── Carrier phase recovery ────────────────────────────────────────────────────

/// Phase of the complex preamble correlation Σ r_k·conj(e_k) over `range`.
///
/// The 64QAM preamble uses only the four equal-magnitude corners, so this vector
/// sum is a clean ML phase estimate (robust to crossfade ISI via the small 1-lag
/// autocorrelation of the designed corner sequence).
fn preamble_corr_phase(
    i_syms: &[f32],
    q_syms: &[f32],
    expected: &[(f32, f32)],
    range: std::ops::Range<usize>,
) -> f32 {
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for k in range {
        let (ri, rq) = (i_syms[k], q_syms[k]);
        let (ei, eq) = expected[k];
        re += ri * ei + rq * eq;
        im += rq * ei - ri * eq;
    }
    im.atan2(re)
}

/// Remove the static carrier phase offset (and, when AFC is active, a linear drift)
/// using the known corner preamble as a pilot.
///
/// 64QAM has no constant modulus, so a Costas/decision-directed loop is unreliable;
/// instead the equal-magnitude corner preamble gives a direct phase reference.  The
/// offset is *always* removed (no small-angle skip): the dense 64QAM constellation
/// has a tiny angular margin, so even a few degrees of uncorrected rotation flips
/// outer points.  The drift term is applied only when the engine signalled a real RF
/// offset (`afc_correction_hz` ≥ 0.5 Hz).
fn carrier_phase_correct(
    i_syms: &[f32],
    q_syms: &[f32],
    afc_correction_hz: f32,
) -> (Vec<f32>, Vec<f32>) {
    let expected = preamble_symbols();
    let p = i_syms
        .len()
        .min(q_syms.len())
        .min(expected.len())
        .min(PREAMBLE_SYMS);
    if p < 4 {
        return (i_syms.to_vec(), q_syms.to_vec());
    }

    let (phase_0, drift) = if afc_correction_hz.abs() >= 0.5 {
        let half = p / 2;
        let p_a = preamble_corr_phase(i_syms, q_syms, &expected, 0..half);
        let p_b = preamble_corr_phase(i_syms, q_syms, &expected, half..p);
        let k_a = (half - 1) as f32 / 2.0;
        let k_b = half as f32 + (p - half - 1) as f32 / 2.0;
        let mut dphi = p_b - p_a;
        while dphi > PI {
            dphi -= 2.0 * PI;
        }
        while dphi < -PI {
            dphi += 2.0 * PI;
        }
        let drift = dphi / (k_b - k_a);
        (p_a - drift * k_a, drift)
    } else {
        (preamble_corr_phase(i_syms, q_syms, &expected, 0..p), 0.0)
    };

    let mut i_out = Vec::with_capacity(i_syms.len());
    let mut q_out = Vec::with_capacity(q_syms.len());
    for (k, (&i, &q)) in i_syms.iter().zip(q_syms.iter()).enumerate() {
        let theta = -(phase_0 + drift * k as f32);
        let (s, c) = theta.sin_cos();
        i_out.push(i * c - q * s);
        q_out.push(i * s + q * c);
    }
    (i_out, q_out)
}

// ── Symbol → bytes ────────────────────────────────────────────────────────────

fn symbols_to_bytes(i_syms: &[f32], q_syms: &[f32]) -> Vec<u8> {
    let mut bits = Vec::new();
    for (&i, &q) in i_syms.iter().zip(q_syms.iter()) {
        let b = qam64_decide_bits(i, q);
        for shift in 0..6u8 {
            bits.push((b >> shift) & 1 == 1);
        }
    }
    // Pack bits LSB-first into bytes.
    bits.chunks(8)
        .map(|c| {
            let mut byte = 0u8;
            for (i, &bit) in c.iter().enumerate() {
                if bit {
                    byte |= 1 << i;
                }
            }
            byte
        })
        .collect()
}

// ── Public demodulation entry points ─────────────────────────────────────────

pub fn qam64_demodulate(samples: &[f32], config: &ModulationConfig) -> Result<Vec<u8>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return Err(ModemError::Demodulation("signal too short".into()));
    }

    let (i_syms, q_syms) = if let Some(alpha) = rrc_alpha(config) {
        qam64_demodulate_rrc(samples, n, baud, fc, fs, alpha)
    } else {
        let offset = find_timing_offset(samples, n, fc, fs);
        demodulate_iq(samples, n, fc, fs, offset)
    };
    let (i_syms, q_syms) = carrier_phase_correct(&i_syms, &q_syms, config.afc_correction_hz);
    // Data-aided AGC: normalise the symbol level to the constellation scale before
    // the amplitude-sensitive DD carrier loop and demap (no-op at a matched level).
    let (i_syms, q_syms) = normalize_to_constellation(&i_syms, &q_syms);
    // Track residual carrier frequency drift across the frame (hardware crystal
    // offset); static phase correction alone cannot follow it on the dense grid.
    let (i_syms, q_syms) = dd_carrier_track_2pass(&i_syms, &q_syms, 0.01);
    // Then track the AMPLITUDE reference across the frame; the preamble fit above is static.
    let (i_syms, q_syms) = track_gain_across_frame(&i_syms, &q_syms);

    if i_syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".into(),
        ));
    }

    let data_start = PREAMBLE_SYMS;
    let data_end = i_syms.len() - TAIL_SYMS;
    Ok(symbols_to_bytes(
        &i_syms[data_start..data_end],
        &q_syms[data_start..data_end],
    ))
}

/// Soft demodulator: max-log-MAP LLRs, one per bit (6 bits per symbol).
///
/// For each bit position k (0–5), the LLR is computed as the minimum squared
/// Euclidean distance to all 64QAM points with bit k=0, minus the minimum to
/// all points with bit k=1, scaled to σ²=1.
pub fn qam64_demodulate_soft(
    samples: &[f32],
    config: &ModulationConfig,
) -> Result<Vec<f32>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return Err(ModemError::Demodulation("signal too short".into()));
    }

    let (i_syms, q_syms) = if let Some(alpha) = rrc_alpha(config) {
        qam64_demodulate_rrc(samples, n, baud, fc, fs, alpha)
    } else {
        let offset = find_timing_offset(samples, n, fc, fs);
        demodulate_iq(samples, n, fc, fs, offset)
    };
    let (i_syms, q_syms) = carrier_phase_correct(&i_syms, &q_syms, config.afc_correction_hz);
    // Data-aided AGC: normalise the symbol level to the constellation scale before
    // the amplitude-sensitive DD carrier loop and demap (no-op at a matched level).
    let (i_syms, q_syms) = normalize_to_constellation(&i_syms, &q_syms);
    // Track residual carrier frequency drift across the frame (hardware crystal
    // offset); static phase correction alone cannot follow it on the dense grid.
    let (i_syms, q_syms) = dd_carrier_track_2pass(&i_syms, &q_syms, 0.01);
    // Then track the AMPLITUDE reference across the frame; the preamble fit above is static.
    let (i_syms, q_syms) = track_gain_across_frame(&i_syms, &q_syms);

    if i_syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".into(),
        ));
    }

    let data_start = PREAMBLE_SYMS;
    let data_end = i_syms.len() - TAIL_SYMS;
    let mut llrs = Vec::with_capacity((data_end - data_start) * 6);

    // Max-log-MAP LLRs via the shared constellation engine (positive LLR → bit more likely 0).
    //
    // `noise_var` is measured from the symbols rather than fixed at 1.0, which makes these *true*
    // log-likelihood ratios: their magnitude scales as 1/σ². Nothing that decodes a single frame
    // cares (soft Viterbi, min-sum LDPC and max-log turbo are all scale-invariant), but HARQ soft
    // combining across receive attempts does — an uncalibrated attempt from a deep fade otherwise
    // votes as loudly as a clean one. See `openpulse_core::fec::combine_llrs_map`.
    let points = constellation_points(6);
    let data: Vec<Complex32> = i_syms[data_start..data_end]
        .iter()
        .zip(q_syms[data_start..data_end].iter())
        .map(|(&yi, &yq)| Complex32::new(yi, yq))
        .collect();
    // Measure σ² from the known corner preamble, not a decision-directed estimate over the data.
    // On the dense 64QAM grid, distance-to-nearest-point saturates once symbols cross a decision
    // boundary (the wrong-but-near point is close), so it under-reads σ² 2–5× at 6–14 dB and the
    // LLRs come out badly over-confident — measured at 24× the promised error rate at 10 dB.
    // The corner preamble is a known, constant-modulus reference; its residual is an unbiased σ².
    let noise_var = preamble_noise_var(&i_syms, &q_syms)
        .unwrap_or_else(|| estimate_decision_noise_var(&data, 6));
    for sym in &data {
        llrs.extend(symbol_llrs(*sym, 6, noise_var, &points));
    }
    Ok(llrs)
}

/// Data-aided 2-D noise variance `E|n|²` from the recovered corner preamble.
///
/// The preamble symbols (already level- and carrier-corrected, on the constellation scale) are a
/// known reference, so the mean squared deviation from [`preamble_symbols`] measures the additive
/// noise directly — unbiased at any SNR, unlike the decision-directed estimate that saturates on
/// the dense grid. Returns `None` if no preamble symbols are available. The unit matches
/// [`estimate_decision_noise_var`] (2-D, i.e. `2σ²` per dimension), as [`symbol_llrs`] expects.
fn preamble_noise_var(i_syms: &[f32], q_syms: &[f32]) -> Option<f32> {
    let expected = preamble_symbols();
    let n = expected.len().min(i_syms.len()).min(q_syms.len());
    if n == 0 {
        return None;
    }
    let sum: f32 = (0..n)
        .map(|k| {
            let di = i_syms[k] - expected[k].0;
            let dq = q_syms[k] - expected[k].1;
            di * di + dq * dq
        })
        .sum();
    Some((sum / n as f32).max(1e-6))
}

/// GPU-accelerated soft demodulator. Falls back to the CPU path if the GPU
/// returns `None`.
#[cfg(feature = "gpu")]
pub fn qam64_demodulate_soft_gpu(
    samples: &[f32],
    config: &ModulationConfig,
    ctx: &std::sync::Arc<openpulse_gpu::GpuContext>,
) -> Result<Vec<f32>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return Err(ModemError::Demodulation("signal too short".into()));
    }

    let (i_syms, q_syms) = if let Some(alpha) = rrc_alpha(config) {
        qam64_demodulate_rrc(samples, n, baud, fc, fs, alpha)
    } else {
        let offset = find_timing_offset(samples, n, fc, fs);
        demodulate_iq(samples, n, fc, fs, offset)
    };
    let (i_syms, q_syms) = carrier_phase_correct(&i_syms, &q_syms, config.afc_correction_hz);
    // Data-aided AGC: normalise the symbol level to the constellation scale before
    // the amplitude-sensitive DD carrier loop and demap (no-op at a matched level).
    let (i_syms, q_syms) = normalize_to_constellation(&i_syms, &q_syms);
    // Track residual carrier frequency drift across the frame (hardware crystal
    // offset); static phase correction alone cannot follow it on the dense grid.
    let (i_syms, q_syms) = dd_carrier_track_2pass(&i_syms, &q_syms, 0.01);
    // Then track the AMPLITUDE reference across the frame; the preamble fit above is static.
    let (i_syms, q_syms) = track_gain_across_frame(&i_syms, &q_syms);

    if i_syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".into(),
        ));
    }

    let data_start = PREAMBLE_SYMS;
    let data_end = i_syms.len() - TAIL_SYMS;
    let syms: Vec<(f32, f32)> = i_syms[data_start..data_end]
        .iter()
        .zip(q_syms[data_start..data_end].iter())
        .map(|(&i, &q)| (i, q))
        .collect();

    let constellation: Vec<(f32, f32)> = (0..64u8).map(crate::modulate::gray_map_64qam).collect();
    let bit_table: Vec<u32> = (0..64u32).collect();

    if let Some(mut llrs) = openpulse_gpu::gpu_soft_demod(ctx, &syms, &constellation, &bit_table, 6)
    {
        // The kernel emits σ²=1 max-log distance differences; the CPU path divides these by the
        // noise variance (`symbol_llrs`) to make them true LLRs. Apply the identical scaling here —
        // measured from the same corner-preamble residual — so the GPU path is calibrated too and
        // HARQ combining weights its attempts correctly.
        let noise_var = preamble_noise_var(&i_syms, &q_syms).unwrap_or_else(|| {
            let data: Vec<Complex32> = syms.iter().map(|&(i, q)| Complex32::new(i, q)).collect();
            estimate_decision_noise_var(&data, 6)
        });
        let inv = 1.0 / noise_var.max(1e-6);
        for l in &mut llrs {
            *l *= inv;
        }
        return Ok(llrs);
    }
    // GPU returned None — fall back to CPU.
    qam64_demodulate_soft(samples, config)
}

/// GPU-accelerated hard demodulator.  Uses GPU RRC FIR for the matched filter;
/// timing recovery and symbol decisions remain on CPU.
/// Falls back to the CPU path if the GPU returns `None`.
#[cfg(feature = "gpu")]
pub fn qam64_demodulate_gpu(
    samples: &[f32],
    config: &ModulationConfig,
    ctx: &std::sync::Arc<openpulse_gpu::GpuContext>,
) -> Option<Result<Vec<u8>, ModemError>> {
    let alpha = rrc_alpha(config)?; // only accelerate RRC modes

    let baud = parse_baud_rate(&config.mode).ok()?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud).ok()?;

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return Some(Err(ModemError::Demodulation("signal too short".into())));
    }

    let two_pi = 2.0 * std::f32::consts::PI;
    let num_taps = RRC_SPAN_SYMBOLS * n + 1;
    let coeffs = generate_rrc_coefficients(fs, baud, alpha, num_taps);
    let group_delay = (num_taps - 1) / 2;

    let i_mix: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(k, &s)| s * (two_pi * fc * k as f32 / fs).cos() * 2.0)
        .collect();
    let q_mix: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(k, &s)| -s * (two_pi * fc * k as f32 / fs).sin() * 2.0)
        .collect();

    let gpu_rrc = |mix: Vec<f32>| -> Option<Vec<f32>> {
        let padded: Vec<f32> = mix
            .iter()
            .copied()
            .chain(std::iter::repeat_n(0.0, group_delay))
            .collect();
        let filtered = openpulse_gpu::gpu_rrc_fir(ctx, &padded, &coeffs)?;
        Some(filtered[group_delay..].to_vec())
    };

    let i_bb = gpu_rrc(i_mix)?;
    let q_bb = gpu_rrc(q_mix)?;

    let initial_timing = find_timing_offset_bb(&i_bb, &q_bb, n);
    let start = initial_timing.min(i_bb.len());
    let mut i_out = Vec::new();
    let mut q_out = Vec::new();
    let mut pos = start;
    while pos < i_bb.len() {
        i_out.push(i_bb[pos]);
        q_out.push(q_bb.get(pos).copied().unwrap_or(0.0));
        pos += n;
    }

    if i_out.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Some(Err(ModemError::Demodulation(
            "no data symbols after preamble".into(),
        )));
    }

    let data_start = PREAMBLE_SYMS;
    let data_end = i_out.len() - TAIL_SYMS;
    Some(Ok(symbols_to_bytes(
        &i_out[data_start..data_end],
        &q_out[data_start..data_end],
    )))
}

fn rrc_alpha(config: &ModulationConfig) -> Option<f32> {
    if let PulseShape::Rrc { alpha } = config.pulse_shape {
        Some(alpha)
    } else if config.mode.ends_with("-RRC") {
        Some(0.35f32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modulate::{gray_map_64qam, pam8_amplitude};

    #[test]
    fn pam8_decide_all_levels_correct() {
        for gray in 0..8u8 {
            let amp = pam8_amplitude(gray);
            assert_eq!(
                pam8_decide(amp),
                gray,
                "pam8 round-trip failed for gray={gray:03b}"
            );
        }
    }

    #[test]
    fn qam64_decide_all_symbols_correct() {
        for sym in 0..64u8 {
            let (i, q) = gray_map_64qam(sym);
            assert_eq!(
                qam64_decide_bits(i, q),
                sym,
                "64QAM round-trip failed for sym={sym:06b}"
            );
        }
    }

    /// The GPU soft path must emit the *same calibrated* LLRs as the CPU path — the kernel produces
    /// σ²=1 distance differences, so both must apply the identical preamble-residual `1/σ²` scaling.
    /// Runs only where a wgpu adapter exists; skips otherwise (like the other GPU equivalence tests).
    #[cfg(feature = "gpu")]
    #[test]
    #[ignore = "requires a GPU adapter"]
    fn gpu_soft_llrs_match_calibrated_cpu() {
        use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
        let Some(ctx) = openpulse_gpu::GpuContext::init() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let cfg = ModulationConfig {
            mode: "64QAM500".into(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            ..Default::default()
        };
        let payload: Vec<u8> = (0..120u32)
            .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
            .collect();
        let tx = crate::Qam64Plugin::new()
            .modulate(&payload, &cfg)
            .expect("modulate");
        // A little deterministic noise so σ² is nonzero and the scaling is actually exercised.
        let rx: Vec<f32> = tx
            .iter()
            .enumerate()
            .map(|(i, &s)| s + 0.03 * ((i as f32 * 0.61).sin()))
            .collect();
        let cpu = qam64_demodulate_soft(&rx, &cfg).expect("cpu soft");
        let gpu = qam64_demodulate_soft_gpu(&rx, &cfg, &ctx).expect("gpu soft");
        assert_eq!(cpu.len(), gpu.len(), "LLR count mismatch");
        for (c, g) in cpu.iter().zip(gpu.iter()) {
            assert!(
                (c - g).abs() <= 1e-2 * (1.0 + c.abs()),
                "GPU LLR {g} not calibrated to CPU LLR {c}"
            );
        }
    }
}
