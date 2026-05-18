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

pub fn scfdma_demodulate(samples: &[f32], mode: &str) -> Vec<u8> {
    let p = params_for_mode(mode).expect("caller must validate mode before scfdma_demodulate");
    demodulate_with_params(samples, &p)
}

/// Demodulate SC-FDMA samples and return per-bit soft values (LLRs).
///
/// Positive values indicate bit 0 is more likely; negative values indicate bit 1.
pub fn scfdma_demodulate_soft(samples: &[f32], mode: &str) -> Vec<f32> {
    let p = params_for_mode(mode).expect("caller must validate mode before scfdma_demodulate_soft");
    demodulate_soft_with_params(samples, &p).llrs
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
    let p = params_for_mode(mode)
        .expect("caller must validate mode before scfdma_demodulate_soft_with_metrics");
    demodulate_soft_with_params(samples, &p)
}

/// Combine multiple LLR attempts using inverse-noise variance weighting.
fn demodulate_with_params(samples: &[f32], p: &ScFdmaParams) -> Vec<u8> {
    let sync = modulate_with_params(&preamble_payload(p), p);
    if samples.len() < sync.len() + SYM_LEN {
        return vec![];
    }

    let offset = find_sync_offset(samples, &sync);
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
        let h_est = dft_ce_estimate(p, &freq);
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

    let offset = find_sync_offset(samples, &sync);
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

    let fft_scale = 1.0 / (FFT_SIZE as f32).sqrt();
    let idft_scale = 1.0 / (p.n_data as f32).sqrt();

    let points = constellation_points(p.bits_per_sc);
    let mut llrs = Vec::with_capacity(n_syms * p.bits_per_symbol());
    let mut noise_sum = 0.0f32;
    let mut k_db_sum = 0.0f32;
    let mut metric_symbols = 0usize;

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

        let h_est = dft_ce_estimate(p, &freq);
        let pilot_noise_var = estimate_noise_var(p, &freq, &h_est).max(1e-6);

        // Rician K for SoftFrameMetrics: computed from raw pilot LS observations
        // so the estimator range is independent of the CE method used for equalization.
        let h_pilots: Vec<Complex32> = pilot_positions(p)
            .iter()
            .map(|&sc| freq[sc] / Complex32::new(PILOT_AMPLITUDE, 0.0))
            .collect();
        let k_linear = estimate_rician_k_linear(&h_pilots);
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

fn find_sync_offset(samples: &[f32], sync: &[f32]) -> usize {
    if samples.len() <= sync.len() {
        return 0;
    }

    let max_offset = samples.len() - sync.len();
    let mut best_offset = 0usize;
    let mut best_score = f32::NEG_INFINITY;

    // Search the full capture so a caller can prefix arbitrary leading noise.
    for offset in 0..=max_offset {
        let score: f32 = samples[offset..offset + sync.len()]
            .iter()
            .zip(sync.iter())
            .map(|(&a, &b)| a * b)
            .sum();
        if score > best_score {
            best_score = score;
            best_offset = offset;
        }
    }

    best_offset
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
