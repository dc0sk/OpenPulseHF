use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::{Mutex, OnceLock};

use num_complex::Complex32;
use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, PulseShape};
use openpulse_dsp::acquisition::{estimate_cfo_data_aided, preamble_corr_sq};
use openpulse_dsp::constellation::psk_symbol_noise_var;
use openpulse_dsp::equalizer::LmsEqualizer;
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::pll::CarrierPll;
use openpulse_dsp::rrc::generate_rrc_coefficients;

use crate::modulate::{
    gray_map_8psk, preamble_symbols, samples_per_symbol, PREAMBLE_SYMS, RRC_SPAN_SYMBOLS, TAIL_SYMS,
};
use crate::parse_baud_rate;

pub fn afc_estimate_hz(samples: &[f32], config: &ModulationConfig) -> Option<f32> {
    let baud = parse_baud_rate(&config.mode).ok()?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud).ok()?;
    let cosine_overlap =
        config.pulse_shape == PulseShape::CosineOverlap || is_hf_mode(&config.mode);

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return None;
    }

    // RRC modes: estimate CFO on the matched-filtered baseband preamble, not the
    // passband Hann demod.  At the RRC modes' low oversampling (4 sps for
    // 8PSK2000-RRC) the Hann passband demod is badly mismatched and the estimate is
    // erratic (e.g. a spurious +25 Hz lock at zero offset), landing outside the RRC
    // demod's tolerance.  Data-aided on the matched preamble (range ±baud/2) is
    // accurate; the downstream Costas absorbs the small residual ISI bias.
    let rrc_alpha = if let PulseShape::Rrc { alpha } = config.pulse_shape {
        Some(alpha)
    } else if config.mode.ends_with("-RRC") {
        Some(0.35f32)
    } else {
        None
    };
    if let Some(alpha) = rrc_alpha {
        let (i_bb, q_bb) = rrc_baseband(samples, n, baud, fc, fs, alpha);
        let timing = find_timing_offset_bb_iq(&i_bb, &q_bb, n);
        let (mut i_syms, mut q_syms) = (Vec::new(), Vec::new());
        let mut pos = timing;
        while pos < i_bb.len() && i_syms.len() < PREAMBLE_SYMS {
            i_syms.push(i_bb[pos]);
            q_syms.push(q_bb[pos]);
            pos += n;
        }
        if i_syms.len() < 2 {
            return Some(0.0);
        }
        return Some(
            estimate_cfo_data_aided(&i_syms, &q_syms, &preamble_symbols(), baud).unwrap_or(0.0),
        );
    }

    let timing = find_timing_offset(samples, n, fc, fs, cosine_overlap);
    let syms = demodulate_symbols(samples, n, fc, fs, timing, cosine_overlap);
    if syms.len() < PREAMBLE_SYMS {
        return None;
    }

    // Two-stage estimate.  Wide-range ANCHOR = a GLOBAL grid search (`coarse_cfo_grid`): re-demod
    // the raw preamble at each candidate centre and take the one that maximises the preamble
    // correlation.  The 1-lag consecutive-symbol data-aided estimate is erratic beyond ~30 Hz at the
    // 8PSK1000 oversampling (8 sps) — its crossfade ISI bias, AND the preamble demod itself is
    // corrupted at large CFO — so a hill-climbing settle locks a spurious fixed point (true +40 →
    // +82 Hz; see the psk8_1000_afc_diag probe).  A global grid over the raw samples can't stick.
    // Once the anchor drives the residual inside ±baud/64, refine with the ISI-robust half-split.
    let rough = coarse_cfo_grid(samples, n, fc, fs, cosine_overlap, baud);
    if rough.abs() < baud / 64.0 {
        let raw = estimate_cfo_half_split(&syms, baud)?;
        let bias = half_split_bias(config, baud, n, fc, fs, cosine_overlap);
        return Some(raw - bias);
    }
    Some(rough)
}

/// Global two-stage coarse-CFO grid search: re-demodulate the raw preamble at each candidate centre
/// and return the offset that maximises the phase-insensitive preamble correlation. Global (not
/// hill-climbing), so it cannot lock a spurious fixed point the way the symbol-domain 1-lag anchor +
/// iterative settle do at dense low-oversampling modes (8PSK1000, 8 sps).
///
/// Covers the full **±baud/2** acquisition range (matching the previous data-aided anchor and the
/// engine's ~±450 Hz inter-rig expectation) without a per-candidate blow-up: a coarse scan at a step
/// below the 16-symbol correlation main-lobe width (baud/16) locates the peak region, then a fine
/// scan at ~baud/100 around it lands within the half-split's ±baud/64 *gate* so the settle refines to
/// sub-Hz. (Half-split RANGE is ±baud/16; the gate that engages it is |rough| < baud/64.)
fn coarse_cfo_grid(
    samples: &[f32],
    n: usize,
    fc: f32,
    fs: f32,
    cosine_overlap: bool,
    baud: f32,
) -> f32 {
    let expected = preamble_symbols();
    // Demod only the preamble region (+ margin) for speed.
    let span = ((PREAMBLE_SYMS + 4) * n).min(samples.len());
    let pre = &samples[..span];
    let score = |f: f32| -> f32 {
        let timing = find_timing_offset(pre, n, fc + f, fs, cosine_overlap);
        let syms = demodulate_symbols(pre, n, fc + f, fs, timing, cosine_overlap);
        if syms.len() < PREAMBLE_SYMS {
            return -1.0;
        }
        preamble_corr_sq(&syms[..PREAMBLE_SYMS], &expected)
    };
    // Stage 1: coarse scan over ±baud/2 at ~baud/24 (< main lobe baud/16 → cannot skip the peak).
    let coarse_step = (baud / 24.0).max(1.0);
    let coarse_n = (baud / 2.0 / coarse_step).round() as i32;
    let (mut best_f, mut best) = (0.0f32, -1.0f32);
    for i in -coarse_n..=coarse_n {
        let f = i as f32 * coarse_step;
        let s = score(f);
        if s > best {
            best = s;
            best_f = f;
        }
    }
    // Stage 2: fine scan ±coarse_step around the coarse peak at ~baud/100.
    let fine_step = (baud / 100.0).max(0.5);
    let fine_n = (coarse_step / fine_step).ceil() as i32;
    for i in -fine_n..=fine_n {
        let f = best_f + i as f32 * fine_step;
        let s = score(f);
        if s > best {
            best = s;
            best_f = f;
        }
    }
    best_f
}

