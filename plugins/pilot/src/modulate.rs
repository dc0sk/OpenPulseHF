//! Passband modulation for the pilot-framed waveform.
//!
//! Maps payload bytes to the [`PilotFrame`] symbol stream, then upconverts each
//! complex symbol to a real audio passband at `center_frequency` with a
//! rectangular pulse (constant amplitude held over the symbol period). The POC
//! mode is rectangular; an RRC variant follows once the rectangular chain is
//! validated end-to-end (the established bring-up order).

use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::ModulationConfig;

use crate::frame::PilotFrame;

/// TX amplitude (headroom below ±1.0).
const TX_AMPLITUDE: f32 = 0.5;

/// Baud rate for a pilot-framed mode string, e.g. `"PILOT-QPSK500"` → 500.
pub fn baud_for_mode(mode: &str) -> Result<f32, ModemError> {
    match mode.to_ascii_uppercase().as_str() {
        "PILOT-QPSK500" | "PILOT-8PSK500" | "PILOT-16QAM500" => Ok(500.0),
        other => Err(ModemError::Configuration(format!(
            "pilot plugin: unsupported mode {other}"
        ))),
    }
}

/// Data-constellation order (bits/symbol) for a mode: QPSK=2, 8PSK=3, 16QAM=4.
pub fn bits_per_sc_for_mode(mode: &str) -> Result<usize, ModemError> {
    match mode.to_ascii_uppercase().as_str() {
        "PILOT-QPSK500" => Ok(2),
        "PILOT-8PSK500" => Ok(3),
        "PILOT-16QAM500" => Ok(4),
        other => Err(ModemError::Configuration(format!(
            "pilot plugin: unsupported mode {other}"
        ))),
    }
}

/// Integer samples per symbol for `mode` at `config.sample_rate`.
pub fn samples_per_symbol(config: &ModulationConfig) -> Result<usize, ModemError> {
    let baud = baud_for_mode(&config.mode)?;
    let sps = (config.sample_rate as f32 / baud).round() as usize;
    if sps == 0 {
        return Err(ModemError::Configuration(
            "pilot plugin: sample rate < baud".into(),
        ));
    }
    Ok(sps)
}

/// Upconvert complex symbols to a real passband signal (rectangular pulse).
///
/// `s(n) = amp · (re·cos(2π·fc·n/fs) − im·sin(2π·fc·n/fs))`, each symbol held
/// for `sps` samples. The absolute sample index sets the carrier phase; the
/// receiver's pilot tracker removes any resulting static phase.
pub fn upconvert(symbols: &[(f32, f32)], fc: f32, fs: f32, sps: usize) -> Vec<f32> {
    let two_pi = 2.0 * PI;
    let mut out = Vec::with_capacity(symbols.len() * sps);
    let mut n = 0usize;
    for &(re, im) in symbols {
        for _ in 0..sps {
            let t = n as f32 / fs;
            let c = (two_pi * fc * t).cos();
            let s = (two_pi * fc * t).sin();
            out.push(TX_AMPLITUDE * (re * c - im * s));
            n += 1;
        }
    }
    out
}

/// Modulate `data` into pilot-framed QPSK passband audio.
pub fn pilot_modulate(data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
    let sps = samples_per_symbol(config)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let bits = bits_per_sc_for_mode(&config.mode)?;
    let symbols = PilotFrame::with_bits(bits).encode(data);
    Ok(upconvert(&symbols, fc, fs, sps))
}

/// Build the passband onset-correlation template (the upconverted preamble).
pub fn preamble_template(config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
    let sps = samples_per_symbol(config)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    Ok(upconvert(PilotFrame::new().preamble(), fc, fs, sps))
}
