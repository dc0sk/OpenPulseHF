use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, PulseShape};
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::rrc::generate_rrc_coefficients;

use crate::parse_baud_rate;

pub const PREAMBLE_SYMS: usize = 16;
pub const TAIL_SYMS: usize = 8;

const INV_SQRT_2: f32 = 0.70710677;

/// RRC FIR filter span in symbols. 12 (not 8) drops the residual-ISI floor from ~-36 to ~-50 dB —
/// it matters for the dense RRC rungs whose tight constellations are ISI-floor-limited. Both ends use
/// this same constant, so mod and demod stay matched.
pub(crate) const RRC_SPAN_SYMBOLS: usize = 12;

pub fn qpsk_modulate(data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    let cosine_overlap =
        config.pulse_shape == PulseShape::CosineOverlap || config.mode.ends_with("-HF");
    let rrc_alpha = if let PulseShape::Rrc { alpha } = config.pulse_shape {
        Some(alpha)
    } else if config.mode.ends_with("-RRC") {
        Some(0.35)
    } else {
        None
    };

    let mut symbols = preamble_symbols();
    symbols.extend(bits_to_symbols(&bytes_to_bits(data)));
    symbols.extend(std::iter::repeat_n((INV_SQRT_2, INV_SQRT_2), TAIL_SYMS));

    let total = symbols.len() * n;
    let mut out = vec![0.0f32; total];
    // For RRC: keep separate baseband I and Q impulse streams.
    let mut bb_i = if rrc_alpha.is_some() {
        vec![0.0f32; total]
    } else {
        vec![]
    };
    let mut bb_q = if rrc_alpha.is_some() {
        vec![0.0f32; total]
    } else {
        vec![]
    };
    let two_pi = 2.0 * PI;

    for (sym_idx, &(i_amp, q_amp)) in symbols.iter().enumerate() {
        let sym_start = sym_idx * n;
        if rrc_alpha.is_some() {
            // RRC path: baseband impulse at symbol start; carrier applied after
            // RRC filtering below.
            bb_i[sym_start] = i_amp;
            bb_q[sym_start] = q_amp;
        } else if cosine_overlap {
            for i in 0..n {
                // sin²(πi/n): 0 at boundaries, peaks at 1 at midpoint.
                let amp = 0.5 * (1.0 - (2.0 * PI * i as f32 / n as f32).cos());
                let t = (sym_start + i) as f32 / fs;
                let c = (two_pi * fc * t).cos();
                let s = (two_pi * fc * t).sin();
                out[sym_start + i] = (i_amp * c - q_amp * s) * amp;
            }
        } else {
            let (i_next, q_next) = symbols.get(sym_idx + 1).copied().unwrap_or((0.0, 0.0));
            for i in 0..n {
                let w_tail = 0.5 * (1.0 + (PI * i as f32 / n as f32).cos());
                let w_head = 1.0 - w_tail;
                let t = (sym_start + i) as f32 / fs;
                let c = (two_pi * fc * t).cos();
                let s = (two_pi * fc * t).sin();
                let env_i = i_amp * w_tail + i_next * w_head;
                let env_q = q_amp * w_tail + q_next * w_head;
                out[sym_start + i] = env_i * c - env_q * s;
            }
        }
    }

    // Apply RRC TX filter if requested (operates on baseband), then upconvert.
    if let Some(alpha) = rrc_alpha {
        let num_taps = RRC_SPAN_SYMBOLS * n + 1;
        let coeffs = generate_rrc_coefficients(fs, baud, alpha, num_taps);
        let group_delay = (num_taps - 1) / 2;

        let filter_bb = |bb: Vec<f32>| -> Vec<f32> {
            let padded: Vec<f32> = bb
                .iter()
                .copied()
                .chain(std::iter::repeat_n(0.0, group_delay))
                .collect();
            let mut fir = FirFilter::new(coeffs.clone());
            let filtered = fir.apply(&padded);
            filtered[group_delay..].to_vec()
        };

        let i_filt = filter_bb(bb_i);
        let q_filt = filter_bb(bb_q);

        // Upconvert shaped baseband I/Q to bandpass.
        out = i_filt
            .iter()
            .zip(q_filt.iter())
            .enumerate()
            .map(|(k, (&bi, &bq))| {
                let t = k as f32 / fs;
                let c = (two_pi * fc * t).cos();
                let s = (two_pi * fc * t).sin();
                bi * c - bq * s
            })
            .collect();
    }

    Ok(out)
}

