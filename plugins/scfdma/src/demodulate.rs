//! SC-FDMA demodulation: samples → FFT → LS/MMSE equalize → IDFT → payload.

use num_complex::Complex32;
use rustfft::FftPlanner;

use crate::channel::{
    deramp_timing, dft_ce_estimate, estimate_noise_var, estimate_rician_k_linear, mmse_equalize,
    mmse_llr_noise_var, pilot_positions,
};
use crate::modulate::{modulate_with_params, preamble_payload};
use crate::params::PILOT_AMPLITUDE;
use crate::params::{params_for_mode, ScFdmaParams, CP, FFT_SIZE, SYM_LEN};
use openpulse_dsp::constellation::{
    constellation_points, demap_symbol, estimate_decision_noise_var, symbol_llrs,
};

// Re-export from the canonical core implementation so the plugin exposes the
// same public path without duplicating the logic.
use openpulse_core::error::ModemError;
pub use openpulse_core::fec::combine_llrs_weighted;
use openpulse_core::len_prefix::{
    decode_len_prefix, decode_len_prefix_llrs, LEN_PREFIX_BITS, LEN_PREFIX_BYTES,
};
// Canonical shared implementation (openpulse-dsp); re-exported because the
// lib.rs acquisition regression test references it via this module's path.
#[cfg(test)]
pub(crate) use openpulse_dsp::acquisition::quadrature;
use openpulse_dsp::acquisition::IqMatchedFilter;

pub fn scfdma_demodulate(samples: &[f32], mode: &str) -> Result<Vec<u8>, ModemError> {
    let p = params_for_mode(mode).ok_or_else(|| {
        ModemError::Configuration(format!("SC-FDMA plugin: unknown mode '{mode}'"))
    })?;
    demodulate_with_params(samples, &p)
}

/// Demodulate SC-FDMA samples and return per-bit soft values (LLRs).
///
/// Positive values indicate bit 0 is more likely; negative values indicate bit 1.
pub fn scfdma_demodulate_soft(samples: &[f32], mode: &str) -> Result<Vec<f32>, ModemError> {
    let p = params_for_mode(mode).ok_or_else(|| {
        ModemError::Configuration(format!("SC-FDMA plugin: unknown mode '{mode}'"))
    })?;
    Ok(demodulate_soft_with_params(samples, &p)?.llrs)
}

/// Per-frame quality metrics produced during soft demodulation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoftFrameMetrics {
    /// Mean pilot-residual noise variance across demodulated symbols.
    pub mean_noise_var: f32,
    /// Mean estimated Rician K-factor in dB across symbols.
    pub mean_rician_k_db: f32,
    /// Number of symbols included in the metric averages.
    pub symbols_used: usize,
}

/// Soft demodulation output with reliability metrics for adaptive combining.
#[derive(Debug, Clone, PartialEq)]
pub struct SoftDemodOutput {
    /// Payload LLRs (positive => likely 0, negative => likely 1).
    pub llrs: Vec<f32>,
    /// Aggregated frame metrics measured from pilots/channel estimate.
    pub metrics: SoftFrameMetrics,
}

/// Demodulate SC-FDMA samples into LLRs and frame quality metrics.
pub fn scfdma_demodulate_soft_with_metrics(
    samples: &[f32],
    mode: &str,
) -> Result<SoftDemodOutput, ModemError> {
    let p = params_for_mode(mode).ok_or_else(|| {
        ModemError::Configuration(format!("SC-FDMA plugin: unknown mode '{mode}'"))
    })?;
    demodulate_soft_with_params(samples, &p)
}

