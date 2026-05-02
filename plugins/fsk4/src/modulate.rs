use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::ModulationConfig;

use crate::BAUD;

/// Spacing between adjacent 4FSK tones (Hz).
const TONE_SPACING: f32 = 100.0;

/// Return the 4 tone frequencies centred on `fc`.
pub(crate) fn tones(fc: f32) -> [f32; 4] {
    [
        fc - 1.5 * TONE_SPACING, // tone 0 — lowest
        fc - 0.5 * TONE_SPACING, // tone 1
        fc + 0.5 * TONE_SPACING, // tone 2
        fc + 1.5 * TONE_SPACING, // tone 3 — highest
    ]
}

/// Modulate `data` bytes using 4FSK.
///
/// Bit packing: MSB-first within each byte.  Each byte yields 4 symbols
/// (2 bits each).  Symbols are Hann-windowed to reduce spectral splatter.
///
/// Output length: `data.len() × 4 × samples_per_symbol`.
pub fn fsk4_modulate(data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let tones = tones(fc);
    let n = (fs / BAUD).round() as usize; // samples per symbol

    let mut out = Vec::with_capacity(data.len() * 4 * n);

    for &byte in data {
        for sym_idx in 0..4usize {
            let bits = (byte >> (6 - sym_idx * 2)) & 0x03;
            let tone_freq = tones[bits as usize];
            let sym_start = out.len();

            for i in 0..n {
                let t = (sym_start + i) as f32 / fs;
                let window = 0.5 * (1.0 - (2.0 * PI * i as f32 / n as f32).cos());
                out.push((2.0 * PI * tone_freq * t).sin() * window);
            }
        }
    }

    Ok(out)
}