/// CFO from the phase difference between the two preamble halves.
///
/// Each half's phase is the ISI-robust vector-sum correlation `arg(Σ r·conj(e))`,
/// whose bias is bounded by the preamble's small 1-lag autocorrelation — unlike the
/// consecutive-symbol increments of `estimate_cfo_data_aided`, where the crossfade
/// 1-lag ISI (large at the 8PSK1000 oversampling of 8 sps) biases every increment
/// and the bias accumulates.  Range ±baud/16 (±62.5 Hz at 1000 baud).  Carries a
/// constant structural bias (the clean preamble reads non-zero); see `half_split_bias`.
fn estimate_cfo_half_split(syms: &[(f32, f32)], baud: f32) -> Option<f32> {
    if syms.len() < PREAMBLE_SYMS {
        return None;
    }
    let expected = preamble_symbols();
    let half = PREAMBLE_SYMS / 2;
    let p_a = preamble_corr_phase(syms, &expected, 0..half);
    let p_b = preamble_corr_phase(syms, &expected, half..PREAMBLE_SYMS);
    let mut dphi = p_b - p_a;
    while dphi > PI {
        dphi -= 2.0 * PI;
    }
    while dphi < -PI {
        dphi += 2.0 * PI;
    }
    let k_a = (half - 1) as f32 / 2.0;
    let k_b = half as f32 + (half - 1) as f32 / 2.0;
    Some(dphi / (k_b - k_a) * baud / (2.0 * PI))
}

/// Structural half-split bias for a mode: the `estimate_cfo_half_split` reading on a
/// clean, zero-offset modulated preamble (no CFO present), which the crossfade ISI
/// makes non-zero.  Subtracting it debiases the live half-split estimate.  Computed
/// once per mode and cached (the modulate+demod is cheap but called per scan step).
fn half_split_bias(
    config: &ModulationConfig,
    baud: f32,
    n: usize,
    fc: f32,
    fs: f32,
    cosine_overlap: bool,
) -> f32 {
    static CACHE: OnceLock<Mutex<HashMap<String, f32>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(&b) = guard.get(&config.mode) {
            return b;
        }
    }
    let ref_cfg = ModulationConfig {
        center_frequency: fc,
        ..config.clone()
    };
    let bias = crate::modulate::psk8_modulate(&[0x5Au8; 32], &ref_cfg)
        .ok()
        .and_then(|sig| {
            let timing = find_timing_offset(&sig, n, fc, fs, cosine_overlap);
            let syms = demodulate_symbols(&sig, n, fc, fs, timing, cosine_overlap);
            estimate_cfo_half_split(&syms, baud)
        })
        .unwrap_or(0.0);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(config.mode.clone(), bias);
    }
    bias
}

pub fn psk8_demodulate(samples: &[f32], config: &ModulationConfig) -> Result<Vec<u8>, ModemError> {
    let data = extract_data_symbols(samples, config)?;
    let bits = symbols_to_bits(&data);
    Ok(bits_to_bytes(&bits))
}

/// Return soft LLRs for every bit in the demodulated byte stream.
///
/// Uses max-log-MAP: LLR_k = min_d²(bit_k=1) − min_d²(bit_k=0).
/// Positive LLR means bit=0 is more likely (same sign convention as the hard-decision stub).
/// Output length equals `psk8_demodulate` byte count × 8, in the same bit order (LSB-first).
pub fn psk8_demodulate_soft(
    samples: &[f32],
    config: &ModulationConfig,
) -> Result<Vec<f32>, ModemError> {
    let data = extract_data_symbols(samples, config)?;
    let raw = compute_soft_llrs(&data);
    // symbols_to_bits yields 3 bits/symbol; bits_to_bytes drops the partial final chunk.
    let n_complete_bytes = (data.len() * 3) / 8;
    let mut llrs = raw[..n_complete_bytes * 8].to_vec();

    // Calibrate the soft values into *true* log-likelihood ratios (magnitude ∝ 1/σ²). Nothing that
    // decodes a single frame notices — soft Viterbi, min-sum LDPC and max-log turbo are all
    // scale-invariant — but HARQ soft combining across receive attempts does: uncalibrated, an attempt
    // from a deep fade votes as loudly as a clean one. See `openpulse_core::fec::combine_llrs_map`.
    // `compute_soft_llrs` emits max-log-MAP squared-distance differences; dividing by the 2-D noise
    // variance is exactly the missing 1/σ².
    let syms: Vec<Complex32> = data.iter().map(|&(i, q)| Complex32::new(i, q)).collect();
    let (_, noise_var_per_dim) = psk_symbol_noise_var(&syms, 3);
    let inv = 1.0 / (2.0 * noise_var_per_dim);
    for l in llrs.iter_mut() {
        *l *= inv;
    }
    Ok(llrs)
}

