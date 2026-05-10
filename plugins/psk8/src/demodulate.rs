use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, PulseShape};
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::pll::CarrierPll;
use openpulse_dsp::rrc::generate_rrc_coefficients;
use openpulse_dsp::timing::GardnerDetector;

use crate::modulate::{
    gray_map_8psk, preamble_symbols, samples_per_symbol, PREAMBLE_SYMS, RRC_SPAN_SYMBOLS, TAIL_SYMS,
};
use crate::parse_baud_rate;

pub fn psk8_demodulate(samples: &[f32], config: &ModulationConfig) -> Result<Vec<u8>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;
    let cosine_overlap =
        config.pulse_shape == PulseShape::CosineOverlap || config.mode.ends_with("-HF");
    let rrc_alpha = if let PulseShape::Rrc { alpha } = config.pulse_shape {
        Some(alpha)
    } else if config.mode.ends_with("-RRC") {
        Some(0.35f32)
    } else {
        None
    };

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return Err(ModemError::Demodulation("signal too short".to_string()));
    }

    // For RRC: downmix to baseband I/Q then apply the matched low-pass RRC
    // filter; applying the baseband RRC directly to the passband signal would
    // place fc outside the filter passband and attenuate the signal to ~0.
    let syms = if let Some(alpha) = rrc_alpha {
        psk8_demodulate_rrc(samples, n, baud, fc, fs, alpha)
    } else {
        let timing = find_timing_offset(samples, n, fc, fs, cosine_overlap);
        demodulate_symbols(samples, n, fc, fs, timing, cosine_overlap)
    };

    if syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".to_string(),
        ));
    }

    let data = &syms[PREAMBLE_SYMS..(syms.len() - TAIL_SYMS)];
    let bits = symbols_to_bits(data);
    Ok(bits_to_bytes(&bits))
}

/// RRC demodulation: downmix → matched RRC filter → brute-force timing → sample.
fn psk8_demodulate_rrc(
    samples: &[f32],
    n: usize,
    baud: f32,
    fc: f32,
    fs: f32,
    alpha: f32,
) -> Vec<(f32, f32)> {
    let two_pi = 2.0 * PI;
    let num_taps = RRC_SPAN_SYMBOLS * n + 1;
    let coeffs = generate_rrc_coefficients(fs, baud, alpha, num_taps);
    let group_delay = (num_taps - 1) / 2;

    // 1. Downmix to baseband I and Q.
    let i_mix: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(k, &s)| s * (two_pi * fc * k as f32 / fs).cos() * 2.0)
        .collect();
    let q_mix: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(k, &s)| -s * (two_pi * fc * k as f32 / fs).sin() * 2.0)
        .collect();

    // 2. Apply RRC matched filter with group delay compensation.
    let rrc_filter = |mix: Vec<f32>| -> Vec<f32> {
        let padded: Vec<f32> = mix
            .iter()
            .copied()
            .chain(std::iter::repeat_n(0.0, group_delay))
            .collect();
        let mut fir = FirFilter::new(coeffs.clone());
        let filtered = fir.apply(&padded);
        filtered[group_delay..].to_vec()
    };
    let i_bb = rrc_filter(i_mix);
    let q_bb = rrc_filter(q_mix);

    // 3. Coarse timing acquisition via IQ preamble correlation (brute-force).
    let initial_timing = find_timing_offset_bb_iq(&i_bb, &q_bb, n);

    // 4. Adaptive timing + carrier recovery.
    gardner_pll_sample_rrc(&i_bb, &q_bb, n, initial_timing)
}

/// Adaptive timing (Gardner) + carrier recovery (Costas PLL) for 8PSK-RRC.
///
/// `initial_timing` seeds the Gardner loop from the IQ preamble correlation.
/// The Costas PLL (psk_order=3) corrects residual carrier phase and frequency offset.
fn gardner_pll_sample_rrc(
    i_bb: &[f32],
    q_bb: &[f32],
    n: usize,
    initial_timing: usize,
) -> Vec<(f32, f32)> {
    let start = initial_timing.min(i_bb.len());
    let mut det = GardnerDetector::new(n, 0.02);
    // Pre-arm so the first sample at `start` (already an ISI-free point) is output immediately.
    det.pre_arm();
    let mut pll = CarrierPll::new(0.02, 3);
    let mut syms = Vec::new();
    for (idx, &s_i) in i_bb[start..].iter().enumerate() {
        if det.update(s_i).is_some() {
            let s_q = q_bb.get(start + idx).copied().unwrap_or(0.0);
            pll.update(s_i, s_q);
            syms.push(pll.correct(s_i, s_q));
        }
    }
    syms
}

/// Brute-force timing search using both I and Q baseband channels.
///
/// For 8PSK, four of the 16 preamble symbols have I=0, so an I-only
/// correlation misses half the signal energy.  Full IQ correlation gives a
/// sharper peak at the true ISI-free timing offset.
fn find_timing_offset_bb_iq(i_bb: &[f32], q_bb: &[f32], n: usize) -> usize {
    let expected = preamble_symbols();
    let mut best_off = 0usize;
    let mut best_score = f32::NEG_INFINITY;

    for off in 0..n {
        if i_bb.len() < off + n * PREAMBLE_SYMS {
            break;
        }
        let score: f32 = (0..PREAMBLE_SYMS)
            .map(|s| i_bb[off + s * n] * expected[s].0 + q_bb[off + s * n] * expected[s].1)
            .sum();
        if score > best_score {
            best_score = score;
            best_off = off;
        }
    }
    best_off
}

