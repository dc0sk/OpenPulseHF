//! OFDM demodulation: samples → FFT frames → LS/ZF equalize → payload.

use num_complex::Complex32;
use openpulse_dsp::acquisition::IqMatchedFilter;
use openpulse_dsp::constellation::{
    constellation_points, demap_symbol, estimate_decision_noise_var, symbol_llrs,
};
use rustfft::FftPlanner;

use openpulse_core::error::ModemError;
use openpulse_core::len_prefix::{
    decode_len_prefix, decode_len_prefix_llrs, LEN_PREFIX_BITS, LEN_PREFIX_BYTES,
};

use crate::channel::{is_pilot, ls_estimate, zf_equalize};
use crate::params::{params_for_mode, OfdmParams, CP, FFT_SIZE, SYM_LEN};

pub fn ofdm_demodulate(samples: &[f32], mode: &str) -> Result<Vec<u8>, ModemError> {
    let p = params_for_mode(mode)
        .ok_or_else(|| ModemError::Configuration(format!("OFDM plugin: unknown mode '{mode}'")))?;
    demodulate_with_params(samples, &p)
}

/// Demodulate OFDM samples and return per-bit soft LLRs.
///
/// After ZF equalization each subcarrier carries one `bits_per_sc` constellation
/// symbol; max-log-MAP gives `bits_per_sc` LLRs per symbol.  The per-subcarrier
/// effective noise is scaled by `mean|H|² / |H_sc|²`, so faded subcarriers yield
/// lower-confidence LLRs — the per-subcarrier weighting that makes OFDM robust to
/// frequency-selective fades.  For QPSK this reduces to the |H|²-weighted
/// matched-filter LLR.
///
/// **LLR sign convention**: positive = bit more likely 0, matching all other
/// plugins and codecs in this codebase.
///
/// The majority-protected length prefix inserted by `ofdm_modulate` is
/// consumed and excluded from the output.
pub fn ofdm_demodulate_soft(samples: &[f32], mode: &str) -> Result<Vec<f32>, ModemError> {
    let p = params_for_mode(mode)
        .ok_or_else(|| ModemError::Configuration(format!("OFDM plugin: unknown mode '{mode}'")))?;
    demodulate_soft_with_params(samples, &p)
}