/// Extract Gray-coded IQ data symbols after preamble/tail stripping with LMS equalization.
fn extract_data_symbols(
    samples: &[f32],
    config: &ModulationConfig,
) -> Result<Vec<(f32, f32)>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;
    let cosine_overlap =
        config.pulse_shape == PulseShape::CosineOverlap || is_hf_mode(&config.mode);
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
    let mut syms = if let Some(alpha) = rrc_alpha {
        // The RRC Costas PLL removes carrier frequency/phase only up to the 45° 8PSK
        // rotational ambiguity; resolve the absolute phase against the known preamble.
        let rrc_syms = psk8_demodulate_rrc(samples, n, baud, fc, fs, alpha);
        carrier_phase_correct(&rrc_syms, config.afc_correction_hz)
    } else {
        let timing = find_timing_offset(samples, n, fc, fs, cosine_overlap);
        let mut raw = demodulate_symbols(samples, n, fc, fs, timing, cosine_overlap);
        // Undo the transmitter's raised-cosine crossfade before any carrier/equalizer stage; the ISI is
        // on the next (anti-causal) symbol, so the downstream DFE cannot reach it.  Only the plain
        // (crossfade) pulse leaks the neighbour — the cosine-overlap `sin²` pulse is per-symbol and has
        // no crossfade, so cancellation there would inject error.
        if !cosine_overlap {
            cancel_crossfade_isi(&mut raw, crossfade_isi_beta(n));
        }
        let phase_corrected = carrier_phase_correct(&raw, config.afc_correction_hz);
        carrier_pll_track(&phase_corrected)
    };

    if syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".to_string(),
        ));
    }

    // Apply LMS equalization trained on preamble.
    syms = psk8_lms_equalize(&syms, &config.mode);

    Ok(syms[PREAMBLE_SYMS..(syms.len() - TAIL_SYMS)].to_vec())
}

/// Max-log-MAP soft LLR per bit for a slice of received IQ symbols.
///
/// Returns 3 LLRs per symbol in the order [b0, b1, b2, b0, b1, b2, ...].
fn compute_soft_llrs(syms: &[(f32, f32)]) -> Vec<f32> {
    let pts: [((f32, f32), [bool; 3]); 8] = [
        (gray_map_8psk(false, false, false), [false, false, false]),
        (gray_map_8psk(false, false, true), [false, false, true]),
        (gray_map_8psk(false, true, false), [false, true, false]),
        (gray_map_8psk(false, true, true), [false, true, true]),
        (gray_map_8psk(true, false, false), [true, false, false]),
        (gray_map_8psk(true, false, true), [true, false, true]),
        (gray_map_8psk(true, true, false), [true, true, false]),
        (gray_map_8psk(true, true, true), [true, true, true]),
    ];

    let mut llrs = Vec::with_capacity(syms.len() * 3);
    for &(ri, rq) in syms {
        for bit_pos in 0..3usize {
            let mut min_d0 = f32::INFINITY;
            let mut min_d1 = f32::INFINITY;
            for &((ci, cq), bits) in &pts {
                let di = ri - ci;
                let dq = rq - cq;
                let d2 = di * di + dq * dq;
                if bits[bit_pos] {
                    min_d1 = min_d1.min(d2);
                } else {
                    min_d0 = min_d0.min(d2);
                }
            }
            // Positive → bit=0 more likely (matches hard-decision sign convention).
            llrs.push(min_d1 - min_d0);
        }
    }
    llrs
}

/// GPU-accelerated hard demodulator.  Uses GPU RRC FIR for the matched filter;
/// timing/carrier recovery and LMS equalization remain on CPU.
/// Returns `None` for non-RRC modes or on GPU error (caller falls back to CPU).
#[cfg(feature = "gpu")]
pub fn psk8_demodulate_gpu(
    samples: &[f32],
    config: &ModulationConfig,
    ctx: &std::sync::Arc<openpulse_gpu::GpuContext>,
) -> Option<Result<Vec<u8>, ModemError>> {
    // Only accelerate RRC modes; non-RRC path has no FIR to offload.
    let alpha = if let PulseShape::Rrc { alpha } = config.pulse_shape {
        alpha
    } else if config.mode.ends_with("-RRC") {
        0.35
    } else {
        return None;
    };

    let baud = parse_baud_rate(&config.mode).ok()?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud).ok()?;

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return Some(Err(ModemError::Demodulation(
            "signal too short".to_string(),
        )));
    }

    let two_pi = 2.0 * PI;
    let num_taps = RRC_SPAN_SYMBOLS * n + 1;
    let coeffs = generate_rrc_coefficients(fs, baud, alpha, num_taps);
    let group_delay = (num_taps - 1) / 2;

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

    let gpu_rrc = |mix: Vec<f32>| -> Option<Vec<f32>> {
        let padded: Vec<f32> = mix
            .iter()
            .copied()
            .chain(std::iter::repeat_n(0.0, group_delay))
            .collect();
        let filtered = openpulse_gpu::gpu_rrc_fir(ctx, &padded, &coeffs)?;
        Some(filtered[group_delay..].to_vec())
    };

    let i_bb = gpu_rrc(i_mix)?;
    let q_bb = gpu_rrc(q_mix)?;

    let initial_timing = find_timing_offset_bb_iq(&i_bb, &q_bb, n);
    let mut syms = gardner_pll_sample_rrc(&i_bb, &q_bb, n, initial_timing);

    if syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Some(Err(ModemError::Demodulation(
            "no data symbols after preamble".to_string(),
        )));
    }

    syms = psk8_lms_equalize(&syms, &config.mode);
    let data = syms[PREAMBLE_SYMS..(syms.len() - TAIL_SYMS)].to_vec();
    let bits = symbols_to_bits(&data);
    Some(Ok(bits_to_bytes(&bits)))
}

