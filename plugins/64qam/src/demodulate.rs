//! 64QAM demodulator.
//!
//! Pipeline: downmix → rectangular integration or RRC matched-filter + Gardner
//! → nearest-point decision → bit extraction.

use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, PulseShape};
use openpulse_dsp::acquisition::{estimate_cfo_data_aided, preamble_corr_sq};
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::rrc::generate_rrc_coefficients;
use openpulse_dsp::timing::GardnerDetector;

use crate::modulate::{
    gray_map_64qam, preamble_symbols, samples_per_symbol, PAM8_SCALE, PREAMBLE_SYMS,
    RRC_SPAN_SYMBOLS, TAIL_SYMS,
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

/// Decision-directed carrier tracking loop for 64QAM.
///
/// 64QAM is not constant-modulus, so an M-PSK Costas loop cannot be used.  This
/// second-order loop instead derives its phase error from the decision: for each
/// symbol it de-rotates by the running phase estimate, decides the nearest
/// constellation point, and uses Im(r·conj(decision)) as the error.  It is seeded
/// at zero because `carrier_phase_correct` has already removed the static offset, so
/// the loop only has to track the residual frequency drift (e.g. the difference in
/// USB-audio crystal frequencies between two stations), which static correction
/// cannot follow across a frame.
fn dd_carrier_track(i_syms: &[f32], q_syms: &[f32], loop_bw: f32) -> (Vec<f32>, Vec<f32>) {
    let alpha = loop_bw;
    let beta = loop_bw * loop_bw * 0.25;
    let mut phase = 0.0f32;
    let mut freq = 0.0f32;
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

    let start = initial_timing.min(i_bb.len());
    let mut det = GardnerDetector::new(n, 0.02);
    det.pre_arm();
    let mut i_out = Vec::new();
    let mut q_out = Vec::new();
    for idx in 0..i_bb[start..].len() {
        let raw_i = i_bb[start + idx];
        let raw_q = q_bb.get(start + idx).copied().unwrap_or(0.0);
        let s_i = raw_i * cos0 - raw_q * sin0;
        if det.update(s_i).is_some() {
            let s_q = raw_i * sin0 + raw_q * cos0;
            i_out.push(s_i);
            q_out.push(s_q);
        }
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
    // Track residual carrier frequency drift across the frame (hardware crystal
    // offset); static phase correction alone cannot follow it on the dense grid.
    let (i_syms, q_syms) = dd_carrier_track(&i_syms, &q_syms, 0.01);

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
    // Track residual carrier frequency drift across the frame (hardware crystal
    // offset); static phase correction alone cannot follow it on the dense grid.
    let (i_syms, q_syms) = dd_carrier_track(&i_syms, &q_syms, 0.01);

    if i_syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".into(),
        ));
    }

    let data_start = PREAMBLE_SYMS;
    let data_end = i_syms.len() - TAIL_SYMS;
    let mut llrs = Vec::with_capacity((data_end - data_start) * 6);

    // Precompute all 64 constellation points.
    let points: Vec<(f32, f32)> = (0..64u8).map(gray_map_64qam).collect();

    for (&yi, &yq) in i_syms[data_start..data_end]
        .iter()
        .zip(q_syms[data_start..data_end].iter())
    {
        for bit_pos in 0..6u8 {
            let mask = 1 << bit_pos;
            let mut min_d0 = f32::MAX;
            let mut min_d1 = f32::MAX;
            for (sym_idx, &(pi, pq)) in points.iter().enumerate() {
                let d = (yi - pi).powi(2) + (yq - pq).powi(2);
                if sym_idx as u8 & mask == 0 {
                    if d < min_d0 {
                        min_d0 = d;
                    }
                } else if d < min_d1 {
                    min_d1 = d;
                }
            }
            // Positive LLR → bit is more likely 0.
            llrs.push(min_d1 - min_d0);
        }
    }
    Ok(llrs)
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
    // Track residual carrier frequency drift across the frame (hardware crystal
    // offset); static phase correction alone cannot follow it on the dense grid.
    let (i_syms, q_syms) = dd_carrier_track(&i_syms, &q_syms, 0.01);

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

    let constellation: Vec<(f32, f32)> = (0..64u8).map(gray_map_64qam).collect();
    let bit_table: Vec<u32> = (0..64u32).collect();

    if let Some(llrs) = openpulse_gpu::gpu_soft_demod(ctx, &syms, &constellation, &bit_table, 6) {
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
            .chain(std::iter::repeat(0.0).take(group_delay))
            .collect();
        let filtered = openpulse_gpu::gpu_rrc_fir(ctx, &padded, &coeffs)?;
        Some(filtered[group_delay..].to_vec())
    };

    let i_bb = gpu_rrc(i_mix)?;
    let q_bb = gpu_rrc(q_mix)?;

    let initial_timing = find_timing_offset_bb(&i_bb, &q_bb, n);
    let start = initial_timing.min(i_bb.len());
    let mut det = GardnerDetector::new(n, 0.02);
    det.pre_arm();
    let mut i_out = Vec::new();
    let mut q_out = Vec::new();
    for (idx, &s_i) in i_bb[start..].iter().enumerate() {
        if det.update(s_i).is_some() {
            let s_q = q_bb.get(start + idx).copied().unwrap_or(0.0);
            i_out.push(s_i);
            q_out.push(s_q);
        }
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
    use crate::modulate::pam8_amplitude;

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
}
