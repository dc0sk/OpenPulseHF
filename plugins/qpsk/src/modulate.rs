use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, PulseShape};
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::rrc::generate_rrc_coefficients;

use crate::parse_baud_rate;

pub const PREAMBLE_SYMS: usize = 16;
pub const TAIL_SYMS: usize = 8;

const INV_SQRT_2: f32 = 0.70710677;

/// Number of symbol spans for the RRC FIR filter (controls stop-band rejection).
pub(crate) const RRC_SPAN_SYMBOLS: usize = 8;

pub fn qpsk_modulate(data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    let cosine_overlap =
        config.pulse_shape == PulseShape::CosineOverlap || config.mode.ends_with("-HF");
    let rrc_alpha = if let PulseShape::Rrc { alpha } = config.pulse_shape {
        Some(alpha)
    } else if config.mode.ends_with("-RRC") {
        Some(0.35)
    } else {
        None
    };

    let mut symbols = preamble_symbols();
    symbols.extend(bits_to_symbols(&bytes_to_bits(data)));
    symbols.extend(std::iter::repeat_n((INV_SQRT_2, INV_SQRT_2), TAIL_SYMS));

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

/// Encode `data` bytes as QPSK baseband I and Q sample vectors.
///
/// Returns `(i_bb, q_bb)` without carrier upconversion; suitable for direct
/// SDR I/Q streaming or stereo audio output.
pub fn qpsk_modulate_iq(
    data: &[u8],
    config: &ModulationConfig,
) -> Result<(Vec<f32>, Vec<f32>), ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let n = samples_per_symbol(fs, baud)?;

    let cosine_overlap =
        config.pulse_shape == PulseShape::CosineOverlap || config.mode.ends_with("-HF");
    let rrc_alpha = if let PulseShape::Rrc { alpha } = config.pulse_shape {
        Some(alpha)
    } else if config.mode.ends_with("-RRC") {
        Some(0.35)
    } else {
        None
    };

    let mut symbols = preamble_symbols();
    symbols.extend(bits_to_symbols(&bytes_to_bits(data)));
    symbols.extend(std::iter::repeat_n((INV_SQRT_2, INV_SQRT_2), TAIL_SYMS));

    let total = symbols.len() * n;
    let mut bb_i = vec![0.0f32; total];
    let mut bb_q = vec![0.0f32; total];

    for (sym_idx, &(i_amp, q_amp)) in symbols.iter().enumerate() {
        let sym_start = sym_idx * n;
        if rrc_alpha.is_some() {
            // Impulse at symbol start; RRC filter provides pulse shaping below.
            bb_i[sym_start] = i_amp;
            bb_q[sym_start] = q_amp;
        } else if cosine_overlap {
            for i in 0..n {
                let amp = 0.5 * (1.0 - (2.0 * PI * i as f32 / n as f32).cos());
                bb_i[sym_start + i] = i_amp * amp;
                bb_q[sym_start + i] = q_amp * amp;
            }
        } else {
            let (i_next, q_next) = symbols.get(sym_idx + 1).copied().unwrap_or((0.0, 0.0));
            for i in 0..n {
                let w_tail = 0.5 * (1.0 + (PI * i as f32 / n as f32).cos());
                let w_head = 1.0 - w_tail;
                bb_i[sym_start + i] = i_amp * w_tail + i_next * w_head;
                bb_q[sym_start + i] = q_amp * w_tail + q_next * w_head;
            }
        }
    }

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

        bb_i = filter_bb(bb_i);
        bb_q = filter_bb(bb_q);
    }

    Ok((bb_i, bb_q))
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

pub(crate) fn bits_to_symbols(bits: &[bool]) -> Vec<(f32, f32)> {
    let mut syms = Vec::with_capacity(bits.len().div_ceil(2));
    for pair in bits.chunks(2) {
        let b0 = pair.first().copied().unwrap_or(false);
        let b1 = pair.get(1).copied().unwrap_or(false);
        syms.push(gray_map(b0, b1));
    }
    syms
}

pub(crate) fn gray_map(b0: bool, b1: bool) -> (f32, f32) {
    // Gray mapping: 00->45deg, 01->135deg, 11->225deg, 10->315deg
    match (b0, b1) {
        (false, false) => (INV_SQRT_2, INV_SQRT_2),
        (false, true) => (-INV_SQRT_2, INV_SQRT_2),
        (true, true) => (-INV_SQRT_2, -INV_SQRT_2),
        (true, false) => (INV_SQRT_2, -INV_SQRT_2),
    }
}

pub(crate) fn samples_per_symbol(sample_rate: f32, baud: f32) -> Result<usize, ModemError> {
    let n = (sample_rate / baud).round() as usize;
    if n < 4 {
        return Err(ModemError::Configuration(format!(
            "sample rate {sample_rate} Hz is too low for {baud} baud (need at least 4 samples/symbol)"
        )));
    }
    Ok(n)
}

pub(crate) fn preamble_symbols() -> Vec<(f32, f32)> {
    let pattern = [
        gray_map(false, false),
        gray_map(false, true),
        gray_map(true, true),
        gray_map(true, false),
    ];
    (0..PREAMBLE_SYMS)
        .map(|i| pattern[i % pattern.len()])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray_map_constellation_points() {
        assert_eq!(gray_map(false, false), (INV_SQRT_2, INV_SQRT_2));
        assert_eq!(gray_map(false, true), (-INV_SQRT_2, INV_SQRT_2));
        assert_eq!(gray_map(true, true), (-INV_SQRT_2, -INV_SQRT_2));
        assert_eq!(gray_map(true, false), (INV_SQRT_2, -INV_SQRT_2));
    }
}