/// GPU-accelerated soft demodulator. Falls back to the CPU path if the GPU
/// returns `None`.
#[cfg(feature = "gpu")]
pub fn psk8_demodulate_soft_gpu(
    samples: &[f32],
    config: &ModulationConfig,
    ctx: &std::sync::Arc<openpulse_gpu::GpuContext>,
) -> Result<Vec<f32>, ModemError> {
    let data = extract_data_symbols(samples, config)?;

    let pts_iq: Vec<(f32, f32)> = [
        (false, false, false),
        (false, false, true),
        (false, true, false),
        (false, true, true),
        (true, false, false),
        (true, false, true),
        (true, true, false),
        (true, true, true),
    ]
    .iter()
    .map(|&(b2, b1, b0)| gray_map_8psk(b2, b1, b0))
    .collect();
    let bit_table: Vec<u32> = (0..8u32).collect();

    if let Some(raw) = openpulse_gpu::gpu_soft_demod(ctx, &data, &pts_iq, &bit_table, 3) {
        let n_complete_bytes = (data.len() * 3) / 8;
        return Ok(raw[..n_complete_bytes * 8].to_vec());
    }
    // GPU returned None — fall back to CPU.
    psk8_demodulate_soft(samples, config)
}

/// RRC demodulation: downmix → matched RRC filter → brute-force timing → sample.
/// Downmix to baseband I/Q and apply the RRC matched filter — the shared front-end
/// of the RRC demod and the RRC AFC estimate.
fn rrc_baseband(
    samples: &[f32],
    n: usize,
    baud: f32,
    fc: f32,
    fs: f32,
    alpha: f32,
) -> (Vec<f32>, Vec<f32>) {
    let two_pi = 2.0 * PI;
    let num_taps = RRC_SPAN_SYMBOLS * n + 1;
    let coeffs = generate_rrc_coefficients(fs, baud, alpha, num_taps);
    let group_delay = (num_taps - 1) / 2;

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
    (rrc_filter(i_mix), rrc_filter(q_mix))
}

fn psk8_demodulate_rrc(
    samples: &[f32],
    n: usize,
    baud: f32,
    fc: f32,
    fs: f32,
    alpha: f32,
) -> Vec<(f32, f32)> {
    // 1+2. Downmix to baseband I/Q and apply the RRC matched filter.
    let (i_bb, q_bb) = rrc_baseband(samples, n, baud, fc, fs, alpha);

    // 3. Coarse timing acquisition via IQ preamble correlation (brute-force).
    let initial_timing = find_timing_offset_bb_iq(&i_bb, &q_bb, n);

    // 4. Resolve the coarse carrier phase from the preamble and de-rotate the whole
    //    baseband BEFORE the decision-directed loop.  The 8PSK Costas PLL only locks
    //    to the nearest 45° rotational symmetry; if the true offset is off-grid (e.g.
    //    67.5°) the PLL hard-quantises every symbol onto the wrong grid, leaving them
    //    on the ±22.5° decision boundary.  Pre-de-rotating onto the correct grid (the
    //    preamble is a known pilot) lets the PLL track only the residual.
    let phase_0 = coarse_baseband_phase(&i_bb, &q_bb, n, initial_timing);
    let (sin0, cos0) = (-phase_0).sin_cos();
    let i_rot: Vec<f32> = i_bb
        .iter()
        .zip(q_bb.iter())
        .map(|(&i, &q)| i * cos0 - q * sin0)
        .collect();
    let q_rot: Vec<f32> = i_bb
        .iter()
        .zip(q_bb.iter())
        .map(|(&i, &q)| i * sin0 + q * cos0)
        .collect();

    // 5. Adaptive timing + carrier recovery on the de-rotated baseband.
    gardner_pll_sample_rrc(&i_rot, &q_rot, n, initial_timing)
}

/// Coarse carrier phase from the preamble samples of the RRC baseband.
///
/// Samples the `PREAMBLE_SYMS` preamble symbols at `timing` with stride `n` and
/// returns arg(Σ r_k·conj(e_k)) — the ML phase estimate, robust to ISI via the
/// vector sum.
fn coarse_baseband_phase(i_bb: &[f32], q_bb: &[f32], n: usize, timing: usize) -> f32 {
    let expected = preamble_symbols();
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (s, &(ei, eq)) in expected.iter().enumerate().take(PREAMBLE_SYMS) {
        let idx = timing + s * n;
        if idx >= i_bb.len() {
            break;
        }
        let (ri, rq) = (i_bb[idx], q_bb[idx]);
        re += ri * ei + rq * eq;
        im += rq * ei - ri * eq;
    }
    im.atan2(re)
}