fn find_timing_offset(samples: &[f32], n: usize, fc: f32, fs: f32, cosine_overlap: bool) -> usize {
    let expected = preamble_symbols();
    let mut best_off = 0usize;
    let mut best_score = f32::NEG_INFINITY;

    for off in 0..n {
        if samples.len() <= off + n * PREAMBLE_SYMS {
            break;
        }
        let syms = demodulate_symbols(samples, n, fc, fs, off, cosine_overlap);
        if syms.len() < PREAMBLE_SYMS {
            continue;
        }
        let score: f32 = syms
            .iter()
            .zip(expected.iter())
            .take(PREAMBLE_SYMS)
            .map(|(&(i, q), &(ei, eq))| i * ei + q * eq)
            .sum();
        if score > best_score {
            best_score = score;
            best_off = off;
        }
    }

    best_off
}

fn demodulate_symbols(
    samples: &[f32],
    n: usize,
    fc: f32,
    fs: f32,
    offset: usize,
    cosine_overlap: bool,
) -> Vec<(f32, f32)> {
    let two_pi = 2.0 * PI;
    let aligned = &samples[offset.min(samples.len())..];
    let n_syms = aligned.len() / n;
    let mut out = Vec::with_capacity(n_syms);

    for sym_idx in 0..n_syms {
        let start = sym_idx * n;
        let mut i_acc = 0.0f32;
        let mut q_acc = 0.0f32;
        let mut norm = 0.0f32;

        for i in 0..n {
            let g = (offset + start + i) as f32;
            let sample = aligned[start + i];
            // Matched filter: sin²(πi/n) for CosineOverlap; squared raised cosine for Hann overlap.
            let window = if cosine_overlap {
                0.5 * (1.0 - (two_pi * i as f32 / n as f32).cos())
            } else {
                let w = 0.5 * (1.0 + (PI * i as f32 / n as f32).cos());
                w * w
            };
            let t = g / fs;
            let c = (two_pi * fc * t).cos();
            let s = (two_pi * fc * t).sin();

            i_acc += sample * c * window * 2.0;
            q_acc += -sample * s * window * 2.0;
            norm += window * window;
        }

        if norm > 1e-9 {
            i_acc /= norm;
            q_acc /= norm;
        }

        out.push((i_acc, q_acc));
    }

    out
}

fn symbols_to_bits(symbols: &[(f32, f32)]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(symbols.len() * 3);
    for &(i, q) in symbols {
        let (b0, b1, b2) = nearest_gray_triplet(i, q);
        bits.push(b0);
        bits.push(b1);
        bits.push(b2);
    }
    bits
}

type Candidate = ((f32, f32), (bool, bool, bool));

fn nearest_gray_triplet(i: f32, q: f32) -> (bool, bool, bool) {
    let candidates: [Candidate; 8] = [
        (gray_map_8psk(false, false, false), (false, false, false)),
        (gray_map_8psk(false, false, true), (false, false, true)),
        (gray_map_8psk(false, true, true), (false, true, true)),
        (gray_map_8psk(false, true, false), (false, true, false)),
        (gray_map_8psk(true, true, false), (true, true, false)),
        (gray_map_8psk(true, true, true), (true, true, true)),
        (gray_map_8psk(true, false, true), (true, false, true)),
        (gray_map_8psk(true, false, false), (true, false, false)),
    ];

    let mut best = (false, false, false);
    let mut best_dist = f32::INFINITY;
    for &((ci, cq), bits) in &candidates {
        let di = i - ci;
        let dq = q - cq;
        let dist = di * di + dq * dq;
        if dist < best_dist {
            best_dist = dist;
            best = bits;
        }
    }
    best
}

fn bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    // Drop partial final chunk: 8PSK packs 3 bits/symbol, so decoded bit count
    // may exceed 8*n_bytes by 1–2 bits. The partial chunk is pure padding.
    bits.chunks(8)
        .filter(|c| c.len() == 8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |acc, (i, &b)| acc | ((b as u8) << i))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_core::plugin::ModulationConfig;

    #[test]
    fn psk8_round_trip_500() {
        let cfg = ModulationConfig {
            mode: "8PSK500".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"OpenPulse 8PSK";
        let samples = crate::modulate::psk8_modulate(payload, &cfg).expect("modulate");
        let recovered = psk8_demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(recovered, payload);
    }

    /// Regression: 2048-byte payload (8*2048=16384 bits, 16384 % 3 != 0) must decode
    /// to exactly 2048 bytes, not 2049 (no spurious zero byte from padding tribit).
    #[test]
    fn psk8_1000rrc_round_trip_2048b() {
        let cfg = ModulationConfig {
            mode: "8PSK1000-RRC".to_string(),
            ..ModulationConfig::default()
        };
        let payload: Vec<u8> = (0u8..=255).cycle().take(2048).collect();
        let samples = crate::modulate::psk8_modulate(&payload, &cfg).expect("modulate");
        let recovered = psk8_demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(recovered.len(), payload.len(), "length must be exact");
        assert_eq!(recovered, payload);
    }
}
