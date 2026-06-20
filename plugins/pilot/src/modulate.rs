//! Passband modulation for the pilot-framed waveform.
//!
//! Maps payload bytes to the [`PilotFrame`] symbol stream, then upconverts each
//! complex symbol to a real audio passband at `center_frequency`. Two pulse
//! shapes: the default **rectangular** pulse (constant amplitude held over the
//! symbol period) and the **`-RRC`** variants ([`upconvert_rrc`]), which
//! root-raised-cosine shape the baseband for ~half the occupied bandwidth.

use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::ModulationConfig;
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::rrc::generate_rrc_coefficients;

use crate::frame::PilotFrame;

/// TX amplitude (headroom below ±1.0).
const TX_AMPLITUDE: f32 = 0.5;

/// Root-raised-cosine roll-off for the `-RRC` pilot variants.
pub(crate) const RRC_ALPHA: f32 = 0.35;
/// RRC filter span in symbols (matches the single-carrier RRC modes).
pub(crate) const RRC_SPAN_SYMBOLS: usize = 8;

/// Whether `mode` is an RRC-pulse variant (`"…-RRC"`).
pub fn is_rrc_mode(mode: &str) -> bool {
    mode.to_ascii_uppercase().ends_with("-RRC")
}

/// The rectangular base mode of a (possibly `-RRC`) mode string — both pulse
/// shapes share baud, constellation, and frame structure.
pub fn base_mode(mode: &str) -> String {
    let upper = mode.to_ascii_uppercase();
    upper
        .strip_suffix("-RRC")
        .map(|s| s.to_string())
        .unwrap_or(upper)
}

/// Baud rate for a pilot-framed mode string, e.g. `"PILOT-QPSK500"` → 500.
pub fn baud_for_mode(mode: &str) -> Result<f32, ModemError> {
    match base_mode(mode).as_str() {
        "PILOT-QPSK500" | "PILOT-8PSK500" | "PILOT-16QAM500" | "PILOT-32APSK500" => Ok(500.0),
        other => Err(ModemError::Configuration(format!(
            "pilot plugin: unsupported mode {other}"
        ))),
    }
}

/// Data-constellation order (bits/symbol) for a mode: QPSK=2, 8PSK=3, 16QAM=4.
pub fn bits_per_sc_for_mode(mode: &str) -> Result<usize, ModemError> {
    match base_mode(mode).as_str() {
        "PILOT-QPSK500" => Ok(2),
        "PILOT-8PSK500" => Ok(3),
        "PILOT-16QAM500" => Ok(4),
        other => Err(ModemError::Configuration(format!(
            "pilot plugin: unsupported mode {other}"
        ))),
    }
}

/// Build the symbol-level codec for a mode (32APSK or Gray QPSK/8PSK/16QAM).
pub fn pilot_frame_for_mode(mode: &str) -> Result<PilotFrame, ModemError> {
    match base_mode(mode).as_str() {
        "PILOT-32APSK500" => Ok(PilotFrame::with_apsk32()),
        other => Ok(PilotFrame::with_bits(bits_per_sc_for_mode(other)?)),
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

/// Upconvert complex symbols with a root-raised-cosine pulse: place each symbol
/// as a baseband impulse, RRC-filter the I and Q streams, then upconvert. Halves
/// the occupied bandwidth versus the rectangular pulse's sinc spectrum.
pub fn upconvert_rrc(symbols: &[(f32, f32)], fc: f32, fs: f32, sps: usize, baud: f32) -> Vec<f32> {
    let total = symbols.len() * sps;
    let mut bb_i = vec![0.0f32; total];
    let mut bb_q = vec![0.0f32; total];
    for (k, &(re, im)) in symbols.iter().enumerate() {
        bb_i[k * sps] = re;
        bb_q[k * sps] = im;
    }
    let num_taps = RRC_SPAN_SYMBOLS * sps + 1;
    let coeffs = generate_rrc_coefficients(fs, baud, RRC_ALPHA, num_taps);
    let group_delay = (num_taps - 1) / 2;
    let filter_bb = |bb: Vec<f32>| -> Vec<f32> {
        let padded: Vec<f32> = bb
            .into_iter()
            .chain(std::iter::repeat_n(0.0, group_delay))
            .collect();
        let filtered = FirFilter::new(coeffs.clone()).apply(&padded);
        filtered[group_delay..].to_vec()
    };
    let i_filt = filter_bb(bb_i);
    let q_filt = filter_bb(bb_q);

    let two_pi = 2.0 * PI;
    i_filt
        .iter()
        .zip(q_filt.iter())
        .enumerate()
        .map(|(n, (&bi, &bq))| {
            let t = n as f32 / fs;
            TX_AMPLITUDE * (bi * (two_pi * fc * t).cos() - bq * (two_pi * fc * t).sin())
        })
        .collect()
}

/// Upconvert symbols to passband, dispatching on the mode's pulse shape.
fn symbols_to_passband(
    symbols: &[(f32, f32)],
    config: &ModulationConfig,
) -> Result<Vec<f32>, ModemError> {
    let sps = samples_per_symbol(config)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    if is_rrc_mode(&config.mode) {
        Ok(upconvert_rrc(
            symbols,
            fc,
            fs,
            sps,
            baud_for_mode(&config.mode)?,
        ))
    } else {
        Ok(upconvert(symbols, fc, fs, sps))
    }
}

/// Modulate `data` into pilot-framed passband audio.
pub fn pilot_modulate(data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
    let symbols = pilot_frame_for_mode(&config.mode)?.encode(data);
    symbols_to_passband(&symbols, config)
}

/// Build the passband onset-correlation template (the upconverted preamble).
pub fn preamble_template(config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
    symbols_to_passband(PilotFrame::new().preamble(), config)
}
