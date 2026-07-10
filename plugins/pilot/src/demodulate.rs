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
//!
//! Coarse CFO ([`pilot_estimate_afc_hz`]) is data-aided off the same preamble, so
//! the engine's AFC stage can pre-correct the carrier before this decode runs —
//! which keeps the onset correlation (offset-sensitive at the integer-sample
//! level) accurate.

use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::ModulationConfig;
use openpulse_dsp::acquisition::{estimate_cfo_data_aided, goertzel_carrier_scan, IqMatchedFilter};
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::rrc::generate_rrc_coefficients;

use crate::frame::PilotFrame;
use crate::modulate::{
    baud_for_mode, is_rrc_mode, pilot_frame_for_mode, preamble_template, samples_per_symbol,
    RRC_ALPHA, RRC_SPAN_SYMBOLS,
};

/// Locate the frame onset by phase-insensitive correlation against the passband
/// preamble.
fn find_onset(samples: &[f32], config: &ModulationConfig) -> Option<usize> {
    let template = preamble_template(config).ok()?;
    let mf = IqMatchedFilter::new(template);
    if samples.len() < mf.len() {
        return None;
    }
    let bound = samples.len() - mf.len();
    // Acquire on the *normalised* correlation, not the unnormalised score: the latter's argmax favours a
    // high-energy window and can lock a later data-region window that merely shares the pilot structure
    // over a faded preamble (the #689 SC-FDMA bug — it never propagated to the pilot plugin). A 1 % energy
    // floor keeps ρ meaningful (it is undefined on a silent window).
    mf.search_normalized(samples, bound, 0.01).map(|r| r.offset)
}

/// Coherently downconvert and integrate-and-dump `count` symbol periods starting
/// at `onset`. Returns the recovered complex symbols (clamped to what fits).
fn integrate_and_dump(
    samples: &[f32],
    onset: usize,
    sps: usize,
    count: usize,
    fc: f32,
    fs: f32,
) -> Vec<(f32, f32)> {
    let two_pi = 2.0 * PI;
    let scale = 2.0 / sps as f32;
    let avail = samples.len().saturating_sub(onset) / sps;
    let n = count.min(avail);
    let mut symbols = Vec::with_capacity(n);
    for k in 0..n {
        let start = onset + k * sps;
        let mut acc_i = 0.0f32;
        let mut acc_q = 0.0f32;
        for j in 0..sps {
            let idx = start + j;
            let t = idx as f32 / fs;
            let x = samples[idx];
            acc_i += x * (two_pi * fc * t).cos();
            acc_q += x * -(two_pi * fc * t).sin();
        }
        symbols.push((acc_i * scale, acc_q * scale));
    }
    symbols
}

/// Matched-filter symbol recovery for the RRC variants: coherently downconvert to
/// baseband, apply the matched root-raised-cosine filter, and sample at the symbol
/// instants (`onset + group_delay + k·sps`). The RRC matched filter replaces the
/// rectangular integrate-and-dump; pilot tracking still recovers the carrier.
fn matched_symbols_rrc(
    samples: &[f32],
    onset: usize,
    sps: usize,
    count: usize,
    fc: f32,
    fs: f32,
    baud: f32,
) -> Vec<(f32, f32)> {
    let two_pi = 2.0 * PI;
    let mut bb_i = Vec::with_capacity(samples.len());
    let mut bb_q = Vec::with_capacity(samples.len());
    for (n, &x) in samples.iter().enumerate() {
        let t = n as f32 / fs;
        bb_i.push(2.0 * x * (two_pi * fc * t).cos());
        bb_q.push(2.0 * x * -(two_pi * fc * t).sin());
    }

    let num_taps = RRC_SPAN_SYMBOLS * sps + 1;
    let coeffs = generate_rrc_coefficients(fs, baud, RRC_ALPHA, num_taps);
    let group_delay = (num_taps - 1) / 2;
    // Pad by the group delay so the symbol-instant samples (shifted by the matched
    // filter's delay) are all in range.
    let filter_bb = |bb: Vec<f32>| -> Vec<f32> {
        let padded: Vec<f32> = bb
            .into_iter()
            .chain(std::iter::repeat_n(0.0, group_delay))
            .collect();
        FirFilter::new(coeffs.clone()).apply(&padded)
    };
    let fi = filter_bb(bb_i);
    let fq = filter_bb(bb_q);

    let mut symbols = Vec::with_capacity(count);
    for k in 0..count {
        let idx = onset + group_delay + k * sps;
        if idx >= fi.len() {
            break;
        }
        symbols.push((fi[idx], fq[idx]));
    }
    symbols
}