/// Locate the first data-symbol body via Schmidl-Cox preamble acquisition.
///
/// Returns the sample index of the FFT window for data symbol 0 (i.e. just past
/// the preamble), or `None` when no preamble correlation peak is found.
///
/// Acquisition is two-stage:
///
/// 1. **Coarse, CFO-robust presence detection** via a Schmidl-Cox half-symbol
///    autocorrelation `M(d) = P(d)²/R(d)²` over the whole slice.  The preamble
///    body has two identical halves of length `L = FFT_SIZE/2`, so `M → 1` on a
///    plateau of width `CP`; the argmax falls somewhere in that plateau.
/// 2. **Fine, sample-accurate timing** via a normalised cross-correlation
///    against the known clean preamble waveform, searched only in a small window
///    around the coarse peak.  The matched filter gives a single sharp peak at
///    the preamble's first sample (frame start), resolving the plateau ambiguity
///    that the autocorrelation alone cannot.
pub(crate) fn find_first_data_body(samples: &[f32], p: &OfdmParams) -> Option<usize> {
    const L: usize = FFT_SIZE / 2;
    // The preamble is always near the front of the slice: the receive engine
    // positions each demodulation window at the detected signal start and slides
    // it forward symbol-by-symbol, so when the preamble is present it appears
    // within the first few hundred samples.  Bounding the autocorrelation search
    // keeps this O(SEARCH_CAP) per call instead of O(slice_len) — the engine may
    // hand us a slice tens of seconds long, and an unbounded scan both starves
    // the real-time receive loop and risks a spurious far-field correlation peak.
    const SEARCH_CAP: usize = 8192;
    if samples.len() < 2 * L + 1 {
        return None;
    }
    let max_d = (samples.len() - 2 * L).min(SEARCH_CAP);

    // ── Stage 1: coarse Schmidl-Cox autocorrelation ────────────────────────────
    // Normalised by the MEAN energy of both half-windows (Minn variant), not
    // just the second half.  The classic P²/R₂² form explodes at the frame's
    // TRAILING edge: the first half holds the signal tail, the second holds
    // near-silence, R₂ → ε and M → 10³⁺, beating the true preamble's M ≈ 1
    // and locking acquisition onto the end of the frame.  On hardware this is
    // gated by the sound card's noise floor — a quiet card exposes it.  With
    // the mean-energy normalisation a silent half drags M toward 0 instead.
    let mut p_acc = 0.0f32;
    let mut r1_acc = 0.0f32;
    let mut r2_acc = 0.0f32;
    for m in 0..L {
        p_acc += samples[m] * samples[m + L];
        r1_acc += samples[m] * samples[m];
        r2_acc += samples[m + L] * samples[m + L];
    }
    let mut best_m = 0.0f32;
    let mut best_d = 0usize;
    for d in 0..=max_d {
        let r_mean = 0.5 * (r1_acc + r2_acc);
        let m = if r_mean > 1e-9 {
            (p_acc * p_acc) / (r_mean * r_mean)
        } else {
            0.0
        };
        if m > best_m {
            best_m = m;
            best_d = d;
        }
        if d < max_d {
            p_acc += -samples[d] * samples[d + L] + samples[d + L] * samples[d + 2 * L];
            r1_acc += -samples[d] * samples[d] + samples[d + L] * samples[d + L];
            r2_acc += -samples[d + L] * samples[d + L] + samples[d + 2 * L] * samples[d + 2 * L];
        }
    }
    // Require a clear correlation peak (M → 1 at perfect alignment).
    if best_m < 0.5 {
        return None;
    }

    // ── Stage 2: matched-filter fine timing ────────────────────────────────────
    // The shared IqMatchedFilter correlates against BOTH the in-phase preamble
    // and its quadrature (Hilbert) companion, then maximises the correlation
    // magnitude.  This is insensitive to the channel's carrier phase, so a
    // multipath/fading channel cannot drag the timing peak off the true frame
    // start the way a bare real correlation would.
    let template = crate::modulate::preamble_template(p);
    let tlen = template.len();
    if samples.len() < tlen {
        return None;
    }
    let filt = IqMatchedFilter::new(template);
    if filt.is_empty() {
        return None;
    }

    // The frame start (preamble's first sample) lies at or just left of the
    // autocorrelation plateau.  Search a window that brackets it.
    let lo = best_d.saturating_sub(L);
    let hi = (best_d + CP).min(samples.len() - tlen);
    if lo > hi {
        return None;
    }
    let rhos = filt.rho_profile(samples, lo, hi);
    if rhos.is_empty() {
        return None;
    }
    let (peak_idx, &best_rho) = rhos
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap_or((0, &0.0));
    // Lock to the LEADING path, not the strongest.  On a multipath channel a
    // delayed echo can be the dominant correlation peak; starting the FFT window
    // there would read into the next symbol (inter-symbol interference).  Starting
    // at — or just before — the first path is absorbed by the cyclic prefix as a
    // benign phase ramp.  Scan back up to one CP from the peak for the earliest
    // tap above 0.20 × the peak correlation (hardware-tuned; low enough to catch
    // a weak leading path, high enough to reject pre-peak noise).
    let lead_thresh = best_rho * 0.20;
    let search_lo = peak_idx.saturating_sub(CP);
    let chosen = (search_lo..=peak_idx)
        .find(|&i| rhos[i] >= lead_thresh)
        .unwrap_or(peak_idx);
    let frame_start = lo + chosen;

    // Data symbol 0 body = frame start + full preamble symbol (SYM_LEN) + the
    // first data symbol's own cyclic prefix (CP).
    Some(frame_start + SYM_LEN + CP)
}

