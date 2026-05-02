use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::ModulationConfig;

use crate::modulate::tones;
use crate::BAUD;

/// Demodulate 4FSK `samples` and return the decoded bytes.
///
/// Groups samples into symbol periods of `fs / BAUD` samples each, then uses
/// the Goertzel algorithm to measure energy at each of the 4 tones and selects
/// the strongest.  Symbols are packed MSB-first: every 4 symbols → 1 byte.
///
/// Returns an error if the input is too short for a single symbol.
pub fn fsk4_demodulate(samples: &[f32], config: &ModulationConfig) -> Result<Vec<u8>, ModemError> {
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let tones = tones(fc);
    let n = (fs / BAUD).round() as usize;

    if samples.len() < n {
        return Err(ModemError::Demodulation("signal too short for 4FSK".into()));
    }

    let n_syms = samples.len() / n;
    let mut sym_vals = Vec::with_capacity(n_syms);

    for sym_idx in 0..n_syms {
        let sym = &samples[sym_idx * n..(sym_idx + 1) * n];

        let mut best_tone = 0usize;
        let mut best_energy = f32::NEG_INFINITY;

        for (k, &f) in tones.iter().enumerate() {
            let energy = goertzel(sym, f, fs);
            if energy > best_energy {
                best_energy = energy;
                best_tone = k;
            }
        }
        sym_vals.push(best_tone as u8);
    }

    // Pack 4 symbols (2 bits each, MSB-first) into each byte.
    let n_bytes = n_syms / 4;
    let mut out = Vec::with_capacity(n_bytes);
    for i in 0..n_bytes {
        let byte = (sym_vals[i * 4] << 6)
            | (sym_vals[i * 4 + 1] << 4)
            | (sym_vals[i * 4 + 2] << 2)
            | sym_vals[i * 4 + 3];
        out.push(byte);
    }

    Ok(out)
}

/// Goertzel algorithm: energy at frequency `f` Hz in `samples`.
fn goertzel(samples: &[f32], f: f32, fs: f32) -> f32 {
    let coeff = 2.0 * (2.0 * PI * f / fs).cos();
    let mut q1 = 0.0f32;
    let mut q2 = 0.0f32;

    for &x in samples {
        let q0 = coeff * q1 - q2 + x;
        q2 = q1;
        q1 = q0;
    }

    q1 * q1 + q2 * q2 - q1 * q2 * coeff
}
