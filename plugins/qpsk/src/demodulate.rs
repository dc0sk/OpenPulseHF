use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, PulseShape};
use openpulse_dsp::equalizer::LmsEqualizer;
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::pll::CarrierPll;
use openpulse_dsp::rrc::generate_rrc_coefficients;
use openpulse_dsp::timing::GardnerDetector;

use crate::modulate::{
    gray_map, preamble_symbols, samples_per_symbol, PREAMBLE_SYMS, RRC_SPAN_SYMBOLS, TAIL_SYMS,
};
use crate::parse_baud_rate;

pub fn qpsk_demodulate(samples: &[f32], config: &ModulationConfig) -> Result<Vec<u8>, ModemError> {
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
        qpsk_demodulate_rrc(samples, n, baud, fc, fs, alpha)
    } else {
        let timing = find_timing_offset(samples, n, fc, fs, cosine_overlap);
        demodulate_symbols(samples, n, fc, fs, timing, cosine_overlap)
    };

    if syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".to_string(),
        ));
    }

    let syms = qpsk_lms_equalize(&syms, &config.mode);

    let data = &syms[PREAMBLE_SYMS..(syms.len() - TAIL_SYMS)];
    let bits = symbols_to_bits(data);
    Ok(bits_to_bytes(&bits))
}

/// RRC demodulation: downmix → matched RRC filter → brute-force timing → sample.
fn qpsk_demodulate_rrc(
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

    // 3. Coarse timing acquisition via preamble correlation (brute-force, same as Hann path).
    let initial_timing = find_timing_offset_bb(&i_bb, n);

    // 4. Adaptive timing + carrier recovery.
    gardner_pll_sample_rrc(&i_bb, &q_bb, n, initial_timing)
}

/// Adaptive timing (Gardner) + carrier recovery (Costas PLL) for QPSK-RRC.
///
/// `initial_timing` seeds the Gardner loop from the brute-force preamble search.
/// The Costas PLL (psk_order=2) corrects residual carrier phase and frequency offset.
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
    let mut pll = CarrierPll::new(0.02, 2);
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

/// Brute-force timing search on the baseband I signal (after downmix + RRC).
fn find_timing_offset_bb(i_bb: &[f32], n: usize) -> usize {
    let expected = preamble_expected();
    let mut best_off = 0usize;
    let mut best_score = f32::NEG_INFINITY;

    for off in 0..n {
        if i_bb.len() < off + n * PREAMBLE_SYMS {
            break;
        }
        let score: f32 = (0..PREAMBLE_SYMS)
            .map(|s| i_bb[off + s * n] * expected[s].0) // correlate on I channel
            .sum();
        if score > best_score {
            best_score = score;
            best_off = off;
        }
    }
    best_off
}

fn find_timing_offset(samples: &[f32], n: usize, fc: f32, fs: f32, cosine_overlap: bool) -> usize {
    let expected = preamble_expected();
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
            // Matched filter: use sin²(πi/n) for CosineOverlap (signal peaks at centre);
            // use raised cosine for Hann overlap (signal peaks at leading edge).
            let window = if cosine_overlap {
                0.5 * (1.0 - (two_pi * i as f32 / n as f32).cos())
            } else {
                0.5 * (1.0 + (PI * i as f32 / n as f32).cos())
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
    let mut bits = Vec::with_capacity(symbols.len() * 2);
    for &(i, q) in symbols {
        let (b0, b1) = nearest_gray_bits(i, q);
        bits.push(b0);
        bits.push(b1);
    }
    bits
}

fn nearest_gray_bits(i: f32, q: f32) -> (bool, bool) {
    let candidates = [
        (gray_map(false, false), (false, false)),
        (gray_map(false, true), (false, true)),
        (gray_map(true, true), (true, true)),
        (gray_map(true, false), (true, false)),
    ];

    let mut best = (false, false);
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

fn gray_map_decision(i: f32, q: f32) -> (f32, f32) {
    let (b0, b1) = nearest_gray_bits(i, q);
    gray_map(b0, b1)
}

/// Apply an LMS equalizer to QPSK symbol-rate I/Q.
///
/// Trains on known preamble symbols, then switches to decision-directed mode.
fn lms_profile(mode: &str) -> (usize, usize, f32) {
    // HF 1000-baud paths see stronger multipath/ISI under Watterson Moderate/Poor,
    // so enable a short DFE section and slightly smaller step size for stability.
    if mode.ends_with("-HF") && mode.contains("1000") {
        (9, 2, 0.015)
    } else {
        (7, 0, 0.02)
    }
}

fn qpsk_lms_equalize(symbols: &[(f32, f32)], mode: &str) -> Vec<(f32, f32)> {
    if symbols.is_empty() {
        return Vec::new();
    }

    let train_len = PREAMBLE_SYMS.min(symbols.len());
    let expected = preamble_expected();
    let mut training_i = Vec::with_capacity(train_len);
    let mut training_q = Vec::with_capacity(train_len);
    for &(i, q) in expected.iter().take(train_len) {
        training_i.push(i);
        training_q.push(q);
    }

    // Split complex symbols in one pass to reduce hot-path iterator churn.
    let (i_syms, q_syms): (Vec<f32>, Vec<f32>) = symbols.iter().copied().unzip();

    let (fwd_len, dfe_len, mu) = lms_profile(mode);
    let mut eq = LmsEqualizer::new(fwd_len, dfe_len, mu);
    let (i_eq, q_eq) = eq.process_frame(&i_syms, &q_syms, &training_i, &training_q, |i, q| {
        gray_map_decision(i, q)
    });

    let mut out = Vec::with_capacity(i_eq.len().min(q_eq.len()));
    for (i, q) in i_eq.into_iter().zip(q_eq) {
        out.push((i, q));
    }
    out
}

fn bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |acc, (i, &b)| acc | ((b as u8) << i))
        })
        .collect()
}

