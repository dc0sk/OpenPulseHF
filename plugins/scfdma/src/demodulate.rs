//! SC-FDMA demodulation: samples → FFT → LS/MMSE equalize → IDFT → payload.

use num_complex::Complex32;
use rustfft::FftPlanner;

use crate::channel::{
    dft_ce_estimate, estimate_noise_var, estimate_rician_k_linear, mmse_equalize,
    mmse_llr_noise_var, pilot_positions,
};
use crate::modulate::{
    gray3_to_natural, gray5_to_natural, modulate_with_params, natural3_to_gray, natural5_to_gray,
    preamble_payload, QAM32_SCALE, QAM32_SPATIAL,
};
use crate::params::PILOT_AMPLITUDE;
use crate::params::{params_for_mode, ScFdmaParams, CP, FFT_SIZE, SYM_LEN};

// Re-export from the canonical core implementation so the plugin exposes the
// same public path without duplicating the logic.
pub use openpulse_core::fec::combine_llrs_weighted;
// Canonical shared implementation (openpulse-dsp); re-exported because the
// lib.rs acquisition regression test references it via this module's path.
#[cfg(test)]
pub(crate) use openpulse_dsp::acquisition::quadrature;
use openpulse_dsp::acquisition::IqMatchedFilter;

pub fn scfdma_demodulate(samples: &[f32], mode: &str) -> Vec<u8> {
    match params_for_mode(mode) {
        Some(p) => demodulate_with_params(samples, &p),
        None => vec![],
    }
}