/// Fixed-stride symbol sampling + carrier recovery (Costas PLL) for 8PSK-RRC.
///
/// See the QPSK twin for why this deliberately does NOT run an interpolating
/// timing loop: at 1000 baud the Watterson delay spread spans symbols and
/// biases the Gardner error toward the echo centroid, walking the timing off
/// the preamble lock; SRO over these short frames is negligible.
fn gardner_pll_sample_rrc(
    i_bb: &[f32],
    q_bb: &[f32],
    n: usize,
    initial_timing: usize,
) -> Vec<(f32, f32)> {
    let start = initial_timing.min(i_bb.len());
    let mut pll = CarrierPll::new(0.02, 3);
    let mut syms = Vec::new();
    let mut pos = start;
    while pos < i_bb.len() {
        let s_i = i_bb[pos];
        let s_q = q_bb.get(pos).copied().unwrap_or(0.0);
        pll.update(s_i, s_q);
        syms.push(pll.correct(s_i, s_q));
        pos += n;
    }
    syms
}

/// Brute-force timing search using both I and Q baseband channels.
///
/// Uses the squared magnitude of the complex preamble correlation so the metric
/// is invariant to the unknown residual carrier phase after downmix.
fn find_timing_offset_bb_iq(i_bb: &[f32], q_bb: &[f32], n: usize) -> usize {
    let expected = preamble_symbols();
    let mut best_off = 0usize;
    let mut best_score = f32::NEG_INFINITY;

    let mut received = vec![(0.0f32, 0.0f32); PREAMBLE_SYMS];
    for off in 0..n {
        if i_bb.len() < off + n * PREAMBLE_SYMS {
            break;
        }
        for (s, slot) in received.iter_mut().enumerate() {
            *slot = (i_bb[off + s * n], q_bb[off + s * n]);
        }
        let score = preamble_corr_sq(&received, &expected);
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
        // Demodulate ONLY the preamble span at this offset (the slice may be
        // multi-second; demodulating all of it per offset is O(offsets × N)).
        let span_end = (off + n * PREAMBLE_SYMS).min(samples.len());
        let syms = demodulate_symbols(&samples[..span_end], n, fc, fs, off, cosine_overlap);
        if syms.len() < PREAMBLE_SYMS {
            continue;
        }
        // Squared magnitude of the complex preamble correlation Σ r_k·conj(e_k),
        // not the signed real part.  The carrier phase at the start of the slice is
        // unknown; the real part collapses to 0 near 90°/270° and a wrong offset
        // wins.  |·|² is rotation-invariant and peaks at the correct offset for
        // any carrier phase (see openpulse_dsp::acquisition::preamble_corr_sq).
        let score = preamble_corr_sq(&syms[..PREAMBLE_SYMS], &expected);
        if score > best_score {
            best_score = score;
            best_off = off;
        }
    }

    best_off
}

/// Phase of the complex preamble correlation Σ r_k·conj(e_k) over `range`.
///
/// This vector-sum estimator is robust to the crossfade ISI: per-symbol phase
/// errors `arg(r_k·conj(e_k))` can exceed ±90° at low oversampling (8PSK9600 has
/// only 5 samples/symbol), so averaging their wrapped `atan2` values is meaningless,
/// but the ISI averages out coherently in the vector sum (its bias is bounded by the
/// preamble's small 1-lag autocorrelation R₁).
fn preamble_corr_phase(
    syms: &[(f32, f32)],
    expected: &[(f32, f32)],
    range: std::ops::Range<usize>,
) -> f32 {
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for k in range {
        let (ri, rq) = syms[k];
        let (ei, eq) = expected[k];
        re += ri * ei + rq * eq;
        im += rq * ei - ri * eq;
    }
    im.atan2(re)
}

/// Remove the static carrier phase offset using the known preamble as a pilot.
///
/// φ₀ is always removed: on real hardware the carrier phase at frame start is
/// effectively random, and the non-HF 8PSK path has no equalizer to absorb it, so
/// without this every symbol decision would be rotated to a wrong constellation
/// point.  φ₀ is the single ML phase over the whole preamble via the ISI-robust
/// vector-sum correlation; residual frequency drift is left to the downstream
/// Costas (`carrier_pll_track`), which is the proper tracker for it.
///
/// A 2-point "drift fit" (slope from the two 8-symbol preamble halves, extrapolated
/// across the frame) was previously applied when the engine signalled an RF offset
/// (`afc_correction_hz` ≥ 0.5).  It was **removed**: the 8-symbol baseline makes the
/// half-to-half phase difference dominated by per-half ISI rather than true drift,
/// so it extrapolated a spurious slope over the ~190 data symbols and broke decode
/// even when the carrier frequency was already exactly correct — the actual cause of
/// the 8PSK carrier-offset acquisition gap (an AFC-accurate frame still failed).
fn carrier_phase_correct(syms: &[(f32, f32)], _afc_correction_hz: f32) -> Vec<(f32, f32)> {
    if syms.len() < PREAMBLE_SYMS {
        return syms.to_vec();
    }
    let expected = preamble_symbols();

    let phase_0 = preamble_corr_phase(syms, &expected, 0..PREAMBLE_SYMS);
    // Only correct a *gross* offset.  The job here is to resolve the random
    // hardware carrier phase (uniform over 360°); an offset already within roughly
    // half the 22.5° 8PSK decision margin is harmless and the downstream Costas PLL
    // removes any residual anyway.  Skipping it keeps the constellation untouched on
    // clean low-offset signals — at the 5-samples/symbol 8PSK9600 rate the eye is
    // nearly closed by crossfade ISI, so even a few degrees of spurious rotation
    // would tip marginal symbols past the boundary.
    if phase_0.abs() < 0.2 {
        return syms.to_vec();
    }
    let (s, c) = (-phase_0).sin_cos();
    syms.iter()
        .map(|&(i, q)| (i * c - q * s, i * s + q * c))
        .collect()
}