/// Combine multiple LLR attempts using inverse-noise variance weighting.
fn demodulate_with_params(samples: &[f32], p: &ScFdmaParams) -> Result<Vec<u8>, ModemError> {
    let sync = modulate_with_params(&preamble_payload(p), p);
    if samples.len() < sync.len() + SYM_LEN {
        return Err(ModemError::Demodulation("signal too short".into()));
    }

    let Some(offset) = find_sync_offset(samples, &sync) else {
        return Err(ModemError::Demodulation("no SC-FDMA sync detected".into()));
    };
    let payload_start = offset + sync.len();
    if payload_start >= samples.len() {
        return Err(ModemError::Demodulation(
            "SC-FDMA frame truncated after sync".into(),
        ));
    }

    let samples = &samples[payload_start..];
    let n_syms = samples.len() / SYM_LEN;
    if n_syms == 0 {
        return Err(ModemError::Demodulation(
            "SC-FDMA frame truncated after sync".into(),
        ));
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    // N_data-point IDFT to undo DFT precoding.
    let idft = planner.plan_fft_inverse(p.n_data);
    // P-point IDFT for DFT-CE pilot CIR estimation — planned once, reused per symbol.
    let ce_idft = planner.plan_fft_inverse(p.n_pilots);

    let fft_scale = 1.0 / (FFT_SIZE as f32).sqrt();
    let idft_scale = 1.0 / (p.n_data as f32).sqrt();

    let mut bits: Vec<bool> = Vec::with_capacity(n_syms * p.bits_per_symbol());

    for sym_idx in 0..n_syms {
        let start = sym_idx * SYM_LEN + CP;
        if start + FFT_SIZE > samples.len() {
            break;
        }

        // Step 1: FFT(256) on the symbol body.
        let mut freq: Vec<Complex32> = samples[start..start + FFT_SIZE]
            .iter()
            .map(|&s| Complex32::new(s * fft_scale, 0.0))
            .collect();
        fft.process(&mut freq);

        // Remove the per-symbol sampling-frequency-offset phase ramp before
        // de-spreading (mirrors the OFDM path); critical for SC-FDMA under SRO.
        deramp_timing(p, &mut freq);

        // Step 2: DFT-domain channel estimation + MMSE equalization.
        let h_est = dft_ce_estimate(p, &freq, &*ce_idft);
        let noise_var = estimate_noise_var(p, &freq, &h_est);
        let mut equalized = mmse_equalize(p, &freq, &h_est, noise_var);

        // Step 3: IDFT(N_data) — undo DFT precoding; scale to preserve energy.
        idft.process(&mut equalized);
        let data_syms: Vec<Complex32> = equalized.iter().map(|c| c * idft_scale).collect();

        // Step 4: Demap recovered symbols according to the constellation order.
        for sym in &data_syms {
            let b = demap_symbol(*sym, p.bits_per_sc);
            for bit_pos in 0..p.bits_per_sc {
                bits.push((b >> bit_pos) & 1 == 1);
            }
        }
    }

    let raw = bits_to_bytes(&bits);

    // Strip the majority-protected length prefix.
    let Some(payload_len) = decode_len_prefix(&raw) else {
        return Err(ModemError::Demodulation(
            "SC-FDMA frame shorter than length prefix".into(),
        ));
    };
    let available = raw.len() - LEN_PREFIX_BYTES;
    let take = (payload_len as usize).min(available);
    Ok(raw[LEN_PREFIX_BYTES..LEN_PREFIX_BYTES + take].to_vec())
}

/// Equalized, de-spread constellation symbols for display — the real QAM scatter the receiver
/// recovers (FFT → DFT-CE → MMSE → IDFT), normalized to RMS ≈ 1 and capped in point count. Returns
/// `None` if the mode is unknown or sync fails. Display-only (mirrors the demod front-end); does not
/// touch the decode path.
pub fn scfdma_constellation(samples: &[f32], mode: &str) -> Option<Vec<(f32, f32)>> {
    let p = params_for_mode(mode)?;
    let sync = modulate_with_params(&preamble_payload(&p), &p);
    if samples.len() < sync.len() + SYM_LEN {
        return None;
    }
    let offset = find_sync_offset(samples, &sync)?;
    let payload_start = offset + sync.len();
    if payload_start >= samples.len() {
        return None;
    }
    let samples = &samples[payload_start..];
    let n_syms = samples.len() / SYM_LEN;
    if n_syms == 0 {
        return None;
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let idft = planner.plan_fft_inverse(p.n_data);
    let ce_idft = planner.plan_fft_inverse(p.n_pilots);
    let fft_scale = 1.0 / (FFT_SIZE as f32).sqrt();
    let idft_scale = 1.0 / (p.n_data as f32).sqrt();

    let mut syms: Vec<Complex32> = Vec::with_capacity(n_syms * p.n_data);
    for sym_idx in 0..n_syms {
        let start = sym_idx * SYM_LEN + CP;
        if start + FFT_SIZE > samples.len() {
            break;
        }
        let mut freq: Vec<Complex32> = samples[start..start + FFT_SIZE]
            .iter()
            .map(|&s| Complex32::new(s * fft_scale, 0.0))
            .collect();
        fft.process(&mut freq);
        deramp_timing(&p, &mut freq);
        let h_est = dft_ce_estimate(&p, &freq, &*ce_idft);
        let noise_var = estimate_noise_var(&p, &freq, &h_est);
        let mut equalized = mmse_equalize(&p, &freq, &h_est, noise_var);
        idft.process(&mut equalized);
        syms.extend(equalized.iter().map(|c| c * idft_scale));
    }
    Some(normalize_constellation_for_display(&syms))
}

/// Normalize equalized symbols to RMS ≈ 1 and subsample to a bounded point count for plotting.
pub(crate) fn normalize_constellation_for_display(syms: &[Complex32]) -> Vec<(f32, f32)> {
    const MAX_POINTS: usize = 256;
    if syms.is_empty() {
        return Vec::new();
    }
    let rms = (syms.iter().map(|c| c.norm_sqr()).sum::<f32>() / syms.len() as f32)
        .sqrt()
        .max(1e-9);
    let step = (syms.len() / MAX_POINTS).max(1);
    syms.iter()
        .step_by(step)
        .map(|c| (c.re / rms, c.im / rms))
        .collect()
}

fn demodulate_soft_with_params(
    samples: &[f32],
    p: &ScFdmaParams,
) -> Result<SoftDemodOutput, ModemError> {
    let sync = modulate_with_params(&preamble_payload(p), p);
    if samples.len() < sync.len() + SYM_LEN {
        return Err(ModemError::Demodulation("signal too short".into()));
    }

    let Some(offset) = find_sync_offset(samples, &sync) else {
        return Err(ModemError::Demodulation("no SC-FDMA sync detected".into()));
    };
    let payload_start = offset + sync.len();
    if payload_start >= samples.len() {
        return Err(ModemError::Demodulation(
            "SC-FDMA frame truncated after sync".into(),
        ));
    }

    let samples = &samples[payload_start..];
    let n_syms = samples.len() / SYM_LEN;
    if n_syms == 0 {
        return Err(ModemError::Demodulation(
            "SC-FDMA frame truncated after sync".into(),
        ));
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let idft = planner.plan_fft_inverse(p.n_data);
    let ce_idft = planner.plan_fft_inverse(p.n_pilots);

    let fft_scale = 1.0 / (FFT_SIZE as f32).sqrt();
    let idft_scale = 1.0 / (p.n_data as f32).sqrt();

    let points = constellation_points(p.bits_per_sc);
    let mut llrs = Vec::with_capacity(n_syms * p.bits_per_symbol());
    let mut noise_sum = 0.0f32;
    let mut k_db_sum = 0.0f32;
    let mut metric_symbols = 0usize;
    let pilot_scs = pilot_positions(p);
    let mut h_pilots_buf = vec![Complex32::new(0.0, 0.0); pilot_scs.len()];

    for sym_idx in 0..n_syms {
        let start = sym_idx * SYM_LEN + CP;
        if start + FFT_SIZE > samples.len() {
            break;
        }

        let mut freq: Vec<Complex32> = samples[start..start + FFT_SIZE]
            .iter()
            .map(|&s| Complex32::new(s * fft_scale, 0.0))
            .collect();
        fft.process(&mut freq);

        // Remove the per-symbol sampling-frequency-offset phase ramp before
        // de-spreading (mirrors the OFDM path); critical for SC-FDMA under SRO.
        deramp_timing(p, &mut freq);

        let h_est = dft_ce_estimate(p, &freq, &*ce_idft);
        let pilot_noise_var = estimate_noise_var(p, &freq, &h_est).max(1e-6);

        // Rician K for SoftFrameMetrics: reuse pre-allocated buffer to avoid per-symbol allocation.
        for (buf, &sc) in h_pilots_buf.iter_mut().zip(pilot_scs.iter()) {
            *buf = freq[sc] / Complex32::new(PILOT_AMPLITUDE, 0.0);
        }
        let k_linear = estimate_rician_k_linear(&h_pilots_buf);
        let k_db = 10.0 * (k_linear + 1e-6).log10();

        let (llr_noise_var, alpha_avg) = mmse_llr_noise_var(p, &h_est, pilot_noise_var);
        let mut equalized = mmse_equalize(p, &freq, &h_est, pilot_noise_var);

        idft.process(&mut equalized);
        // Divide by alpha_avg to restore unit-constellation scale after MMSE bias.
        let data_syms: Vec<Complex32> = equalized
            .iter()
            .map(|c| *c * idft_scale / alpha_avg)
            .collect();

        for sym in &data_syms {
            llrs.extend(symbol_llrs(*sym, p.bits_per_sc, llr_noise_var, &points));
        }

        // Decision-residual metric for inverse-noise combining: computed after
        // alpha_avg normalization so symbols are on the unit-constellation scale.
        let decision_noise_var = estimate_decision_noise_var(&data_syms, p.bits_per_sc).max(1e-6);
        noise_sum += decision_noise_var;
        k_db_sum += k_db;
        metric_symbols += 1;
    }

    let Some(payload_len) = decode_len_prefix_llrs(&llrs) else {
        return Err(ModemError::Demodulation(
            "SC-FDMA frame shorter than length prefix".into(),
        ));
    };
    let payload_bits = (payload_len as usize).saturating_mul(8);
    let available_payload_bits = llrs.len().saturating_sub(LEN_PREFIX_BITS);
    let take = if payload_bits == 0 && available_payload_bits > 0 {
        // A noisy length prefix can decode to zero under fading; in that case,
        // return all whole-byte payload bits so downstream soft combining still
        // has useful information.
        available_payload_bits - (available_payload_bits % 8)
    } else {
        payload_bits.min(available_payload_bits)
    };
    Ok(SoftDemodOutput {
        llrs: llrs[LEN_PREFIX_BITS..LEN_PREFIX_BITS + take].to_vec(),
        metrics: SoftFrameMetrics {
            mean_noise_var: noise_sum / metric_symbols.max(1) as f32,
            mean_rician_k_db: k_db_sum / metric_symbols.max(1) as f32,
            symbols_used: metric_symbols,
        },
    })
}

/// Locate the preamble within `samples` via a phase-insensitive matched filter.
///
/// A bare real cross-correlation (`Σ a·b`) is carrier-phase sensitive: over the
/// async-audio loopback the two sound-card clocks impose an arbitrary carrier
/// phase, and a ~90° rotation collapses the real correlation to near zero,
/// landing on a wrong offset.  The shared [`IqMatchedFilter`] correlates
/// against BOTH the preamble and its quadrature (Hilbert) companion and
/// maximises the magnitude, removing that dependence — the per-symbol
/// pilot/MMSE equalizer then handles the residual phase.  The search is
/// bounded to the slice front: the receive engine aligns each window to the
/// detected signal start, so the preamble appears near the front, and an
/// unbounded scan over a multi-second slice is O(N²) (too slow for the
/// real-time loop) and prone to spurious far-field peaks.
///
/// Returns `None` when the best alignment's normalised correlation falls below
/// the detection floor — on a no-signal window the unnormalised argmax is an
/// arbitrary noise offset, and demodulating from it produces garbage bytes
/// (including a random length prefix) at full frame cost.
fn find_sync_offset(samples: &[f32], sync: &[f32]) -> Option<usize> {
    if samples.len() <= sync.len() {
        return None;
    }
    const SEARCH_CAP: usize = 8192;
    // Minimum normalised correlation to accept a sync lock.  Noise scores
    // ≲ 0.1 with a multi-symbol template; a real (even band-limited, faded)
    // preamble correlates well above this.
    const DETECTION_FLOOR_RHO: f32 = 0.15;

    let filt = IqMatchedFilter::new(sync.to_vec());
    let result = filt.search(samples, SEARCH_CAP)?;
    if result.rho < DETECTION_FLOOR_RHO {
        return None;
    }
    Some(result.offset)
}

// ── Constellation demapping (shared) ──────────────────────────────────────────
// Hard demapper, soft-LLR, constellation points, and decision-noise estimation
// come from `openpulse_dsp::constellation` (imported above).

// ── Bit helpers ───────────────────────────────────────────────────────────────

fn bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8)
        .map(|c| {
            c.iter()
                .enumerate()
                .fold(0u8, |acc, (i, &b)| acc | ((b as u8) << i))
        })
        .collect()
}

/// GPU-accelerated soft demodulator.  Batches all per-symbol 256-point FFTs in
/// a single GPU dispatch; channel estimation, MMSE equalization, IDFT, and LLR
/// computation remain on CPU.  Returns `None` on GPU error (caller falls back).
#[cfg(feature = "gpu")]
pub fn scfdma_demodulate_soft_gpu(
    samples: &[f32],
    mode: &str,
    ctx: &std::sync::Arc<openpulse_gpu::GpuContext>,
) -> Option<Vec<f32>> {
    let p = params_for_mode(mode)?;

    let sync = modulate_with_params(&preamble_payload(&p), &p);
    if samples.len() < sync.len() + SYM_LEN {
        return None;
    }

    let offset = find_sync_offset(samples, &sync)?;
    let payload_start = offset + sync.len();
    if payload_start >= samples.len() {
        return None;
    }

    let payload_samples = &samples[payload_start..];
    let n_syms = payload_samples.len() / SYM_LEN;
    if n_syms == 0 {
        return None;
    }

    let fft_scale = 1.0 / (FFT_SIZE as f32).sqrt();
    let mut packed: Vec<f32> = Vec::with_capacity(n_syms * FFT_SIZE * 2);
    for sym_idx in 0..n_syms {
        let start = sym_idx * SYM_LEN + CP;
        if start + FFT_SIZE > payload_samples.len() {
            break;
        }
        for &s in &payload_samples[start..start + FFT_SIZE] {
            packed.push(s * fft_scale);
            packed.push(0.0);
        }
    }
    let actual_syms = packed.len() / (FFT_SIZE * 2);
    if actual_syms == 0 {
        return None;
    }

    let gpu_out = openpulse_gpu::gpu_fft256_batch(ctx, &packed, true)?;

    let mut planner = rustfft::FftPlanner::<f32>::new();
    let idft = planner.plan_fft_inverse(p.n_data);
    let ce_idft = planner.plan_fft_inverse(p.n_pilots);
    let idft_scale = 1.0 / (p.n_data as f32).sqrt();

    let points = constellation_points(p.bits_per_sc);
    let pilot_scs = crate::channel::pilot_positions(&p);
    let mut h_pilots_buf = vec![Complex32::new(0.0, 0.0); pilot_scs.len()];
    let mut all_llrs: Vec<f32> = Vec::with_capacity(actual_syms * p.bits_per_symbol());

    for sym_idx in 0..actual_syms {
        let base = sym_idx * FFT_SIZE * 2;
        let freq: Vec<Complex32> = (0..FFT_SIZE)
            .map(|k| Complex32::new(gpu_out[base + k * 2], gpu_out[base + k * 2 + 1]))
            .collect();

        let h_est = dft_ce_estimate(&p, &freq, &*ce_idft);
        let pilot_noise_var = estimate_noise_var(&p, &freq, &h_est).max(1e-6);

        for (buf, &sc) in h_pilots_buf.iter_mut().zip(pilot_scs.iter()) {
            *buf = freq[sc] / Complex32::new(crate::params::PILOT_AMPLITUDE, 0.0);
        }

        let (llr_noise_var, alpha_avg) = mmse_llr_noise_var(&p, &h_est, pilot_noise_var);
        let mut equalized = mmse_equalize(&p, &freq, &h_est, pilot_noise_var);

        idft.process(&mut equalized);
        let data_syms: Vec<Complex32> = equalized
            .iter()
            .map(|c| *c * idft_scale / alpha_avg)
            .collect();

        for sym in &data_syms {
            all_llrs.extend(symbol_llrs(*sym, p.bits_per_sc, llr_noise_var, &points));
        }
    }

    // Strip the majority-protected length prefix from the LLR stream,
    // mirroring the CPU path.
    let payload_len = decode_len_prefix_llrs(&all_llrs)? as usize;
    let payload_bits = payload_len.saturating_mul(8);
    let available_payload_bits = all_llrs.len().saturating_sub(LEN_PREFIX_BITS);
    let take = if payload_bits == 0 && available_payload_bits > 0 {
        available_payload_bits - (available_payload_bits % 8)
    } else {
        payload_bits.min(available_payload_bits)
    };
    Some(all_llrs[LEN_PREFIX_BITS..LEN_PREFIX_BITS + take].to_vec())
}

/// GPU-accelerated hard demodulator.  Batches all per-symbol 256-point FFTs
/// into a single GPU dispatch; channel estimation, MMSE equalization, IDFT, and
/// demapping remain on CPU.  Returns `None` on GPU error (caller falls back to CPU).
#[cfg(feature = "gpu")]
pub fn scfdma_demodulate_gpu(
    samples: &[f32],
    mode: &str,
    ctx: &std::sync::Arc<openpulse_gpu::GpuContext>,
) -> Option<Vec<u8>> {
    let p = params_for_mode(mode)?;

    let sync = modulate_with_params(&preamble_payload(&p), &p);
    if samples.len() < sync.len() + SYM_LEN {
        return None;
    }

    let offset = find_sync_offset(samples, &sync)?;
    let payload_start = offset + sync.len();
    if payload_start >= samples.len() {
        return None;
    }

    let payload_samples = &samples[payload_start..];
    let n_syms = payload_samples.len() / SYM_LEN;
    if n_syms == 0 {
        return None;
    }

    // Pack all symbol windows as interleaved (re, 0) complex f32 pairs.
    let mut packed: Vec<f32> = Vec::with_capacity(n_syms * FFT_SIZE * 2);
    for sym_idx in 0..n_syms {
        let start = sym_idx * SYM_LEN + CP;
        if start + FFT_SIZE > payload_samples.len() {
            break;
        }
        let fft_scale = 1.0 / (FFT_SIZE as f32).sqrt();
        for &s in &payload_samples[start..start + FFT_SIZE] {
            packed.push(s * fft_scale);
            packed.push(0.0);
        }
    }
    let actual_syms = packed.len() / (FFT_SIZE * 2);
    if actual_syms == 0 {
        return None;
    }

    let gpu_out = openpulse_gpu::gpu_fft256_batch(ctx, &packed, true)?;

    // Reconstruct Complex32 frequency bins per symbol and run CPU equalization.
    let mut planner = rustfft::FftPlanner::<f32>::new();
    let idft = planner.plan_fft_inverse(p.n_data);
    let ce_idft = planner.plan_fft_inverse(p.n_pilots);
    let idft_scale = 1.0 / (p.n_data as f32).sqrt();

    let mut bits: Vec<bool> = Vec::with_capacity(actual_syms * p.bits_per_symbol());

    for sym_idx in 0..actual_syms {
        let base = sym_idx * FFT_SIZE * 2;
        let freq: Vec<Complex32> = (0..FFT_SIZE)
            .map(|k| Complex32::new(gpu_out[base + k * 2], gpu_out[base + k * 2 + 1]))
            .collect();

        let h_est = dft_ce_estimate(&p, &freq, &*ce_idft);
        let noise_var = estimate_noise_var(&p, &freq, &h_est);
        let mut equalized = mmse_equalize(&p, &freq, &h_est, noise_var);

        idft.process(&mut equalized);
        let data_syms: Vec<Complex32> = equalized.iter().map(|c| c * idft_scale).collect();

        for sym in &data_syms {
            let b = demap_symbol(*sym, p.bits_per_sc);
            for bit_pos in 0..p.bits_per_sc {
                bits.push((b >> bit_pos) & 1 == 1);
            }
        }
    }

    let raw = bits_to_bytes(&bits);

    // Strip the majority-protected length prefix (mirrors the CPU path).
    let payload_len = decode_len_prefix(&raw)? as usize;
    let available = raw.len() - LEN_PREFIX_BYTES;
    let take = payload_len.min(available);
    Some(raw[LEN_PREFIX_BYTES..LEN_PREFIX_BYTES + take].to_vec())
}
