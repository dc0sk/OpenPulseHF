//! 64QAM demodulator.
//!
//! Pipeline: downmix → Hann integration or RRC matched-filter + Gardner → nearest-point
//! decision → bit extraction.

use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, PulseShape};
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::rrc::generate_rrc_coefficients;
use openpulse_dsp::timing::GardnerDetector;

use crate::modulate::{
    gray_map_64qam, preamble_symbols, samples_per_symbol, PAM8_SCALE, PREAMBLE_SYMS,
    RRC_SPAN_SYMBOLS, TAIL_SYMS,
};
use crate::parse_baud_rate;

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

// ── IQ demodulation (Hann integration path) ──────────────────────────────────

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
        let (i_v, q_v) = demodulate_iq(samples, n, fc, fs, off);
        if i_v.len() < PREAMBLE_SYMS || q_v.len() < PREAMBLE_SYMS {
            continue;
        }
        let score: f32 = (0..PREAMBLE_SYMS)
            .map(|s| {
                let (ei, eq) = expected_syms[s];
                i_v[s] * ei + q_v[s] * eq
            })
            .sum();
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
    (i_out, q_out)
}

fn find_timing_offset_bb(i_bb: &[f32], q_bb: &[f32], n: usize) -> usize {
    let expected = preamble_symbols();
    let mut best_off = 0usize;
    let mut best_score = f32::NEG_INFINITY;
    for off in 0..n {
        if i_bb.len() < off + n * PREAMBLE_SYMS {
            break;
        }
        let score: f32 = (0..PREAMBLE_SYMS)
            .map(|s| {
                let (ei, eq) = expected[s];
                i_bb[off + s * n] * ei + q_bb[off + s * n] * eq
            })
            .sum();
        if score > best_score {
            best_score = score;
            best_off = off;
        }
    }
    best_off
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
