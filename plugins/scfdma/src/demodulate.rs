//! SC-FDMA demodulation: samples → FFT → LS/MMSE equalize → IDFT → payload.

use num_complex::Complex32;
use rustfft::FftPlanner;

use crate::channel::{estimate_noise_var, ls_estimate, mmse_equalize};
use crate::params::{params_for_mode, ScFdmaParams, CP, FFT_SIZE, SYM_LEN};

pub fn scfdma_demodulate(samples: &[f32], mode: &str) -> Vec<u8> {
    let p = params_for_mode(mode).expect("caller must validate mode before scfdma_demodulate");
    demodulate_with_params(samples, &p)
}

fn demodulate_with_params(samples: &[f32], p: &ScFdmaParams) -> Vec<u8> {
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