/// Recover complex symbols from `onset`, dispatching on the mode's pulse shape:
/// rectangular integrate-and-dump or RRC matched filter.
fn recover_symbols(
    samples: &[f32],
    onset: usize,
    sps: usize,
    count: usize,
    config: &ModulationConfig,
) -> Vec<(f32, f32)> {
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    if is_rrc_mode(&config.mode) {
        let baud = baud_for_mode(&config.mode).unwrap_or(500.0);
        matched_symbols_rrc(samples, onset, sps, count, fc, fs, baud)
    } else {
        integrate_and_dump(samples, onset, sps, count, fc, fs)
    }
}

/// Demodulate pilot-framed QPSK passband audio back to bytes.
pub fn pilot_demodulate(samples: &[f32], config: &ModulationConfig) -> Result<Vec<u8>, ModemError> {
    let sps = samples_per_symbol(config)?;

    let Some(onset) = find_onset(samples, config) else {
        return Ok(Vec::new());
    };

    let frame = pilot_frame_for_mode(&config.mode)?;
    let total_syms = samples.len().saturating_sub(onset) / sps;
    let symbols = recover_symbols(samples, onset, sps, total_syms, config);
    Ok(frame.decode(&symbols))
}

/// Soft-decision counterpart of [`pilot_demodulate`]: same onset/downconvert/
/// integrate-and-dump and pilot-tracked carrier recovery, but emits per-bit LLRs
/// (positive = bit more likely 0) for the soft FEC decoders. Hard-slicing the
/// LLRs reproduces [`pilot_demodulate`]'s bytes.
pub fn pilot_demodulate_soft(
    samples: &[f32],
    config: &ModulationConfig,
) -> Result<Vec<f32>, ModemError> {
    let sps = samples_per_symbol(config)?;

    let Some(onset) = find_onset(samples, config) else {
        return Ok(Vec::new());
    };

    let frame = pilot_frame_for_mode(&config.mode)?;
    let total_syms = samples.len().saturating_sub(onset) / sps;
    let symbols = recover_symbols(samples, onset, sps, total_syms, config);
    Ok(frame.decode_soft(&symbols))
}

/// Estimate the carrier frequency offset (Hz) for the engine's AFC stage.
///
/// Two stages, mirroring the fielded single-carrier modes:
/// 1. **Coarse** — a wide 2nd-power Goertzel scan over the *preamble portion*
///    only. The preamble and pilots are BPSK regardless of the data
///    constellation, so squaring leaves a clean line at 2·fc even for QAM data
///    (whose M-th power is not a clean tone); the engine's energy-based onset
///    makes the window start at the preamble. ±400 Hz / 12.5 Hz grid.
/// 2. **Fine** — at the coarse-corrected carrier the preamble correlates cleanly;
///    a data-aided mean-phase-increment estimate refines the residual.
///
/// The engine settles `coarse + fine` onto `center_frequency`, leaving a
/// near-zero residual before [`pilot_demodulate`] runs (so its integer-sample
/// onset stays accurate).
pub fn pilot_estimate_afc_hz(samples: &[f32], config: &ModulationConfig) -> Option<f32> {
    let sps = samples_per_symbol(config).ok()?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let baud = baud_for_mode(&config.mode).ok()?;

    let frame = PilotFrame::new(); // preamble is mode-independent (BPSK PN-63)
    let preamble = frame.preamble();

    // 1. Coarse 2nd-power scan over the (BPSK) preamble portion of the window.
    let plen_samples = (preamble.len() * sps).min(samples.len());
    let coarse =
        goertzel_carrier_scan(&samples[..plen_samples], fs, fc, 2, 400.0, 12.5).unwrap_or(0.0);

    // 2. Fine data-aided refinement at the coarse-corrected carrier.
    let fc_coarse = fc + coarse;
    let coarse_cfg = ModulationConfig {
        center_frequency: fc_coarse,
        ..config.clone()
    };
    let fine = find_onset(samples, &coarse_cfg)
        .map(|onset| recover_symbols(samples, onset, sps, preamble.len(), &coarse_cfg))
        .filter(|syms| syms.len() >= preamble.len())
        .and_then(|syms| {
            let i: Vec<f32> = syms.iter().map(|&(i, _)| i).collect();
            let q: Vec<f32> = syms.iter().map(|&(_, q)| q).collect();
            estimate_cfo_data_aided(&i, &q, preamble, baud)
        })
        .unwrap_or(0.0);

    Some(coarse + fine)
}