fn demodulate_with_params(samples: &[f32], p: &OfdmParams) -> Result<Vec<u8>, ModemError> {
    let Some(data_start) = find_first_data_body(samples, p) else {
        return Err(ModemError::Demodulation("no OFDM preamble detected".into()));
    };
    if data_start >= samples.len() {
        return Err(ModemError::Demodulation(
            "OFDM frame truncated before first data symbol".into(),
        ));
    }
    let n_syms = (samples.len() - data_start + CP) / SYM_LEN;
    if n_syms == 0 {
        return Err(ModemError::Demodulation(
            "OFDM frame truncated before first data symbol".into(),
        ));
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let scale = 1.0 / (FFT_SIZE as f32).sqrt();

    let mut bits: Vec<bool> = Vec::with_capacity(n_syms * p.bits_per_symbol());

    for sym_idx in 0..n_syms {
        let start = data_start + sym_idx * SYM_LEN; // FFT window (CP already stripped)
        if start + FFT_SIZE > samples.len() {
            break;
        }

        let mut freq: Vec<Complex32> = samples[start..start + FFT_SIZE]
            .iter()
            .map(|&s| Complex32::new(s * scale, 0.0))
            .collect();
        fft.process(&mut freq);
        crate::channel::deramp_timing(p, &mut freq);

        // LS channel estimation + ZF equalization.
        let h_est = ls_estimate(p, &freq);
        let data_syms = zf_equalize(p, &freq, &h_est);

        // Hard-decode each subcarrier's constellation symbol.
        for sym in &data_syms {
            let label = demap_symbol(*sym, p.bits_per_sc);
            for b in 0..p.bits_per_sc {
                bits.push((label >> b) & 1 == 1);
            }
        }
    }

    let raw = bits_to_bytes(&bits);

    // Strip the majority-protected length prefix.
    let Some(payload_len) = decode_len_prefix(&raw) else {
        return Err(ModemError::Demodulation(
            "OFDM frame shorter than length prefix".into(),
        ));
    };
    let available = raw.len() - LEN_PREFIX_BYTES;
    let take = (payload_len as usize).min(available);
    Ok(raw[LEN_PREFIX_BYTES..LEN_PREFIX_BYTES + take].to_vec())
}

/// Equalized data-subcarrier constellation symbols for display — the real QAM scatter the receiver
/// recovers (FFT → LS-CE → ZF), normalized to RMS ≈ 1 and capped in point count. Returns `None` if
/// the mode is unknown or no preamble is found. Display-only (mirrors the demod front-end).
pub fn ofdm_constellation(samples: &[f32], mode: &str) -> Option<Vec<(f32, f32)>> {
    let p = params_for_mode(mode)?;
    let data_start = find_first_data_body(samples, &p)?;
    if data_start >= samples.len() {
        return None;
    }
    let n_syms = (samples.len() - data_start + CP) / SYM_LEN;
    if n_syms == 0 {
        return None;
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let scale = 1.0 / (FFT_SIZE as f32).sqrt();

    let mut syms: Vec<Complex32> = Vec::new();
    for sym_idx in 0..n_syms {
        let start = data_start + sym_idx * SYM_LEN;
        if start + FFT_SIZE > samples.len() {
            break;
        }
        let mut freq: Vec<Complex32> = samples[start..start + FFT_SIZE]
            .iter()
            .map(|&s| Complex32::new(s * scale, 0.0))
            .collect();
        fft.process(&mut freq);
        crate::channel::deramp_timing(&p, &mut freq);
        let h_est = ls_estimate(&p, &freq);
        syms.extend(zf_equalize(&p, &freq, &h_est));
    }
    Some(normalize_constellation_for_display(&syms))
}

/// Normalize equalized symbols to RMS ≈ 1 and subsample to a bounded point count for plotting.
fn normalize_constellation_for_display(syms: &[Complex32]) -> Vec<(f32, f32)> {
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

fn demodulate_soft_with_params(samples: &[f32], p: &OfdmParams) -> Result<Vec<f32>, ModemError> {
    let Some(data_start) = find_first_data_body(samples, p) else {
        return Err(ModemError::Demodulation("no OFDM preamble detected".into()));
    };
    if data_start >= samples.len() {
        return Err(ModemError::Demodulation(
            "OFDM frame truncated before first data symbol".into(),
        ));
    }
    let n_syms = (samples.len() - data_start + CP) / SYM_LEN;
    if n_syms == 0 {
        return Err(ModemError::Demodulation(
            "OFDM frame truncated before first data symbol".into(),
        ));
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let scale = 1.0 / (FFT_SIZE as f32).sqrt();

    // bits_per_symbol() = 2 for QPSK; each symbol → 2 LLRs.
    let mut llrs: Vec<f32> = Vec::with_capacity(n_syms * p.bits_per_symbol());

    for sym_idx in 0..n_syms {
        let start = data_start + sym_idx * SYM_LEN;
        if start + FFT_SIZE > samples.len() {
            break;
        }

        let mut freq: Vec<Complex32> = samples[start..start + FFT_SIZE]
            .iter()
            .map(|&s| Complex32::new(s * scale, 0.0))
            .collect();
        fft.process(&mut freq);
        crate::channel::deramp_timing(p, &mut freq);

        let h_est = ls_estimate(p, &freq);
        let data_syms = zf_equalize(p, &freq, &h_est);

        // Per-data-subcarrier |H|² weights, in the same order as `data_syms`.
        let mut weights: Vec<f32> = Vec::with_capacity(data_syms.len());
        for (rel, _) in freq[p.first_sc..=p.last_sc].iter().enumerate() {
            let sc = p.first_sc + rel;
            if is_pilot(p, sc) {
                continue;
            }
            weights.push(h_est[rel].norm_sqr());
        }
        let mean_w = (weights.iter().sum::<f32>() / weights.len().max(1) as f32).max(1e-6);
        let block_noise = estimate_decision_noise_var(&data_syms, p.bits_per_sc);
        let points = constellation_points(p.bits_per_sc);

        for (sym, &w) in data_syms.iter().zip(weights.iter()) {
            // Faded subcarriers (low |H|²) get higher effective noise → lower-
            // confidence LLRs.  For QPSK this matches the old |H|²-weighted form.
            let noise_var = block_noise * mean_w / w.max(1e-6);
            llrs.extend(symbol_llrs(*sym, p.bits_per_sc, noise_var, &points));
        }
    }

    // Decode the majority-protected length prefix from the first 48 LLRs
    // (soft-combining the three copies) to recover the actual payload bit
    // count.  This lets us trim padding bits added by the last OFDM symbol
    // boundary so decoders that expect an exact codeword length (e.g. turbo)
    // don't see spurious bits.
    let Some(payload_len) = decode_len_prefix_llrs(&llrs) else {
        return Err(ModemError::Demodulation(
            "OFDM frame shorter than length prefix".into(),
        ));
    };
    // Skip the prefix LLRs and return exactly payload_len × 8 LLRs.
    let bit_llrs = &llrs[LEN_PREFIX_BITS..];
    let take = (payload_len as usize * 8).min(bit_llrs.len());
    Ok(bit_llrs[..take].to_vec())
}

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