/// One decision-directed 8PSK carrier-tracking pass seeded with an initial loop
/// frequency (rad/symbol); returns the de-rotated symbols and the final frequency.
///
/// 8PSK is constant-modulus, so the phase error is `Im(r·conj(d))` for the nearest
/// constellation point `d` (|d| = 1).  A second-order loop integrates it.
fn dd_track_seeded(syms: &[(f32, f32)], loop_bw: f32, init_freq: f32) -> (Vec<(f32, f32)>, f32) {
    let alpha = loop_bw;
    let beta = loop_bw * loop_bw * 0.25;
    let mut phase = 0.0f32;
    let mut freq = init_freq;
    let mut out = Vec::with_capacity(syms.len());
    for &(i, q) in syms {
        let (s, c) = (-phase).sin_cos();
        let di = i * c - q * s;
        let dq = i * s + q * c;
        let (pi, pq) = psk8_map_decision(di, dq);
        let err = dq * pi - di * pq; // Im(r·conj(d)), |d| = 1
        out.push((di, dq));
        freq += beta * err;
        phase += freq + alpha * err;
    }
    (out, freq)
}

/// Track residual carrier frequency drift with a two-pass decision-directed 8PSK loop.
///
/// `carrier_phase_correct` removes the static phase offset, but a residual frequency
/// offset still produces a linear phase ramp across the frame.  A single forward loop
/// seeded at zero spends the early symbols *acquiring* that offset — and at the dense
/// 45° 8PSK spacing those mis-rotated early symbols decide wrong and the loop never
/// recovers (the prior single-pass Costas required the engine AFC to land within
/// ±0.5 Hz, which its ~0.9 Hz preamble-ISI bias could not).  Pass 1 converges the
/// offset over the whole frame; pass 2 re-runs the loop seeded with that frequency so
/// every symbol, including the first, is de-rotated at the correct rate — the same
/// structure 64QAM uses to absorb the identical AFC bias.  On a clean offset-free
/// frame pass 1 converges to ~0 and pass 2 is a no-op, so nothing regresses.  The loop
/// bandwidth stays gentle (0.010): the decision-directed detector is noisier on the
/// dense grid, so a tighter loop would perturb marginal symbols.
fn carrier_pll_track(syms: &[(f32, f32)]) -> Vec<(f32, f32)> {
    // Acquisition/tracking split: pass 1 runs a wider loop (0.05) so it can ACQUIRE
    // the residual frequency within the short frame — the gentle tracking loop alone
    // converges far too slowly to lock even a ~1 Hz residual over ~60–200 symbols,
    // which is why the engine's AFC had to land within ±0.5 Hz (its ~0.9 Hz preamble-
    // ISI bias could not).  Pass 2 re-runs the gentle loop (0.010) seeded with that
    // frequency, so it tracks cleanly from the first symbol without the wide loop's
    // self-noise.  On a clean offset-free frame pass 1 converges to ~0 and pass 2 is
    // the original gentle loop, so clean / Watterson / weak-signal paths do not regress.
    const ACQUIRE_BW: f32 = 0.05;
    const TRACK_BW: f32 = 0.010;
    let (_, freq) = dd_track_seeded(syms, ACQUIRE_BW, 0.0);
    let (out, _) = dd_track_seeded(syms, TRACK_BW, freq);
    out
}

/// ISI coefficient of the rectangular ("plain") 8PSK pulse's raised-cosine crossfade, for the
/// squared-cosine matched window this plugin's `demodulate_symbols` uses.
///
/// The modulator blends adjacent symbols: sample `i` of slot `k` is `sym_k·w_tail(i) + sym_{k+1}·w_head(i)`
/// with `w_tail = ½(1+cos πi/n)`, `w_head = 1−w_tail`.  The matched one-slot demod integrates against
/// `w_tail²`, so it recovers `A·(sym_k + β·sym_{k+1})` where `β = Σ w_head·w_tail² / Σ w_tail³` and the
/// common scale `A = Σ w_tail³ / Σ w_tail⁴` divides out.  Unlike QPSK's un-squared window (β = ⅓,
/// n-independent), the cubed/quartic weighting makes β vary with the oversampling — 0.182 at n = 16
/// (8PSK500), 0.167 at n = 8 (8PSK1000) — so it is computed from the actual window rather than a constant.
fn crossfade_isi_beta(n: usize) -> f32 {
    let mut sum_head_tail2 = 0.0f32;
    let mut sum_tail3 = 0.0f32;
    for i in 0..n {
        let w_tail = 0.5 * (1.0 + (PI * i as f32 / n as f32).cos());
        let w_head = 1.0 - w_tail;
        sum_head_tail2 += w_head * w_tail * w_tail;
        sum_tail3 += w_tail * w_tail * w_tail;
    }
    if sum_tail3 > 1e-9 {
        sum_head_tail2 / sum_tail3
    } else {
        0.0
    }
}

