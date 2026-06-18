//! Passband demodulation for the pilot-framed waveform.
//!
//! The pilot-framed receiver is deliberately simple: because carrier recovery is
//! done by the pilot-aided [`PilotTracker`] at the symbol level (inside
//! [`PilotFrame::decode`]), the passband front-end has no Costas loop, drift fit,
//! or DFE — the machinery that dominates the preamble-only QPSK demodulator.
//!
//! Chain: locate the frame with a carrier-phase-insensitive correlation against
//! the known passband preamble ([`IqMatchedFilter`]); from that onset,
//! coherently downconvert and integrate-and-dump each symbol period (the matched
//! filter for a rectangular pulse); hand the recovered complex symbols to
//! [`PilotFrame::decode`], which acquires on the preamble and tracks the sparse
//! pilots.

use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::ModulationConfig;
use openpulse_dsp::acquisition::IqMatchedFilter;

use crate::frame::PilotFrame;
use crate::modulate::{preamble_template, samples_per_symbol};

/// Demodulate pilot-framed QPSK passband audio back to bytes.
pub fn pilot_demodulate(samples: &[f32], config: &ModulationConfig) -> Result<Vec<u8>, ModemError> {
    let sps = samples_per_symbol(config)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;

    // 1. Frame onset via phase-insensitive correlation against the passband preamble.
    let template = preamble_template(config)?;
    let mf = IqMatchedFilter::new(template);
    if samples.len() < mf.len() {
        return Ok(Vec::new());
    }
    let bound = samples.len() - mf.len();
    let onset = match mf.search(samples, bound) {
        Some(r) => r.offset,
        None => return Ok(Vec::new()),
    };

    // 2. From the onset, downconvert + integrate-and-dump each symbol period.
    let two_pi = 2.0 * PI;
    let total_syms = (samples.len() - onset) / sps;
    let scale = 2.0 / sps as f32;
    let mut symbols = Vec::with_capacity(total_syms);
    for k in 0..total_syms {
        let start = onset + k * sps;
        let mut acc_i = 0.0f32;
        let mut acc_q = 0.0f32;
        for j in 0..sps {
            let n = start + j;
            let t = n as f32 / fs;
            let x = samples[n];
            acc_i += x * (two_pi * fc * t).cos();
            acc_q += x * -(two_pi * fc * t).sin();
        }
        symbols.push((acc_i * scale, acc_q * scale));
    }

    // 3. Pilot-aided symbol-level decode (preamble acquires, pilots track).
    Ok(PilotFrame::new().decode(&symbols))
}