/// Apply RRC FIR on bb_i/bb_q via wgpu, then upconvert to bandpass.
///
/// Falls back to CPU path if the GPU returns `None`.
#[cfg(feature = "gpu")]
pub fn qpsk_modulate_rrc_gpu(
    data: &[u8],
    config: &ModulationConfig,
    ctx: &std::sync::Arc<openpulse_gpu::GpuContext>,
) -> Result<Vec<f32>, ModemError> {
    use openpulse_gpu::gpu_rrc_fir;

    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    // Only handle RRC modes; fall through to CPU for non-RRC.
    let alpha = if let PulseShape::Rrc { alpha } = config.pulse_shape {
        alpha
    } else if config.mode.ends_with("-RRC") {
        0.35f32
    } else {
        return qpsk_modulate(data, config);
    };

    let mut symbols = preamble_symbols();
    symbols.extend(bits_to_symbols(&bytes_to_bits(data)));
    symbols.extend(std::iter::repeat_n((INV_SQRT_2, INV_SQRT_2), TAIL_SYMS));

    let total = symbols.len() * n;
    let mut bb_i = vec![0.0f32; total];
    let mut bb_q = vec![0.0f32; total];

    for (sym_idx, &(i_amp, q_amp)) in symbols.iter().enumerate() {
        let sym_start = sym_idx * n;
        bb_i[sym_start] = i_amp;
        bb_q[sym_start] = q_amp;
    }

    let num_taps = RRC_SPAN_SYMBOLS * n + 1;
    let coeffs = generate_rrc_coefficients(fs, baud, alpha, num_taps);
    let group_delay = (num_taps - 1) / 2;

    let gpu_filter = |bb: &[f32]| -> Option<Vec<f32>> {
        let padded: Vec<f32> = bb
            .iter()
            .copied()
            .chain(std::iter::repeat_n(0.0, group_delay))
            .collect();
        let filtered = gpu_rrc_fir(ctx, &padded, &coeffs)?;
        Some(filtered[group_delay..].to_vec())
    };

    let (i_filt, q_filt) = match (gpu_filter(&bb_i), gpu_filter(&bb_q)) {
        (Some(i), Some(q)) => (i, q),
        _ => {
            // GPU unavailable; complete via CPU fallback.
            let cpu_filter = |bb: Vec<f32>| -> Vec<f32> {
                let padded: Vec<f32> = bb
                    .iter()
                    .copied()
                    .chain(std::iter::repeat_n(0.0, group_delay))
                    .collect();
                let mut fir = FirFilter::new(coeffs.clone());
                let filtered = fir.apply(&padded);
                filtered[group_delay..].to_vec()
            };
            (cpu_filter(bb_i), cpu_filter(bb_q))
        }
    };

    let two_pi = 2.0 * PI;
    let out = i_filt
        .iter()
        .zip(q_filt.iter())
        .enumerate()
        .map(|(k, (&bi, &bq))| {
            let t = k as f32 / fs;
            let c = (two_pi * fc * t).cos();
            let s = (two_pi * fc * t).sin();
            bi * c - bq * s
        })
        .collect();

    Ok(out)
}

