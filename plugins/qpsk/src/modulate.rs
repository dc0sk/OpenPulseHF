use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::ModulationConfig;

use crate::parse_baud_rate;

pub const PREAMBLE_SYMS: usize = 16;
pub const TAIL_SYMS: usize = 8;

const INV_SQRT_2: f32 = 0.70710677;

pub fn qpsk_modulate(data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    let mut symbols = preamble_symbols();
    symbols.extend(bits_to_symbols(&bytes_to_bits(data)));
    symbols.extend(std::iter::repeat_n((INV_SQRT_2, INV_SQRT_2), TAIL_SYMS));

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