/// Demodulate QPSK samples and return per-bit soft log-likelihood ratios.
///
/// Returns two `f32`s per symbol, **[q, i]**, matching the (b0, b1) bit order in
/// `symbols_to_bits`.  With the Gray mapping used by this plugin:
/// - Q projection → LLR for b0 (Q > 0 means b0 = 0)
/// - I projection → LLR for b1 (I > 0 means b1 = 0)
///
/// RRC modes fall back to hard ±1.0 pseudo-LLRs.
pub fn qpsk_demodulate_soft(
    samples: &[f32],
    config: &ModulationConfig,
) -> Result<Vec<f32>, ModemError> {
    if matches!(config.pulse_shape, PulseShape::Rrc { .. }) || config.mode.ends_with("-RRC") {
        let bytes = qpsk_demodulate(samples, config)?;
        return Ok(bytes
            .iter()
            .flat_map(|&b| (0..8u8).map(move |i| if (b >> i) & 1 == 0 { 1.0f32 } else { -1.0f32 }))
            .collect());
    }

    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;
    let cosine_overlap =
        config.pulse_shape == PulseShape::CosineOverlap || config.mode.ends_with("-HF");

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return Err(ModemError::Demodulation("signal too short".to_string()));
    }

    let timing = find_timing_offset(samples, n, fc, fs, cosine_overlap);
    let syms = demodulate_symbols(samples, n, fc, fs, timing, cosine_overlap);

    if syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".to_string(),
        ));
    }

    let syms = qpsk_lms_equalize(&syms, &config.mode);

    let data = &syms[PREAMBLE_SYMS..(syms.len() - TAIL_SYMS)];
    // Per symbol: b0 LLR = Q, b1 LLR = I (from the Gray map geometry).
    // Bits are pushed as (b0, b1) in symbols_to_bits, matching [q, i] here.
    let llrs = data.iter().flat_map(|&(i, q)| [q, i]).collect();
    Ok(llrs)
}

fn preamble_expected() -> Vec<(f32, f32)> {
    preamble_symbols()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_core::plugin::ModulationConfig;

    #[test]
    fn qpsk_round_trip() {
        let cfg = ModulationConfig {
            mode: "QPSK250".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"OpenPulse QPSK";
        let samples = crate::modulate::qpsk_modulate(payload, &cfg).expect("modulate");
        let recovered = qpsk_demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }

    #[test]
    fn lms_equalizer_preserves_symbol_count() {
        let syms = vec![(1.0, 1.0); PREAMBLE_SYMS + 8];
        let eq = qpsk_lms_equalize(&syms, "QPSK1000");
        assert_eq!(eq.len(), syms.len());
    }

    #[test]
    fn lms_profile_hf_uses_dfe() {
        let (fwd, dfe, mu) = lms_profile("QPSK1000-HF");
        assert_eq!(fwd, 9);
        assert_eq!(dfe, 2);
        assert!((mu - 0.015).abs() < 1e-6);
    }

    #[test]
    fn lms_profile_default_matches_baseline() {
        let (fwd, dfe, mu) = lms_profile("QPSK500");
        assert_eq!(fwd, 7);
        assert_eq!(dfe, 0);
        assert!((mu - 0.02).abs() < 1e-6);
    }
}