/// Encode `data` bytes as QPSK baseband I and Q sample vectors.
///
/// Returns `(i_bb, q_bb)` without carrier upconversion; suitable for direct
/// SDR I/Q streaming or stereo audio output.
pub fn qpsk_modulate_iq(
    data: &[u8],
    config: &ModulationConfig,
) -> Result<(Vec<f32>, Vec<f32>), ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let n = samples_per_symbol(fs, baud)?;

    let cosine_overlap =
        config.pulse_shape == PulseShape::CosineOverlap || config.mode.ends_with("-HF");
    let rrc_alpha = if let PulseShape::Rrc { alpha } = config.pulse_shape {
        Some(alpha)
    } else if config.mode.ends_with("-RRC") {
        Some(0.35)
    } else {
        None
    };

    let mut symbols = preamble_symbols();
    symbols.extend(bits_to_symbols(&bytes_to_bits(data)));
    symbols.extend(std::iter::repeat_n((INV_SQRT_2, INV_SQRT_2), TAIL_SYMS));

    let total = symbols.len() * n;
    let mut bb_i = vec![0.0f32; total];
    let mut bb_q = vec![0.0f32; total];

    for (sym_idx, &(i_amp, q_amp)) in symbols.iter().enumerate() {
        let sym_start = sym_idx * n;
        if rrc_alpha.is_some() {
            // Impulse at symbol start; RRC filter provides pulse shaping below.
            bb_i[sym_start] = i_amp;
            bb_q[sym_start] = q_amp;
        } else if cosine_overlap {
            for i in 0..n {
                let amp = 0.5 * (1.0 - (2.0 * PI * i as f32 / n as f32).cos());
                bb_i[sym_start + i] = i_amp * amp;
                bb_q[sym_start + i] = q_amp * amp;
            }
        } else {
            let (i_next, q_next) = symbols.get(sym_idx + 1).copied().unwrap_or((0.0, 0.0));
            for i in 0..n {
                let w_tail = 0.5 * (1.0 + (PI * i as f32 / n as f32).cos());
                let w_head = 1.0 - w_tail;
                bb_i[sym_start + i] = i_amp * w_tail + i_next * w_head;
                bb_q[sym_start + i] = q_amp * w_tail + q_next * w_head;
            }
        }
    }

    if let Some(alpha) = rrc_alpha {
        let num_taps = RRC_SPAN_SYMBOLS * n + 1;
        let coeffs = generate_rrc_coefficients(fs, baud, alpha, num_taps);
        let group_delay = (num_taps - 1) / 2;

        let filter_bb = |bb: Vec<f32>| -> Vec<f32> {
            let padded: Vec<f32> = bb
                .iter()
                .copied()
                .chain(std::iter::repeat_n(0.0, group_delay))
                .collect();
            let mut fir = FirFilter::new(coeffs.clone());
            let filtered = fir.apply(&padded);
            filtered[group_delay..].to_vec()
        };

        bb_i = filter_bb(bb_i);
        bb_q = filter_bb(bb_q);
    }

    Ok((bb_i, bb_q))
}

pub(crate) fn bytes_to_bits(bytes: &[u8]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for shift in 0..8u8 {
            bits.push((b >> shift) & 1 == 1);
        }
    }
    bits
}

pub(crate) fn bits_to_symbols(bits: &[bool]) -> Vec<(f32, f32)> {
    let mut syms = Vec::with_capacity(bits.len().div_ceil(2));
    for pair in bits.chunks(2) {
        let b0 = pair.first().copied().unwrap_or(false);
        let b1 = pair.get(1).copied().unwrap_or(false);
        syms.push(gray_map(b0, b1));
    }
    syms
}

pub(crate) fn gray_map(b0: bool, b1: bool) -> (f32, f32) {
    // Gray mapping: 00->45deg, 01->135deg, 11->225deg, 10->315deg
    match (b0, b1) {
        (false, false) => (INV_SQRT_2, INV_SQRT_2),
        (false, true) => (-INV_SQRT_2, INV_SQRT_2),
        (true, true) => (-INV_SQRT_2, -INV_SQRT_2),
        (true, false) => (INV_SQRT_2, -INV_SQRT_2),
    }
}

pub(crate) fn samples_per_symbol(sample_rate: f32, baud: f32) -> Result<usize, ModemError> {
    let n = (sample_rate / baud).round() as usize;
    if n < 4 {
        return Err(ModemError::Configuration(format!(
            "sample rate {sample_rate} Hz is too low for {baud} baud (need at least 4 samples/symbol)"
        )));
    }
    Ok(n)
}

