//! OFDM demodulation: samples → FFT frames → LS/ZF equalize → payload.

use num_complex::Complex32;
use rustfft::FftPlanner;

use crate::channel::{is_pilot, ls_estimate, zf_equalize};
use crate::params::{params_for_mode, OfdmParams, CP, FFT_SIZE, SYM_LEN};

pub fn ofdm_demodulate(samples: &[f32], mode: &str) -> Vec<u8> {
    match params_for_mode(mode) {
        Some(p) => demodulate_with_params(samples, &p),
        None => vec![],
    }
}

/// Demodulate OFDM samples and return per-bit soft LLRs.
///
/// After ZF equalization each subcarrier carries a QPSK symbol.  The LLR for
/// each of its two bits is the signed projection onto the decision axis:
///
/// - bit 0 → `sym.re`  (positive = I > 0 = bit more likely 0)
/// - bit 1 → `sym.im`  (positive = Q > 0 = bit more likely 0)
///
/// **LLR sign convention**: positive = bit more likely 0, matching all other
/// plugins and codecs in this codebase.
///
/// The 2-byte LE length prefix inserted by `ofdm_modulate` is consumed and
/// excluded from the output.
pub fn ofdm_demodulate_soft(samples: &[f32], mode: &str) -> Vec<f32> {
    match params_for_mode(mode) {
        Some(p) => demodulate_soft_with_params(samples, &p),
        None => vec![],
    }
}

fn demodulate_with_params(samples: &[f32], p: &OfdmParams) -> Vec<u8> {
    let n_syms = samples.len() / SYM_LEN;
    if n_syms == 0 {
        return vec![];
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let scale = 1.0 / (FFT_SIZE as f32).sqrt();

    let mut bits: Vec<bool> = Vec::with_capacity(n_syms * p.bits_per_symbol());

    for sym_idx in 0..n_syms {
        let start = sym_idx * SYM_LEN + CP; // strip cyclic prefix
        if start + FFT_SIZE > samples.len() {
            break;
        }

        let mut freq: Vec<Complex32> = samples[start..start + FFT_SIZE]
            .iter()
            .map(|&s| Complex32::new(s * scale, 0.0))
            .collect();
        fft.process(&mut freq);

        // LS channel estimation + ZF equalization.
        let h_est = ls_estimate(p, &freq);
        let data_syms = zf_equalize(p, &freq, &h_est);

        // Decode QPSK symbols.
        for sym in &data_syms {
            let bits2 = qpsk_demod(*sym);
            bits.push(bits2 & 1 == 1);
            bits.push((bits2 >> 1) & 1 == 1);
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

// ── QPSK demapping ────────────────────────────────────────────────────────────

fn qpsk_demod(c: Complex32) -> u8 {
    let i_bit = if c.re >= 0.0 { 0u8 } else { 1u8 };
    let q_bit = if c.im >= 0.0 { 0u8 } else { 1u8 };
    i_bit | (q_bit << 1)
}

fn qpsk_llr(c: Complex32, weight: f32) -> [f32; 2] {
    // Post-ZF noise variance scales as σ²/|H|², so the matched-filter LLR is
    // proportional to symbol × |H|². Weighting by |H|² suppresses confidence on
    // faded subcarriers and matches the noise statistics seen by the FEC decoder.
    [c.re * weight, c.im * weight]
}

fn demodulate_soft_with_params(samples: &[f32], p: &OfdmParams) -> Vec<f32> {
    let n_syms = samples.len() / SYM_LEN;
    if n_syms == 0 {
        return vec![];
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let scale = 1.0 / (FFT_SIZE as f32).sqrt();

    // bits_per_symbol() = 2 for QPSK; each symbol → 2 LLRs.
    let mut llrs: Vec<f32> = Vec::with_capacity(n_syms * p.bits_per_symbol());

    for sym_idx in 0..n_syms {
        let start = sym_idx * SYM_LEN + CP;
        if start + FFT_SIZE > samples.len() {
            break;
        }

        let mut freq: Vec<Complex32> = samples[start..start + FFT_SIZE]
            .iter()
            .map(|&s| Complex32::new(s * scale, 0.0))
            .collect();
        fft.process(&mut freq);

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

        for (sym, &w) in data_syms.iter().zip(weights.iter()) {
            let [l0, l1] = qpsk_llr(*sym, w);
            llrs.push(l0);
            llrs.push(l1);
        }
    }

    // Hard-decode the 2-byte LE length prefix from the first 16 LLRs to recover the
    // actual payload bit count.  This lets us trim padding bits added by the last
    // OFDM symbol boundary so decoders that expect an exact codeword length (e.g.
    // turbo) don't see spurious bits.
    if llrs.len() < 16 {
        return vec![];
    }
    let b0 = (0..8u8).fold(0u8, |a, i| a | (((llrs[i as usize] < 0.0) as u8) << i));
    let b1 = (0..8u8).fold(0u8, |a, i| a | (((llrs[8 + i as usize] < 0.0) as u8) << i));
    let payload_len = u16::from_le_bytes([b0, b1]) as usize;
    // Skip the 16-LLR prefix and return exactly payload_len * 8 LLRs.
    let bit_llrs = &llrs[16..];
    let take = (payload_len * 8).min(bit_llrs.len());
    bit_llrs[..take].to_vec()
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
