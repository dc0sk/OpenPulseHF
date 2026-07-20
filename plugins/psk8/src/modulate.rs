use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, PulseShape};
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::rrc::generate_rrc_coefficients;

use crate::parse_baud_rate;

pub const PREAMBLE_SYMS: usize = 16;
pub const TAIL_SYMS: usize = 8;
/// RRC FIR filter span in symbols. 12 (not 8) drops the residual-ISI floor ~-36 to ~-50 dB — matters
/// for the dense RRC rungs. Both ends use this constant, so mod and demod stay matched.
pub(crate) const RRC_SPAN_SYMBOLS: usize = 12;

const INV_SQRT_2: f32 = 0.70710677;

/// Gray-coded 8PSK constellation: 8 phases at 45° increments.
///
/// Gray coding: adjacent points differ by exactly one bit.
/// 000→0°, 001→45°, 011→90°, 010→135°, 110→180°, 111→225°, 101→270°, 100→315°
pub(crate) fn gray_map_8psk(b0: bool, b1: bool, b2: bool) -> (f32, f32) {
    match (b0, b1, b2) {
        (false, false, false) => (1.0, 0.0),
        (false, false, true) => (INV_SQRT_2, INV_SQRT_2),
        (false, true, true) => (0.0, 1.0),
        (false, true, false) => (-INV_SQRT_2, INV_SQRT_2),
        (true, true, false) => (-1.0, 0.0),
        (true, true, true) => (-INV_SQRT_2, -INV_SQRT_2),
        (true, false, true) => (0.0, -1.0),
        (true, false, false) => (INV_SQRT_2, -INV_SQRT_2),
    }
}

pub fn psk8_modulate(data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let cosine_overlap =
        config.pulse_shape == PulseShape::CosineOverlap || config.mode.ends_with("-HF");
    let rrc_alpha = if let PulseShape::Rrc { alpha } = config.pulse_shape {
        Some(alpha)
    } else if config.mode.ends_with("-RRC") {
        Some(0.35f32)
    } else {
        None
    };
    let shaped = rrc_alpha.is_some() || cosine_overlap;
    let n = samples_per_symbol_for_pulse(fs, baud, shaped, &config.mode)?;

    let mut symbols = preamble_symbols();
    symbols.extend(bytes_to_symbols(data));
    symbols.extend(std::iter::repeat_n(
        gray_map_8psk(false, false, false),
        TAIL_SYMS,
    ));

    let total = symbols.len() * n;
    let mut out = vec![0.0f32; total];
    // For RRC: keep separate baseband I and Q impulse streams.
    let mut bb_i = if rrc_alpha.is_some() {
        vec![0.0f32; total]
    } else {
        vec![]
    };
    let mut bb_q = if rrc_alpha.is_some() {
        vec![0.0f32; total]
    } else {
        vec![]
    };
    let two_pi = 2.0 * PI;

    for (sym_idx, &(i_amp, q_amp)) in symbols.iter().enumerate() {
        let sym_start = sym_idx * n;
        if rrc_alpha.is_some() {
            // RRC path: baseband impulse at symbol start; carrier applied after
            // RRC filtering below.
            bb_i[sym_start] = i_amp;
            bb_q[sym_start] = q_amp;
        } else if cosine_overlap {
            for i in 0..n {
                // sin²(πi/n): 0 at boundaries, peaks at 1 at midpoint.
                let amp = 0.5 * (1.0 - (2.0 * PI * i as f32 / n as f32).cos());
                let t = (sym_start + i) as f32 / fs;
                let c = (two_pi * fc * t).cos();
                let s = (two_pi * fc * t).sin();
                out[sym_start + i] = (i_amp * c - q_amp * s) * amp;
            }
        } else {
            let (i_next, q_next) = symbols.get(sym_idx + 1).copied().unwrap_or((0.0, 0.0));
            for i in 0..n {
                let w_tail = 0.5 * (1.0 + (PI * i as f32 / n as f32).cos());
                let w_head = 1.0 - w_tail;
                let t = (sym_start + i) as f32 / fs;
                let c = (two_pi * fc * t).cos();
                let s = (two_pi * fc * t).sin();
                let env_i = i_amp * w_tail + i_next * w_head;
                let env_q = q_amp * w_tail + q_next * w_head;
                out[sym_start + i] = env_i * c - env_q * s;
            }
        }
    }

    // Apply RRC TX filter if requested (operates on baseband), then upconvert.
    if let Some(alpha) = rrc_alpha {
        let num_taps = RRC_SPAN_SYMBOLS * n + 1;
        let coeffs = generate_rrc_coefficients(fs, baud, alpha, num_taps);
        let group_delay = (num_taps - 1) / 2;

        let filter_bb = |bb: Vec<f32>| -> Vec<f32> {
            let padded: Vec<f32> = bb
                .iter()
                .copied()
                .chain(std::iter::repeat_n(0.0, group_delay))
                .collect();
            let mut fir = FirFilter::new(coeffs.clone());
            let filtered = fir.apply(&padded);
            filtered[group_delay..].to_vec()
        };

        let i_filt = filter_bb(bb_i);
        let q_filt = filter_bb(bb_q);

        // Upconvert shaped baseband I/Q to bandpass.
        out = i_filt
            .iter()
            .zip(q_filt.iter())
            .enumerate()
            .map(|(k, (&bi, &bq))| {
                let t = k as f32 / fs;
                let c = (two_pi * fc * t).cos();
                let s = (two_pi * fc * t).sin();
                bi * c - bq * s
            })
            .collect();
    }

    Ok(out)
}

