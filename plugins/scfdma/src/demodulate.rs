//! SC-FDMA demodulation: samples → FFT → LS/MMSE equalize → IDFT → payload.

use num_complex::Complex32;
use rustfft::FftPlanner;

use crate::channel::{estimate_noise_var, ls_estimate, mmse_equalize};
use crate::modulate::{modulate_with_params, preamble_payload};
use crate::params::{params_for_mode, ScFdmaParams, CP, FFT_SIZE, SYM_LEN};

pub fn scfdma_demodulate(samples: &[f32], mode: &str) -> Vec<u8> {
    let p = params_for_mode(mode).expect("caller must validate mode before scfdma_demodulate");
    demodulate_with_params(samples, &p)
}

/// Demodulate SC-FDMA samples and return per-bit soft values (LLRs).
///
/// Positive values indicate bit 0 is more likely; negative values indicate bit 1.
pub fn scfdma_demodulate_soft(samples: &[f32], mode: &str) -> Vec<f32> {
    let p = params_for_mode(mode).expect("caller must validate mode before scfdma_demodulate_soft");
    demodulate_soft_with_params(samples, &p)
}

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

        // Step 2: LS channel estimation + MMSE equalization.
        let h_est = ls_estimate(p, &freq);
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

fn demodulate_soft_with_params(samples: &[f32], p: &ScFdmaParams) -> Vec<f32> {
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
    let idft = planner.plan_fft_inverse(p.n_data);

    let fft_scale = 1.0 / (FFT_SIZE as f32).sqrt();
    let idft_scale = 1.0 / (p.n_data as f32).sqrt();

    let points = constellation_points(p.bits_per_sc);
    let mut llrs = Vec::with_capacity(n_syms * p.bits_per_symbol());

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

        let h_est = ls_estimate(p, &freq);
        let noise_var = estimate_noise_var(p, &freq, &h_est).max(1e-6);
        let mut equalized = mmse_equalize(p, &freq, &h_est, noise_var);

        idft.process(&mut equalized);
        let data_syms: Vec<Complex32> = equalized.iter().map(|c| c * idft_scale).collect();

        for sym in &data_syms {
            llrs.extend(symbol_llrs(*sym, p.bits_per_sc, noise_var, &points));
        }
    }

    if llrs.len() < 16 {
        return llrs;
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
    llrs[16..16 + take].to_vec()
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
        4 => qam16_demod(c),
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
        4 => (0u8..16).map(|b| (b, qam16_point(b))).collect(),
        6 => (0u8..64).map(|b| (b, qam64_point(b))).collect(),
        _ => (0u8..4).map(|b| (b, qpsk_point(b))).collect(),
    }
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