/// Remove the crossfade ISI from a rectangular-8PSK symbol projection stream in place.
///
/// `p_k = A·(sym_k + β·sym_{k+1})` is bidiagonal, so `s_k = p_k − β·s_{k+1}` recovers the (uniformly
/// scaled) symbols exactly by back-substitution.  The recursion is stable — each step scales the running
/// error by `β < 0.2`, so it decays backward — and the terminal is exact: the modulator sets the last
/// data symbol's successor to zero.  Noise is amplified by only `1/(1−β²) ≈ 1.03`.
///
/// The ISI is anti-causal (the *next* symbol), so the downstream decision-feedback equalizer — which
/// feeds back *past* decisions — cannot cancel it; left in, it floors the recovered-symbol EVM at
/// `β² ≈ −15 dB` regardless of SNR, which caps every soft consumer (HARQ combining, soft FEC).
fn cancel_crossfade_isi(symbols: &mut [(f32, f32)], beta: f32) {
    for k in (0..symbols.len().saturating_sub(1)).rev() {
        symbols[k].0 -= beta * symbols[k + 1].0;
        symbols[k].1 -= beta * symbols[k + 1].1;
    }
}

/// Test-only accessor: return the equalized, crossfade-cancelled data symbols (preamble/tail stripped).
#[doc(hidden)]
pub fn extract_data_symbols_for_test(
    samples: &[f32],
    config: &ModulationConfig,
) -> Result<Vec<(f32, f32)>, ModemError> {
    extract_data_symbols(samples, config)
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

fn is_hf_mode(mode: &str) -> bool {
    mode.contains("-HF")
}

fn should_equalize(mode: &str) -> bool {
    is_hf_mode(mode)
}

fn psk8_map_decision(i: f32, q: f32) -> (f32, f32) {
    let (b0, b1, b2) = nearest_gray_triplet(i, q);
    gray_map_8psk(b0, b1, b2)
}

fn lms_profile(mode: &str) -> (usize, usize, f32) {
    // HF 1000-baud paths see stronger multipath/ISI under Watterson Moderate/Poor,
    // so enable a short DFE section and slightly smaller step size for stability.
    if is_hf_mode(mode) && mode.contains("-RRC") && mode.contains("1000") {
        (9, 2, 0.012)
    } else if is_hf_mode(mode) && mode.contains("1000") {
        (9, 2, 0.015)
    } else {
        (7, 0, 0.02)
    }
}

fn psk8_lms_equalize(symbols: &[(f32, f32)], mode: &str) -> Vec<(f32, f32)> {
    if !should_equalize(mode) {
        return symbols.to_vec();
    }

    if symbols.is_empty() {
        return Vec::new();
    }

    let train_len = PREAMBLE_SYMS.min(symbols.len());
    let expected = preamble_symbols();
    let training = &expected[..train_len];

    let (fwd_len, dfe_len, mu) = lms_profile(mode);
    let mut eq = LmsEqualizer::new(fwd_len, dfe_len, mu);
    let (i_syms, q_syms): (Vec<f32>, Vec<f32>) = symbols.iter().copied().unzip();
    let (i_eq, q_eq) = eq.process_frame(
        &i_syms,
        &q_syms,
        &training.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
        &training.iter().map(|(_, q)| *q).collect::<Vec<_>>(),
        psk8_map_decision,
    );

    i_eq.into_iter().zip(q_eq).collect()
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
    use openpulse_channel::watterson::WattersonChannel;
    use openpulse_channel::{ChannelModel, WattersonConfig};
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

    fn ber_helper(decoded: &[u8], expected: &[u8]) -> f32 {
        let mut bit_errors = 0usize;
        let mut total_bits = 0usize;
        for (dec_byte, exp_byte) in decoded.iter().zip(expected.iter()) {
            let xor = dec_byte ^ exp_byte;
            bit_errors += xor.count_ones() as usize;
            total_bits += 8;
        }
        if total_bits == 0 {
            0.0
        } else {
            bit_errors as f32 / total_bits as f32
        }
    }

    #[test]
    fn psk8_1000_hf_watterson_moderate_f1_decode_coverage() {
        let payload: Vec<u8> = (0u8..=255).cycle().take(256).collect();
        let cfg = ModulationConfig {
            mode: "8PSK1000-HF".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            ..ModulationConfig::default()
        };
        let tx_samples = crate::modulate::psk8_modulate(&payload, &cfg).expect("modulate");

        // Run 8 trials, each with a fresh seed.
        let mut decoded_count = 0usize;
        let mut low_ber_count = 0usize;
        for seed in [42u64, 111, 222, 333, 444, 555, 666, 777] {
            let mut ch = WattersonChannel::new(WattersonConfig::moderate_f1(Some(seed)))
                .expect("watterson moderate f1");
            let rx_samples = ch.apply(&tx_samples);
            if let Ok(decoded) = psk8_demodulate(&rx_samples, &cfg) {
                decoded_count += 1;
                let ber = ber_helper(&decoded, &payload);
                if ber <= 0.30 {
                    low_ber_count += 1;
                }
            }
        }

        // Expect at least 6 out of 8 to decode, with at least 1 showing BER <= 0.30.
        // The 0.30 threshold reflects realistic moderate-F1 fading on uncoded 8PSK:
        // 1 Hz Doppler over a 256-byte payload spans multiple coherence times, so
        // some envelope dwells will deliver clean symbols and others will not.
        assert!(
            decoded_count >= 6,
            "Moderate F1: decode coverage at least 6/8, got {}",
            decoded_count
        );
        assert!(
            low_ber_count >= 1,
            "Moderate F1: at least 1/8 should show BER <= 0.30, got {} (8PSK higher-order; poor_f1 test provides harder gate)",
            low_ber_count
        );
    }

    #[test]
    fn psk8_1000_hf_watterson_poor_f1_decode_presence() {
        let payload: Vec<u8> = (0u8..=255).cycle().take(256).collect();
        let cfg = ModulationConfig {
            mode: "8PSK1000-HF".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            ..ModulationConfig::default()
        };
        let tx_samples = crate::modulate::psk8_modulate(&payload, &cfg).expect("modulate");

        let mut best_ber = f32::INFINITY;
        let mut decoded_any = false;

        // Run 8 trials; prove the equalizer is actually recovering bits, not just
        // returning right-length output (verified by BER bound < 0.5, beat random).
        for seed in [42u64, 111, 222, 333, 444, 555, 666, 777] {
            let mut ch = WattersonChannel::new(WattersonConfig::poor_f1(Some(seed)))
                .expect("watterson poor f1");
            let rx_samples = ch.apply(&tx_samples);
            if let Ok(decoded) = psk8_demodulate(&rx_samples, &cfg) {
                decoded_any = true;
                let ber = ber_helper(&decoded, &payload);
                best_ber = best_ber.min(ber);
            }
        }

        // Prove we decode at least once and beat random guessing (BER < 0.5).
        assert!(decoded_any, "Poor F1: must decode at least once");
        assert!(
            best_ber < 0.5,
            "Poor F1: best BER must be < 0.5 (beat random), got {}",
            best_ber
        );
    }

    #[test]
    fn psk8_1000_hf_rrc_watterson_moderate_f1_decode_coverage() {
        let payload: Vec<u8> = (0u8..=255).cycle().take(256).collect();
        let cfg = ModulationConfig {
            mode: "8PSK1000-HF-RRC".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            ..ModulationConfig::default()
        };
        let tx_samples = crate::modulate::psk8_modulate(&payload, &cfg).expect("modulate");

        let mut decoded_count = 0usize;
        let mut best_ber = f32::INFINITY;
        for seed in [42u64, 111, 222, 333, 444, 555, 666, 777] {
            let mut ch = WattersonChannel::new(WattersonConfig::moderate_f1(Some(seed)))
                .expect("watterson moderate f1");
            let rx_samples = ch.apply(&tx_samples);
            if let Ok(decoded) = psk8_demodulate(&rx_samples, &cfg) {
                if decoded.len() >= payload.len() {
                    decoded_count += 1;
                    let ber = ber_helper(&decoded[..payload.len()], &payload);
                    best_ber = best_ber.min(ber);
                }
            }
        }

        assert!(
            decoded_count >= 6,
            "Moderate F1 (HF-RRC): decode coverage at least 6/8, got {}",
            decoded_count
        );
        assert!(
            best_ber < 0.35,
            "Moderate F1 (HF-RRC): best BER must be < 0.35, got {}",
            best_ber
        );
    }

    #[test]
    fn psk8_1000_hf_rrc_watterson_poor_f1_decode_presence() {
        let payload: Vec<u8> = (0u8..=255).cycle().take(256).collect();
        let cfg = ModulationConfig {
            mode: "8PSK1000-HF-RRC".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            ..ModulationConfig::default()
        };
        let tx_samples = crate::modulate::psk8_modulate(&payload, &cfg).expect("modulate");

        let mut best_ber = f32::INFINITY;
        let mut decoded_any = false;

        for seed in [42u64, 111, 222, 333, 444, 555, 666, 777] {
            let mut ch = WattersonChannel::new(WattersonConfig::poor_f1(Some(seed)))
                .expect("watterson poor f1");
            let rx_samples = ch.apply(&tx_samples);
            if let Ok(decoded) = psk8_demodulate(&rx_samples, &cfg) {
                if decoded.len() >= payload.len() {
                    decoded_any = true;
                    let ber = ber_helper(&decoded[..payload.len()], &payload);
                    best_ber = best_ber.min(ber);
                }
            }
        }

        assert!(decoded_any, "Poor F1 (HF-RRC): must decode at least once");
        assert!(
            best_ber < 0.5,
            "Poor F1 (HF-RRC): best BER must be < 0.5 (beat random), got {}",
            best_ber
        );
    }

    #[test]
    fn test_lms_profile_selection() {
        // HF 1000-baud modes should get the stronger profile.
        let (fwd, dfe, mu) = lms_profile("8PSK1000-HF");
        assert_eq!(fwd, 9);
        assert_eq!(dfe, 2);
        assert!(mu < 0.02, "HF mu should be smaller for stability");

        // Composite mode names with HF tag should still select HF profile.
        let (fwd, dfe, mu) = lms_profile("8PSK1000-HF-RRC");
        assert_eq!(fwd, 9);
        assert_eq!(dfe, 2);
        assert!(mu < 0.02, "HF-RRC mu should be smaller for stability");
        assert!((mu - 0.012).abs() < 1e-6, "HF-RRC profile uses tuned mu");
        assert!(should_equalize("8PSK1000-HF-RRC"));

        // Non-HF modes get baseline profile.
        let (fwd, dfe, mu) = lms_profile("8PSK500");
        assert_eq!(fwd, 7);
        assert_eq!(dfe, 0);
        assert_eq!(mu, 0.02);

        // Non-1000 HF mode still gets baseline.
        let (fwd, dfe, mu) = lms_profile("8PSK500-HF");
        assert_eq!(fwd, 7);
        assert_eq!(dfe, 0);
        assert_eq!(mu, 0.02);
    }

    #[test]
    fn test_lms_profile_hf_rrc_more_conservative_than_hf() {
        let (_fwd_hf, _dfe_hf, mu_hf) = lms_profile("8PSK1000-HF");
        let (_fwd_rrc, _dfe_rrc, mu_rrc) = lms_profile("8PSK1000-HF-RRC");
        assert!(mu_rrc < mu_hf);
    }
}