pub(crate) fn bytes_to_symbols(data: &[u8]) -> Vec<(f32, f32)> {
    let bits = bytes_to_bits(data);
    bits.chunks(3)
        .map(|c| {
            let b0 = c.first().copied().unwrap_or(false);
            let b1 = c.get(1).copied().unwrap_or(false);
            let b2 = c.get(2).copied().unwrap_or(false);
            gray_map_8psk(b0, b1, b2)
        })
        .collect()
}

pub(crate) fn bytes_to_bits(bytes: &[u8]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for shift in 0..8u8 {
            bits.push((b >> shift) & 1 == 1);
        }
    }
    bits
}

/// Absolute floor: below this there is not enough of a symbol to shape or integrate at all.
pub(crate) const MIN_SAMPLES_PER_SYMBOL: usize = 4;

/// Floor for the plain (crossfade) pulse, which needs one more sample per symbol than a matched filter.
///
/// The plain modulator blends adjacent symbols with a raised cosine, and the demodulator integrates
/// against the squared window, leaving a residual ISI term whose size depends on `n` (β ≈ 0.182 at
/// 16 sps, 0.167 at 8 sps — see CLAUDE.md → *Known sharp edges*). At 4 sps that residual exceeds
/// 8PSK's ±22.5° decision margin and the frame does not decode **on a clean, noiseless channel**.
///
/// Measured 2026-07-20, all clean-channel round-trips:
///
/// | sps | mode | plain pulse |
/// |---|---|---|
/// | 4 | `8PSK2000` @ 8 kHz | **fails** |
/// | 5 | `8PSK9600` @ 48 kHz | works |
/// | 8 | `8PSK1000` @ 8 kHz | works |
/// | 16 | `8PSK500` @ 8 kHz | works |
///
/// So the floor is **5**, not 8. The first version of this guard used 8 — generalised from the 4/8/16
/// samples the 8 kHz modes happen to give, straight past the boundary that made it true. The existing
/// `psk8_9600_loopback_48k` test caught it. Check the boundary before generalising a measurement.
///
/// `8PSK2000-RRC` decodes at the same 4 sps, and so does plain `QPSK2000` — it is the phase margin
/// that runs out, not the sample rate: QPSK has ±45° to spend and 8PSK does not.
///
/// Before this guard, `8PSK2000` at 8 kHz emitted audio that nothing could decode and reported a
/// generic framing error at the receiver — a silent capability lie.
pub(crate) const MIN_SAMPLES_PER_SYMBOL_PLAIN: usize = 5;

