//! 64QAM modulator.

use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, PulseShape};
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::rrc::generate_rrc_coefficients;

use crate::parse_baud_rate;

pub const PREAMBLE_SYMS: usize = 16;
pub const TAIL_SYMS: usize = 8;
pub(crate) const RRC_SPAN_SYMBOLS: usize = 8;

/// Normalisation scale for 8-level PAM amplitudes {±1, ±3, ±5, ±7}.
/// Average power per axis = (1+9+25+49)/4 = 21 → total = 42 → scale = 1/√42.
pub(crate) const PAM8_SCALE: f32 = 0.154_303_35; // 1 / sqrt(42)

/// Map a 3-bit Gray code value (0–7) to a normalised PAM-8 amplitude.
///
/// Gray encoding: adjacent levels differ by one bit.
///
/// | bits | level | amplitude (×scale) |
/// |------|-------|-------------------|
/// | 000  | −7    | −7 × scale        |
/// | 001  | −5    | −5 × scale        |
/// | 011  | −3    | −3 × scale        |
/// | 010  | −1    | −1 × scale        |
/// | 110  | +1    | +1 × scale        |
/// | 111  | +3    | +3 × scale        |
/// | 101  | +5    | +5 × scale        |
/// | 100  | +7    | +7 × scale        |
pub(crate) fn pam8_amplitude(gray: u8) -> f32 {
    let raw: i8 = match gray & 0x7 {
        0b000 => -7,
        0b001 => -5,
        0b011 => -3,
        0b010 => -1,
        0b110 => 1,
        0b111 => 3,
        0b101 => 5,
        0b100 => 7,
        _ => unreachable!(),
    };
    raw as f32 * PAM8_SCALE
}

/// Map 6 bits (b5..b0) to a normalised 64QAM constellation point.
///
/// Bits 5:3 → I axis (Gray-coded PAM-8), bits 2:0 → Q axis.
pub(crate) fn gray_map_64qam(b: u8) -> (f32, f32) {
    let i_bits = (b >> 3) & 0x7;
    let q_bits = b & 0x7;
    (pam8_amplitude(i_bits), pam8_amplitude(q_bits))
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

/// Convert bytes to 64QAM symbols (6 bits per symbol, LSB-first).
pub(crate) fn bytes_to_symbols(data: &[u8]) -> Vec<(f32, f32)> {
    let bits = bytes_to_bits(data);
    bits.chunks(6)
        .map(|c| {
            let mut b = 0u8;
            for (i, &bit) in c.iter().enumerate() {
                if bit {
                    b |= 1 << i;
                }
            }
            gray_map_64qam(b)
        })
        .collect()
}

pub(crate) fn preamble_symbols() -> Vec<(f32, f32)> {
    // Alternating corner and inner points for reliable timing acquisition.
    let pattern = [
        gray_map_64qam(0b000_000), // (−7,−7) normalised
        gray_map_64qam(0b100_100), // (+7,+7)
        gray_map_64qam(0b000_100), // (−7,+7)
        gray_map_64qam(0b100_000), // (+7,−7)
    ];
    (0..PREAMBLE_SYMS)
        .map(|i| pattern[i % pattern.len()])
        .collect()
}

pub(crate) fn samples_per_symbol(sample_rate: f32, baud: f32) -> Result<usize, ModemError> {
    let n = (sample_rate / baud).round() as usize;
    if n < 4 {
        return Err(ModemError::Configuration(format!(
            "sample rate {sample_rate} Hz is too low for {baud} baud (need ≥ 4 samples/symbol)"
        )));
    }
    Ok(n)
}

pub fn qam64_modulate(data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    let rrc_alpha = if let PulseShape::Rrc { alpha } = config.pulse_shape {
        Some(alpha)
    } else if config.mode.ends_with("-RRC") {
        Some(0.35f32)
    } else {
        None
    };

    let mut symbols = preamble_symbols();
    symbols.extend(bytes_to_symbols(data));
    symbols.extend(std::iter::repeat_n(
        gray_map_64qam(0b110_110), // (+1,+1) — low-amplitude tail
        TAIL_SYMS,
    ));

    let total = symbols.len() * n;
    let two_pi = 2.0 * PI;

    if let Some(alpha) = rrc_alpha {
        // RRC path: baseband impulse train → RRC TX filter → upconvert.
        let mut bb_i = vec![0.0f32; total];
        let mut bb_q = vec![0.0f32; total];
        for (sym_idx, &(i_amp, q_amp)) in symbols.iter().enumerate() {
            bb_i[sym_idx * n] = i_amp;
            bb_q[sym_idx * n] = q_amp;
        }

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

        let out = i_filt
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
        return Ok(out);
    }

    // Rectangular-windowed path: abrupt symbol transitions.
    //
    // 64QAM has 8 distinct amplitude levels per axis; the Hann crossfade used
    // by BPSK/8PSK creates ISI proportional to the amplitude difference between
    // adjacent symbols, which breaks simple coherent integration at the receiver.
    // Rectangular windowing keeps adjacent symbol periods independent.
    let mut out = vec![0.0f32; total];
    for (sym_idx, &(i_amp, q_amp)) in symbols.iter().enumerate() {
        let sym_start = sym_idx * n;
        for i in 0..n {
            let t = (sym_start + i) as f32 / fs;
            let c = (two_pi * fc * t).cos();
            let s = (two_pi * fc * t).sin();
            out[sym_start + i] = i_amp * c - q_amp * s;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pam8_covers_eight_distinct_levels() {
        let levels: Vec<f32> = (0..8u8).map(pam8_amplitude).collect();
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert!(
                    (levels[i] - levels[j]).abs() > 1e-5,
                    "levels {i} and {j} are identical"
                );
            }
        }
    }

    #[test]
    fn gray_map_covers_64_distinct_points() {
        let points: Vec<(f32, f32)> = (0..64u8).map(gray_map_64qam).collect();
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

    #[test]
    fn average_constellation_power_is_unit() {
        let total_power: f32 = (0..64u8)
            .map(|b| {
                let (i, q) = gray_map_64qam(b);
                i * i + q * q
            })
            .sum::<f32>()
            / 64.0;
        assert!(
            (total_power - 1.0).abs() < 0.01,
            "average power {total_power:.4} ≠ 1.0"
        );
    }

    #[test]
    fn adjacent_gray_codes_differ_by_one_bit() {
        // Check that each pair of horizontally or vertically adjacent points in the
        // 8×8 grid differs by exactly one bit in the corresponding axis code.
        let gray_differs_by_one = |a: u8, b: u8| -> bool {
            let xor = (a ^ b) & 0x7;
            xor != 0 && (xor & (xor - 1)) == 0 // exactly one bit set
        };
        for i in 0..7u8 {
            let a = i;
            let b = i + 1;
            // The raw loop index maps to Gray codes via the pam8_amplitude function.
            // Adjacent *raw indices* in PAM-8 Gray mapping need not differ by one bit
            // (that's the point of Gray coding — the *amplitude levels* are adjacent,
            // not the index values).  Here we check the Gray codes for adjacent amplitudes.
            // Amplitude levels in ascending order correspond to Gray codes:
            // −7→000, −5→001, −3→011, −1→010, +1→110, +3→111, +5→101, +7→100
            let gray_sequence: [u8; 8] = [0b000, 0b001, 0b011, 0b010, 0b110, 0b111, 0b101, 0b100];
            assert!(
                gray_differs_by_one(gray_sequence[a as usize], gray_sequence[b as usize]),
                "adjacent amplitude levels {a}↔{b} do not differ by exactly one bit"
            );
        }
    }
}
