use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::ModulationConfig;

use crate::parse_baud_rate;

pub const PREAMBLE_SYMS: usize = 16;
pub const TAIL_SYMS: usize = 8;

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
    let n = samples_per_symbol(fs, baud)?;

    let mut symbols = preamble_symbols();
    symbols.extend(bytes_to_symbols(data));
    symbols.extend(std::iter::repeat_n(
        gray_map_8psk(false, false, false),
        TAIL_SYMS,
    ));

    let total = symbols.len() * n;
    let mut out = vec![0.0f32; total];
    let two_pi = 2.0 * PI;

    for (sym_idx, &(i_amp, q_amp)) in symbols.iter().enumerate() {
        let sym_start = sym_idx * n;
        for i in 0..n {
            let envelope = 0.5 * (1.0 - (two_pi * i as f32 / n as f32).cos());
            let t = (sym_start + i) as f32 / fs;
            let c = (two_pi * fc * t).cos();
            let s = (two_pi * fc * t).sin();
            out[sym_start + i] = envelope * (i_amp * c - q_amp * s);
        }
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
        gray_map_8psk(false, false, false),
        gray_map_8psk(false, false, true),
        gray_map_8psk(false, true, true),
        gray_map_8psk(false, true, false),
        gray_map_8psk(true, true, false),
        gray_map_8psk(true, true, true),
        gray_map_8psk(true, false, true),
        gray_map_8psk(true, false, false),
    ];
    (0..PREAMBLE_SYMS)
        .map(|i| pattern[i % pattern.len()])
        .collect()
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