pub(crate) fn samples_per_symbol(sample_rate: f32, baud: f32) -> Result<usize, ModemError> {
    let n = (sample_rate / baud).round() as usize;
    if n < MIN_SAMPLES_PER_SYMBOL {
        return Err(ModemError::Configuration(format!(
            "sample rate {sample_rate} Hz is too low for {baud} baud (need at least \
             {MIN_SAMPLES_PER_SYMBOL} samples/symbol)"
        )));
    }
    Ok(n)
}

/// Like [`samples_per_symbol`], but enforces the higher floor the plain pulse needs.
///
/// `shaped` is true for RRC and for the `-HF` cosine-overlap pulse, both of which are per-symbol
/// shapes that survive 4 samples/symbol.
pub(crate) fn samples_per_symbol_for_pulse(
    sample_rate: f32,
    baud: f32,
    shaped: bool,
    mode: &str,
) -> Result<usize, ModemError> {
    let n = samples_per_symbol(sample_rate, baud)?;
    if !shaped && n < MIN_SAMPLES_PER_SYMBOL_PLAIN {
        return Err(ModemError::Configuration(format!(
            "{mode} at {sample_rate} Hz gives {n} samples/symbol; the plain 8PSK pulse needs at least \
             {MIN_SAMPLES_PER_SYMBOL_PLAIN} (its inter-symbol residual exceeds 8PSK's 22.5-degree \
             margin below that). Use {mode}-RRC, which is a matched filter and works at this rate."
        )));
    }
    Ok(n)
}

pub(crate) fn preamble_symbols() -> Vec<(f32, f32)> {
    // Designed low-autocorrelation sequence over all 8 phases (each used twice).
    //
    // The previous pattern walked the constellation in constant +45° steps, so the
    // 1-lag autocorrelation R₁ ≈ 16.  With the crossfade modulator this made the
    // squared-complex timing correlation nearly flat across all sample offsets, so
    // the correct offset could not be distinguished from the d=n-1 ISI alias and the
    // decode failed whenever the unknown carrier phase landed near 90°.
    //
    // This order (found by search) has peak aperiodic autocorrelation sidelobe ≈2.2
    // and R₁ ≈1.2 versus the mainlobe 16, giving an unambiguous timing peak for any
    // carrier phase.  All 8 constellation points appear exactly twice for supervised
    // LMS training diversity, and there are no adjacent repeats.  Phase-index order:
    //   [90,0,270,180,90,135,45,180,0,45,135,225,315,225,270,315]°
    let pts = [
        gray_map_8psk(false, false, false), // idx 0 →   0°
        gray_map_8psk(false, false, true),  // idx 1 →  45°
        gray_map_8psk(false, true, true),   // idx 2 →  90°
        gray_map_8psk(false, true, false),  // idx 3 → 135°
        gray_map_8psk(true, true, false),   // idx 4 → 180°
        gray_map_8psk(true, true, true),    // idx 5 → 225°
        gray_map_8psk(true, false, true),   // idx 6 → 270°
        gray_map_8psk(true, false, false),  // idx 7 → 315°
    ];
    const PHASE_IDX: [usize; PREAMBLE_SYMS] = [2, 0, 6, 4, 2, 3, 1, 4, 0, 1, 3, 5, 7, 5, 6, 7];
    PHASE_IDX.iter().map(|&k| pts[k]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray_map_covers_all_eight_points() {
        let points = [
            gray_map_8psk(false, false, false),
            gray_map_8psk(false, false, true),
            gray_map_8psk(false, true, true),
            gray_map_8psk(false, true, false),
            gray_map_8psk(true, true, false),
            gray_map_8psk(true, true, true),
            gray_map_8psk(true, false, true),
            gray_map_8psk(true, false, false),
        ];
        for i in 0..points.len() {
            for j in (i + 1)..points.len() {
                let (ai, aq) = points[i];
                let (bi, bq) = points[j];
                assert!(
                    (ai - bi).abs() > 1e-5 || (aq - bq).abs() > 1e-5,
                    "constellation points {i} and {j} are identical"
                );
            }
        }
    }
}