pub(crate) fn preamble_symbols() -> Vec<(f32, f32)> {
    // Designed sequence: [45°,135°,225°,315°,225°,135°,45°,315°,225°,135°,45°,135°,225°,315°,45°,315°]
    //
    // Three properties are required simultaneously:
    //
    // 1. Timing discriminability: the cyclic 4-phase pattern had a constant +90° step
    //    between every pair, so the 1-lag autocorrelation R₁ = Σ e_{k+1}·conj(e_k) ≈ 16j.
    //    Crossfade ISI then made the squared-complex correlation flat across ALL timing
    //    offsets — the correct d=0 was indistinguishable from d=n-1.  This sequence has
    //    R₁ = -j (minimum magnitude 1), so the d=n-1 sidelobe is negligible vs the
    //    N²=256 mainlobe.
    //
    // 2. carrier_phase_correct drift accuracy: ISI introduces per-symbol phase biases
    //    bias_k ∝ Im(e_{k+1}·conj(e_k)).  Drift is estimated by least-squares fit.
    //    The artifact is drift_error = (16·Σk·bias_k − 120·Σbias_k) / 5440.  For this
    //    sequence Σk·Im(d_k) = −7 and Σ Im(d_k) = −1, giving drift_error = ε/680 ≈ 0
    //    (same as the alternating [45°,315°] preamble).  Without this property the fit
    //    misestimates drift by ~0.02 rad/sym, accumulating to >90° over a 64-symbol frame.
    //
    // 3. LMS training diversity: all 4 QPSK constellation points appear exactly 4× each,
    //    providing both I and Q variation for the supervised preamble-training phase of
    //    the LMS equalizer.  The alternating [45°,315°] preamble had constant I=0.707,
    //    which degraded equalizer convergence on dispersive HF channels.
    [
        gray_map(false, false), // k=0:  45°
        gray_map(false, true),  // k=1: 135°
        gray_map(true, true),   // k=2: 225°
        gray_map(true, false),  // k=3: 315°
        gray_map(true, true),   // k=4: 225°
        gray_map(false, true),  // k=5: 135°
        gray_map(false, false), // k=6:  45°
        gray_map(true, false),  // k=7: 315°
        gray_map(true, true),   // k=8: 225°
        gray_map(false, true),  // k=9: 135°
        gray_map(false, false), // k=10: 45°
        gray_map(false, true),  // k=11:135°
        gray_map(true, true),   // k=12:225°
        gray_map(true, false),  // k=13:315°
        gray_map(false, false), // k=14: 45°
        gray_map(true, false),  // k=15:315°
    ]
    .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray_map_constellation_points() {
        assert_eq!(gray_map(false, false), (INV_SQRT_2, INV_SQRT_2));
        assert_eq!(gray_map(false, true), (-INV_SQRT_2, INV_SQRT_2));
        assert_eq!(gray_map(true, true), (-INV_SQRT_2, -INV_SQRT_2));
        assert_eq!(gray_map(true, false), (INV_SQRT_2, -INV_SQRT_2));
    }

    /// CPU vs GPU RRC FIR equivalence: max sample delta < 1e-4.
    #[cfg(feature = "gpu")]
    #[test]
    fn qpsk500_rrc_gpu_matches_cpu() {
        use openpulse_core::plugin::ModulationConfig;

        let ctx = match openpulse_gpu::GpuContext::init() {
            Some(c) => c,
            None => return, // skip on headless / CI without GPU
        };
        let cfg = ModulationConfig {
            mode: "QPSK500-RRC".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"cpu vs gpu equivalence test";
        let cpu_out = qpsk_modulate(payload, &cfg).expect("CPU modulate failed");
        let gpu_out = qpsk_modulate_rrc_gpu(payload, &cfg, &ctx).expect("GPU modulate failed");
        assert_eq!(cpu_out.len(), gpu_out.len(), "output length mismatch");
        let max_delta = cpu_out
            .iter()
            .zip(gpu_out.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_delta < 1e-4,
            "GPU/CPU max sample delta {max_delta:.2e} exceeds 1e-4"
        );
    }
}