/// Demodulate SC-FDMA samples and return per-bit soft values (LLRs).
///
/// Positive values indicate bit 0 is more likely; negative values indicate bit 1.
pub fn scfdma_demodulate_soft(samples: &[f32], mode: &str) -> Vec<f32> {
    match params_for_mode(mode) {
        Some(p) => demodulate_soft_with_params(samples, &p).llrs,
        None => vec![],
    }
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
pub fn scfdma_demodulate_soft_with_metrics(samples: &[f32], mode: &str) -> SoftDemodOutput {
    match params_for_mode(mode) {
        Some(p) => demodulate_soft_with_params(samples, &p),
        None => SoftDemodOutput {
            llrs: vec![],
            metrics: SoftFrameMetrics {
                mean_noise_var: 0.0,
                mean_rician_k_db: 0.0,
                symbols_used: 0,
            },
        },
    }
}

/// Combine multiple LLR attempts using inverse-noise variance weighting.
fn demodulate_with_params(samples: &[f32], p: &ScFdmaParams) -> Vec<u8> {
    let sync = modulate_with_params(&preamble_payload(p), p);
    if samples.len() < sync.len() + SYM_LEN {
        return vec![];
    }

    let Some(offset) = find_sync_offset(samples, &sync) else {
        return vec![];
    };
    let payload_start = offset + sync.len();
    if payload_start >= samples.len() {
        return vec![];
    }

    let samples = &samples[payload_start..];
    let n_syms = samples.len() / SYM_LEN;
    if n_syms == 0 {
        return vec![];
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

    // Strip 2-byte LE length prefix.
    if raw.len() < 2 {
        return raw;
    }
    let payload_len = u16::from_le_bytes([raw[0], raw[1]]) as usize;
    let available = raw.len() - 2;
    let take = payload_len.min(available);
    raw[2..2 + take].to_vec()
}

fn demodulate_soft_with_params(samples: &[f32], p: &ScFdmaParams) -> SoftDemodOutput {
    let sync = modulate_with_params(&preamble_payload(p), p);
    if samples.len() < sync.len() + SYM_LEN {
        return SoftDemodOutput {
            llrs: vec![],
            metrics: SoftFrameMetrics {
                mean_noise_var: 0.0,
                mean_rician_k_db: 0.0,
                symbols_used: 0,
            },
        };
    }

    let Some(offset) = find_sync_offset(samples, &sync) else {
        return SoftDemodOutput {
            llrs: vec![],
            metrics: SoftFrameMetrics {
                mean_noise_var: 0.0,
                mean_rician_k_db: 0.0,
                symbols_used: 0,
            },
        };
    };
    let payload_start = offset + sync.len();
    if payload_start >= samples.len() {
        return SoftDemodOutput {
            llrs: vec![],
            metrics: SoftFrameMetrics {
                mean_noise_var: 0.0,
                mean_rician_k_db: 0.0,
                symbols_used: 0,
            },
        };
    }

    let samples = &samples[payload_start..];
    let n_syms = samples.len() / SYM_LEN;
    if n_syms == 0 {
        return SoftDemodOutput {
            llrs: vec![],
            metrics: SoftFrameMetrics {
                mean_noise_var: 0.0,
                mean_rician_k_db: 0.0,
                symbols_used: 0,
            },
        };
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

    if llrs.len() < 16 {
        return SoftDemodOutput {
            llrs,
            metrics: SoftFrameMetrics {
                mean_noise_var: if metric_symbols > 0 {
                    noise_sum / metric_symbols as f32
                } else {
                    0.0
                },
                mean_rician_k_db: if metric_symbols > 0 {
                    k_db_sum / metric_symbols as f32
                } else {
                    0.0
                },
                symbols_used: metric_symbols,
            },
        };
    }

    let mut len_bytes = [0u8; 2];
    for (byte_idx, byte_out) in len_bytes.iter_mut().enumerate() {
        let mut v = 0u8;
        for bit in 0..8 {
            if llrs[byte_idx * 8 + bit].is_sign_negative() {
                v |= 1u8 << bit;
            }
        }
        *byte_out = v;
    }
    let payload_len = u16::from_le_bytes(len_bytes) as usize;
    let payload_bits = payload_len.saturating_mul(8);
    let available_payload_bits = llrs.len().saturating_sub(16);
    let take = if payload_bits == 0 && available_payload_bits > 0 {
        // A noisy length prefix can decode to zero under fading; in that case,
        // return all whole-byte payload bits so downstream soft combining still
        // has useful information.
        available_payload_bits - (available_payload_bits % 8)
    } else {
        payload_bits.min(available_payload_bits)
    };
    SoftDemodOutput {
        llrs: llrs[16..16 + take].to_vec(),
        metrics: SoftFrameMetrics {
            mean_noise_var: noise_sum / metric_symbols.max(1) as f32,
            mean_rician_k_db: k_db_sum / metric_symbols.max(1) as f32,
            symbols_used: metric_symbols,
        },
    }
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

// ── Constellation demappers ───────────────────────────────────────────────────

fn demap_symbol(c: Complex32, bits_per_sc: usize) -> u8 {
    match bits_per_sc {
        2 => qpsk_demod(c),
        3 => psk8_demod(c),
        4 => qam16_demod(c),
        5 => qam32_demod(c),
        6 => qam64_demod(c),
        _ => qpsk_demod(c),
    }
}

fn qpsk_demod(c: Complex32) -> u8 {
    let i_bit = if c.re >= 0.0 { 0u8 } else { 1u8 };
    let q_bit = if c.im >= 0.0 { 0u8 } else { 1u8 };
    i_bit | (q_bit << 1)
}

/// Hard-decision 16QAM demapper: nearest PAM-4 point on each axis.
fn qam16_demod(c: Complex32) -> u8 {
    pam4_slice(c.re) << 2 | pam4_slice(c.im)
}

/// Hard-decision 64QAM demapper: nearest PAM-8 point on each axis.
fn qam64_demod(c: Complex32) -> u8 {
    pam8_slice(c.re) << 3 | pam8_slice(c.im)
}

/// Nearest PAM-4 Gray code for a real amplitude (thresholds at 0 and ±2×scale).
fn pam4_slice(x: f32) -> u8 {
    const SCALE: f32 = 0.316_227_77; // 1/sqrt(10)
    const T1: f32 = 2.0 * SCALE; // threshold between ±1 and ±3 levels
    if x < -T1 {
        0b00 // −3
    } else if x < 0.0 {
        0b01 // −1
    } else if x < T1 {
        0b11 // +1
    } else {
        0b10 // +3
    }
}

/// Nearest PAM-8 Gray code for a real amplitude (thresholds at even multiples of scale).
fn pam8_slice(x: f32) -> u8 {
    const SCALE: f32 = 0.154_303_35; // 1/sqrt(42)
    const T1: f32 = 2.0 * SCALE;
    const T2: f32 = 4.0 * SCALE;
    const T3: f32 = 6.0 * SCALE;
    if x < -T3 {
        0b000 // −7
    } else if x < -T2 {
        0b001 // −5
    } else if x < -T1 {
        0b011 // −3
    } else if x < 0.0 {
        0b010 // −1
    } else if x < T1 {
        0b110 // +1
    } else if x < T2 {
        0b111 // +3
    } else if x < T3 {
        0b101 // +5
    } else {
        0b100 // +7
    }
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

fn symbol_llrs(
    symbol: Complex32,
    bits_per_sc: usize,
    noise_var: f32,
    points: &[(u8, Complex32)],
) -> Vec<f32> {
    let inv_noise = 1.0 / noise_var.max(1e-6);
    let mut out = Vec::with_capacity(bits_per_sc);

    for bit in 0..bits_per_sc {
        let mut min0 = f32::INFINITY;
        let mut min1 = f32::INFINITY;

        for (label, pt) in points {
            let d = (symbol - *pt).norm_sqr() * inv_noise;
            if (label >> bit) & 1 == 0 {
                if d < min0 {
                    min0 = d;
                }
            } else if d < min1 {
                min1 = d;
            }
        }

        out.push(min1 - min0);
    }

    out
}

fn constellation_points(bits_per_sc: usize) -> Vec<(u8, Complex32)> {
    match bits_per_sc {
        2 => (0u8..4).map(|b| (b, qpsk_point(b))).collect(),
        3 => (0u8..8).map(|b| (b, psk8_point(b))).collect(),
        4 => (0u8..16).map(|b| (b, qam16_point(b))).collect(),
        5 => (0u8..32).map(|b| (b, qam32_point(b))).collect(),
        6 => (0u8..64).map(|b| (b, qam64_point(b))).collect(),
        _ => (0u8..4).map(|b| (b, qpsk_point(b))).collect(),
    }
}

fn estimate_decision_noise_var(symbols: &[Complex32], bits_per_sc: usize) -> f32 {
    if symbols.is_empty() {
        return 1e-6;
    }
    let points = constellation_points(bits_per_sc);
    let sum_min_dist: f32 = symbols
        .iter()
        .map(|s| {
            points
                .iter()
                .map(|(_, pt)| (*s - *pt).norm_sqr())
                .fold(f32::INFINITY, f32::min)
        })
        .sum();
    (sum_min_dist / symbols.len() as f32).max(1e-6)
}

fn qpsk_point(bits: u8) -> Complex32 {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    match bits & 0x3 {
        0 => Complex32::new(s, s),
        1 => Complex32::new(-s, s),
        2 => Complex32::new(s, -s),
        _ => Complex32::new(-s, -s),
    }
}

fn qam16_point(bits: u8) -> Complex32 {
    const SCALE: f32 = 0.316_227_77;
    fn pam4(g: u8) -> f32 {
        match g & 0x3 {
            0b00 => -3.0,
            0b01 => -1.0,
            0b11 => 1.0,
            _ => 3.0,
        }
    }
    let i = pam4((bits >> 2) & 0x3) * SCALE;
    let q = pam4(bits & 0x3) * SCALE;
    Complex32::new(i, q)
}

fn psk8_point(bits: u8) -> Complex32 {
    let k = gray3_to_natural(bits);
    let angle = k as f32 * std::f32::consts::FRAC_PI_4;
    Complex32::new(angle.cos(), angle.sin())
}

fn psk8_demod(c: Complex32) -> u8 {
    use std::f32::consts::{FRAC_PI_4, TAU};
    let angle = c.im.atan2(c.re).rem_euclid(TAU);
    let k = ((angle / FRAC_PI_4) + 0.5).floor() as u8 % 8;
    natural3_to_gray(k)
}

fn qam32_point(bits: u8) -> Complex32 {
    let (i, q) = QAM32_SPATIAL[gray5_to_natural(bits) as usize];
    Complex32::new(i as f32 * QAM32_SCALE, q as f32 * QAM32_SCALE)
}

fn qam32_demod(c: Complex32) -> u8 {
    let mut best_idx = 0u8;
    let mut best_d = f32::INFINITY;
    for (idx, &(i, q)) in QAM32_SPATIAL.iter().enumerate() {
        let d = (c.re - i as f32 * QAM32_SCALE).powi(2) + (c.im - q as f32 * QAM32_SCALE).powi(2);
        if d < best_d {
            best_d = d;
            best_idx = idx as u8;
        }
    }
    natural5_to_gray(best_idx)
}

fn qam64_point(bits: u8) -> Complex32 {
    const SCALE: f32 = 0.154_303_35;
    fn pam8(g: u8) -> f32 {
        let raw: i8 = match g & 0x7 {
            0b000 => -7,
            0b001 => -5,
            0b011 => -3,
            0b010 => -1,
            0b110 => 1,
            0b111 => 3,
            0b101 => 5,
            _ => 7,
        };
        raw as f32 * SCALE
    }
    let i = pam8((bits >> 3) & 0x7);
    let q = pam8(bits & 0x7);
    Complex32::new(i, q)
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

    if all_llrs.len() < 16 {
        return Some(all_llrs);
    }

    // Strip the 2-byte LE length prefix from the LLR stream, mirroring the CPU path.
    let mut len_bytes = [0u8; 2];
    for (byte_idx, byte_out) in len_bytes.iter_mut().enumerate() {
        let mut v = 0u8;
        for bit in 0..8 {
            if all_llrs[byte_idx * 8 + bit].is_sign_negative() {
                v |= 1u8 << bit;
            }
        }
        *byte_out = v;
    }
    let payload_len = u16::from_le_bytes(len_bytes) as usize;
    let payload_bits = payload_len.saturating_mul(8);
    let available_payload_bits = all_llrs.len().saturating_sub(16);
    let take = if payload_bits == 0 && available_payload_bits > 0 {
        available_payload_bits - (available_payload_bits % 8)
    } else {
        payload_bits.min(available_payload_bits)
    };
    Some(all_llrs[16..16 + take].to_vec())
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

    if raw.len() < 2 {
        return Some(raw);
    }
    let payload_len = u16::from_le_bytes([raw[0], raw[1]]) as usize;
    let available = raw.len() - 2;
    let take = payload_len.min(available);
    Some(raw[2..2 + take].to_vec())
}
