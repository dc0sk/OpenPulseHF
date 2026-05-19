//! BPSK demodulator.
//!
//! The demodulation pipeline is:
//!
//! ```text
//! audio samples
//!   → multiply by I/Q reference carriers
//!   → matched-filter (half-Hann w_tail) integration per symbol period
//!   → timing search over first symbol period (brute-force energy maximisation)
//!   → differential phase detection (NRZI decode)
//!   → bits → bytes
//! ```
//!
//! ## Symbol timing
//!
//! The modulator prepends [`PREAMBLE_SYMS`] symbols with alternating phases
//! (+1, −1, +1, …).  The demodulator scans every possible timing offset
//! within the first symbol period, picks the offset that maximises the
//! demodulated preamble energy, and uses that offset for the rest of the frame.
//!
//! ## Phase ambiguity
//!
//! Differential detection removes the 180° absolute-phase ambiguity that
//! would otherwise require carrier-phase recovery.

use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, PulseShape};
use openpulse_dsp::equalizer::LmsEqualizer;
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::rrc::generate_rrc_coefficients;
use openpulse_dsp::timing::GardnerDetector;

use crate::modulate::{
    nrzi_encode, samples_per_symbol, PREAMBLE_SYMS, RRC_SPAN_SYMBOLS, TAIL_SYMS,
};
use crate::parse_baud_rate;

// ── Public entry point ────────────────────────────────────────────────────────

/// Demodulate audio `samples` and return the recovered bytes.
pub fn bpsk_demodulate(samples: &[f32], config: &ModulationConfig) -> Result<Vec<u8>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    let rrc_alpha = if let PulseShape::Rrc { alpha } = config.pulse_shape {
        Some(alpha)
    } else if config.mode.ends_with("-RRC") {
        Some(0.35f32)
    } else {
        None
    };

    // Apply matched RRC RX filter for -RRC modes.
    // For RRC: downmix to baseband I/Q first, then apply the RRC as a low-pass
    // matched filter.  (Applying the baseband RRC to the passband signal would
    // place fc far outside the filter passband and attenuate the signal to ~0.)
    let (i_syms, q_syms) = if let Some(alpha) = rrc_alpha {
        if samples.len() < n * (PREAMBLE_SYMS + 1) {
            return Err(ModemError::Demodulation("signal too short".into()));
        }
        bpsk_demodulate_rrc(samples, n, baud, fc, fs, alpha, &config.mode)
    } else {
        if samples.len() < n * (PREAMBLE_SYMS + 1) {
            return Err(ModemError::Demodulation("signal too short".into()));
        }
        let offset = find_timing_offset(samples, n, fc, fs);
        let (iv, qv) = demodulate_iq(samples, n, fc, fs, offset);
        (iv, qv)
    };

    if i_syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".into(),
        ));
    }

    // Differential phase detection (handles absolute-phase ambiguity).
    // We take consecutive (I,Q) pairs and compute Re(z[k] * conj(z[k-1])).
    // Positive → same phase → NRZI "0" (no flip); negative → "1" (flip).
    let data_syms_start = PREAMBLE_SYMS;
    let data_syms_end = i_syms.len() - TAIL_SYMS;

    if data_syms_start >= data_syms_end {
        return Ok(vec![]);
    }

    // Build the full range including the last preamble symbol as the reference
    // for the first data bit.
    let range_start = PREAMBLE_SYMS - 1; // include prev preamble symbol as reference
    let iq: Vec<(f32, f32)> = i_syms[range_start..data_syms_end]
        .iter()
        .zip(q_syms[range_start..data_syms_end].iter())
        .map(|(&i, &q)| (i, q))
        .collect();

    let bits = differential_decode(&iq);
    let bytes = bits_to_bytes(&bits);
    Ok(bytes)
}

// ── AFC frequency-offset estimator ───────────────────────────────────────────

/// Estimate the carrier frequency offset in Hz from demodulated IQ symbols.
///
/// Uses the IQ-squaring method: squaring each complex symbol removes the DBPSK
/// data modulation (since `2·φ_data ∈ {0, 2π}`), leaving a phasor that rotates
/// at `4π·Δf/baud_rate` radians per symbol.  The mean phase of consecutive
/// squared-symbol products then gives `Δf`.
///
/// **Tracking range:** `|Δf| ≤ baud_rate / 4`
/// - BPSK31:  ±7.8 Hz
/// - BPSK63:  ±15.6 Hz
/// - BPSK100: ±25 Hz
/// - BPSK250: ±62.5 Hz  ← covers the ±50 Hz spec at the widest BPSK mode
pub fn estimate_frequency_offset(i_syms: &[f32], q_syms: &[f32], baud_rate: f32) -> f32 {
    if i_syms.len() < 2 {
        return 0.0;
    }

    // z²[k] = (I[k]+jQ[k])² = (I²-Q²) + j(2IQ)
    let re2: Vec<f32> = i_syms
        .iter()
        .zip(q_syms.iter())
        .map(|(&i, &q)| i * i - q * q)
        .collect();
    let im2: Vec<f32> = i_syms
        .iter()
        .zip(q_syms.iter())
        .map(|(&i, &q)| 2.0 * i * q)
        .collect();

    // D[k] = z²[k] * conj(z²[k-1]); accumulate sum
    let mut re_sum = 0.0f32;
    let mut im_sum = 0.0f32;
    for k in 1..re2.len() {
        re_sum += re2[k] * re2[k - 1] + im2[k] * im2[k - 1];
        im_sum += im2[k] * re2[k - 1] - re2[k] * im2[k - 1];
    }

    // Δf = baud_rate * atan2(im, re) / (4π)
    im_sum.atan2(re_sum) * baud_rate / (4.0 * PI)
}

/// Run a lightweight demodulation pass to estimate the carrier frequency offset.
///
/// Returns `None` if the sample buffer is too short.
pub fn afc_estimate_hz(samples: &[f32], config: &ModulationConfig) -> Option<f32> {
    let baud = crate::parse_baud_rate(&config.mode).ok()?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = crate::modulate::samples_per_symbol(fs, baud).ok()?;

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return None;
    }

    let offset = find_timing_offset(samples, n, fc, fs);
    let (i_syms, q_syms) = demodulate_iq(samples, n, fc, fs, offset);
    Some(estimate_frequency_offset(&i_syms, &q_syms, baud))
}

/// Demodulate audio `samples` and return per-bit soft log-likelihood ratios.
///
/// Returns one `f32` per decoded bit, with **positive = bit more likely 0**.
/// Uses the differential-detection dot product directly (real part of
/// z[k] × conj(z[k−1])) as the soft value; positive dot → same phase → bit 0.
///
/// Returns one `f32` per decoded bit (positive = bit more likely 0).
///
/// For non-RRC modes: differential cross-correlation dot product on Hann-windowed symbols.
/// For RRC modes: same dot product applied to Gardner+LMS recovered symbols.
pub fn bpsk_demodulate_soft(
    samples: &[f32],
    config: &ModulationConfig,
) -> Result<Vec<f32>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return Err(ModemError::Demodulation("signal too short".into()));
    }

    let rrc_alpha = if let PulseShape::Rrc { alpha } = config.pulse_shape {
        Some(alpha)
    } else if config.mode.ends_with("-RRC") {
        Some(0.35f32)
    } else {
        None
    };

    let (i_syms, q_syms) = if let Some(alpha) = rrc_alpha {
        bpsk_demodulate_rrc(samples, n, baud, fc, fs, alpha, &config.mode)
    } else {
        let offset = find_timing_offset(samples, n, fc, fs);
        demodulate_iq(samples, n, fc, fs, offset)
    };

    if i_syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".into(),
        ));
    }

    let range_start = PREAMBLE_SYMS - 1;
    let data_syms_end = i_syms.len() - TAIL_SYMS;
    let iq: Vec<(f32, f32)> = i_syms[range_start..data_syms_end]
        .iter()
        .zip(q_syms[range_start..data_syms_end].iter())
        .map(|(&i, &q)| (i, q))
        .collect();

    // dot = Re(z[k] × conj(z[k-1])) = i1*i0 + q1*q0
    // Positive → same phase → NRZI "0" → bit 0 → LLR > 0 ✓
    let llrs = iq
        .windows(2)
        .map(|w| {
            let (i0, q0) = w[0];
            let (i1, q1) = w[1];
            i1 * i0 + q1 * q0
        })
        .collect();
    Ok(llrs)
}

/// GPU-accelerated demodulation path.
#[cfg(feature = "gpu")]
pub fn bpsk_demodulate_with_gpu(
    samples: &[f32],
    config: &ModulationConfig,
    ctx: &openpulse_gpu::GpuContext,
) -> Result<Vec<u8>, ModemError> {
    // RRC path requires FIR filtering; fall back to CPU.
    if matches!(config.pulse_shape, PulseShape::Rrc { .. }) || config.mode.ends_with("-RRC") {
        return bpsk_demodulate(samples, config);
    }

    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return Err(ModemError::Demodulation("signal too short".into()));
    }

    let expected = expected_preamble_symbols(PREAMBLE_SYMS);
    let offset = match openpulse_gpu::timing_offset_search_gpu(
        ctx,
        samples,
        n,
        PREAMBLE_SYMS,
        &expected,
        fc,
        fs,
    ) {
        Some(o) => o,
        None => return bpsk_demodulate(samples, config),
    };

    let effective = &samples[offset.min(samples.len())..];
    let (i_syms, q_syms) = match openpulse_gpu::bpsk_iq_demod_gpu(ctx, effective, n, fc, fs, offset)
    {
        Some(iq) => iq,
        None => return bpsk_demodulate(samples, config),
    };

    if i_syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".into(),
        ));
    }

    let data_syms_end = i_syms.len() - TAIL_SYMS;
    if PREAMBLE_SYMS >= data_syms_end {
        return Ok(vec![]);
    }

    let range_start = PREAMBLE_SYMS - 1;
    let iq: Vec<(f32, f32)> = i_syms[range_start..data_syms_end]
        .iter()
        .zip(q_syms[range_start..data_syms_end].iter())
        .map(|(&i, &q)| (i, q))
        .collect();

    let bits = differential_decode(&iq);
    Ok(bits_to_bytes(&bits))
}

// ── RRC baseband demodulation path ───────────────────────────────────────────

/// Full RRC demodulation: downmix → matched RRC filter → timing → sample.
///
/// The RRC filter is a low-pass (baseband) filter.  It must be applied AFTER
/// downmixing to baseband, not directly to the bandpass signal.
fn bpsk_demodulate_rrc(
    samples: &[f32],
    n: usize,
    baud: f32,
    fc: f32,
    fs: f32,
    alpha: f32,
    mode: &str,
) -> (Vec<f32>, Vec<f32>) {
    let two_pi = 2.0 * PI;
    let num_taps = RRC_SPAN_SYMBOLS * n + 1;
    let coeffs = generate_rrc_coefficients(fs, baud, alpha, num_taps);
    let group_delay = (num_taps - 1) / 2;

    // 1. Downmix to baseband I and Q (factor of 2 compensates the carrier ½).
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

    // 2. Apply RRC matched filter with group delay compensation to each channel.
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

    // 4. Adaptive timing recovery via Gardner detector starting from the acquired offset.
    let (i_out, q_out) = gardner_sample_rrc(&i_bb, &q_bb, n, initial_timing);

    // 5. LMS equalizer: train on the known preamble symbols, then decision-directed.
    // RRC path: DFE enabled for BPSK250 to handle multipath ISI.
    let (i_eq, q_eq) = bpsk_lms_equalize(&i_out, &q_out, mode);

    (i_eq, q_eq)
}

/// Select the LMS tap/step profile for a given mode.
///
/// BPSK250 has a 4 ms/symbol period — short enough that Watterson Moderate/Poor
/// delay spread (0.5–3 ms) produces multi-symbol ISI.  A 9-tap feedforward
/// plus 2-tap DFE with a tighter step gives better convergence on the
/// RRC+Gardner path where multipath ISI is the dominant impairment.
/// Narrow-band HF modes (BPSK31/63/100) have symbol periods ≥ 10 ms and are
/// inherently ISI-immune at typical HF delay spreads; the baseline 7-tap
/// equalizer is sufficient.
fn lms_profile(mode: &str) -> (usize, usize, f32) {
    if mode.contains("250") {
        (9, 2, 0.015)
    } else {
        (7, 0, 0.02)
    }
}

/// Apply a mode-aware LMS equalizer to BPSK symbol-rate I/Q.
///
/// Trains on the first `PREAMBLE_SYMS` samples using the known preamble
/// sequence, then switches to decision-directed mode.  Called only from the
/// RRC+Gardner path; the Hann-windowed non-RRC path does not apply LMS
/// (the integration already suppresses ISI and LMS decision-directed mode
/// degrades fading-channel performance).
fn bpsk_lms_equalize(i_syms: &[f32], q_syms: &[f32], mode: &str) -> (Vec<f32>, Vec<f32>) {
    let training = expected_preamble_symbols(PREAMBLE_SYMS.min(i_syms.len()));
    let training_q = vec![0.0f32; training.len()];
    let (fwd_len, dfe_len, mu) = lms_profile(mode);
    let mut eq = LmsEqualizer::new(fwd_len, dfe_len, mu);
    eq.process_frame(i_syms, q_syms, &training, &training_q, |i, _q| {
        (if i >= 0.0 { 1.0 } else { -1.0 }, 0.0)
    })
}

// ── Timing search ─────────────────────────────────────────────────────────────

/// Adaptive symbol sampling using the Gardner timing error detector.
///
/// `initial_timing` seeds the start position from brute-force preamble
/// correlation.  The Gardner loop then tracks timing drift adaptively for
/// the remainder of the frame.
fn gardner_sample_rrc(
    i_bb: &[f32],
    q_bb: &[f32],
    n: usize,
    initial_timing: usize,
) -> (Vec<f32>, Vec<f32>) {
    let start = initial_timing.min(i_bb.len());
    let mut det = GardnerDetector::new(n, 0.02);
    // Pre-arm so the first sample at `start` (already an ISI-free point) is output immediately.
    det.pre_arm();
    let mut i_out = Vec::new();
    let mut q_out = Vec::new();
    for (idx, &s_i) in i_bb[start..].iter().enumerate() {
        if det.update(s_i).is_some() {
            // Strobe fires: s_i is the boundary sample; Q is synchronous (same FIR).
            let s_q = q_bb.get(start + idx).copied().unwrap_or(0.0);
            i_out.push(s_i);
            q_out.push(s_q);
        }
    }
    (i_out, q_out)
}

/// Brute-force timing search on the baseband I signal (after downmix + RRC).
///
/// Tries every offset in 0..n, samples the baseband I at positions
/// `offset + k*n`, and correlates with the expected preamble pattern.
fn find_timing_offset_bb(i_bb: &[f32], n: usize) -> usize {
    let expected = expected_preamble_symbols(PREAMBLE_SYMS);
    let mut best_off = 0usize;
    let mut best_score = f32::NEG_INFINITY;

    for off in 0..n {
        if i_bb.len() < off + n * PREAMBLE_SYMS {
            break;
        }
        let score: f32 = (0..PREAMBLE_SYMS)
            .map(|s| i_bb[off + s * n] * expected[s])
            .sum();
        if score > best_score {
            best_score = score;
            best_off = off;
        }
    }
    best_off
}

/// Try every possible timing offset within one symbol period.  Return the
/// offset that gives the maximum preamble energy.
fn find_timing_offset(samples: &[f32], n: usize, fc: f32, fs: f32) -> usize {
    let mut best_energy = f32::NEG_INFINITY;
    let mut best_offset = 0usize;

    for offset in 0..n {
        if samples.len() < offset + n * PREAMBLE_SYMS {
            break;
        }
        let (i_syms, _) = demodulate_iq(&samples[offset..], n, fc, fs, 0);
        if i_syms.len() < PREAMBLE_SYMS {
            continue;
        }

        // Correlate the first PREAMBLE_SYMS I values with the expected
        // alternating pattern (+1, −1, +1, −1, …) that NRZI-encoding the
        // preamble bits produces.
        let expected = expected_preamble_symbols(PREAMBLE_SYMS);
        let energy: f32 = i_syms[..PREAMBLE_SYMS]
            .iter()
            .zip(expected.iter())
            .map(|(&s, &e)| s * e)
            .sum();

        if energy > best_energy {
            best_energy = energy;
            best_offset = offset;
        }
    }

    best_offset
}

/// Build the expected I-channel amplitudes for the preamble.
fn expected_preamble_symbols(len: usize) -> Vec<f32> {
    // The preamble bits are 1,0,1,0,… → NRZI gives phase_neg = T,T,F,F,T,T,…
    // but we want the raw alternating for correlation: +1,−1,+1,−1,…
    // Actually NRZI(1,0,1,0,…):
    //   bit1: flip → phase_neg=true  → amplitude −1
    //   bit0: keep → phase_neg=true  → amplitude −1
    //   bit1: flip → phase_neg=false → amplitude +1
    //   bit0: keep → phase_neg=false → amplitude +1
    // → pattern: −1,−1,+1,+1,−1,−1,+1,+1,…
    // Pre-compute this via nrzi_encode.
    let bits: Vec<bool> = (0..len).map(|i| i % 2 == 0).collect();
    let phases = nrzi_encode(&bits);
    phases
        .iter()
        .map(|&neg| if neg { -1.0f32 } else { 1.0f32 })
        .collect()
}

// ── IQ demodulation ───────────────────────────────────────────────────────────

/// Mix `samples` with I and Q reference carriers, apply the matched filter
/// (half-Hann w_tail, 1→0) and integrate over each symbol period.
///
/// Returns `(i_values, q_values)` — one value per symbol.
fn demodulate_iq(
    samples: &[f32],
    n: usize,
    fc: f32,
    fs: f32,
    offset: usize,
) -> (Vec<f32>, Vec<f32>) {
    let effective = &samples[offset.min(samples.len())..];
    let n_syms = effective.len() / n;
    let two_pi = 2.0 * PI;

    let mut i_out = Vec::with_capacity(n_syms);
    let mut q_out = Vec::with_capacity(n_syms);

    for sym_idx in 0..n_syms {
        let sym_start = sym_idx * n;
        let mut i_sum = 0.0f32;
        let mut q_sum = 0.0f32;
        let mut norm = 0.0f32;

        for i in 0..n {
            let global_n = (offset + sym_start + i) as f32;
            let sample = effective[sym_start + i];

            // Matched filter for the overlapping half-Hann modulator: the
            // decreasing half (w_tail = 1→0) correlates with the current
            // symbol's tail and is approximately orthogonal to the next
            // symbol's rising head, keeping ISI below the decision threshold.
            let window = 0.5 * (1.0 + (PI * i as f32 / n as f32).cos());

            let t = global_n / fs;
            let ci = (two_pi * fc * t).cos();
            let cq = -(two_pi * fc * t).sin();

            // Factor-of-2 compensates for the ½ in the carrier product.
            i_sum += sample * ci * window * 2.0;
            q_sum += sample * cq * window * 2.0;
            norm += window * window;
        }

        if norm > 1e-9 {
            i_sum /= norm;
            q_sum /= norm;
        }

        i_out.push(i_sum);
        q_out.push(q_sum);
    }

    (i_out, q_out)
}

// ── Differential phase detection (NRZI decode) ───────────────────────────────
///
/// `Re(z[k] * conj(z[k−1]))` is positive when the phase is the same
/// ("0" bit / no flip) and negative when the phase has flipped ("1" bit).
fn differential_decode(iq: &[(f32, f32)]) -> Vec<bool> {
    iq.windows(2)
        .map(|w| {
            let (i0, q0) = w[0];
            let (i1, q1) = w[1];
            // Real part of z1 * conj(z0) = i1*i0 + q1*q0
            let dot = i1 * i0 + q1 * q0;
            dot < 0.0 // negative → phase flipped → bit "1"
        })
        .collect()
}

// ── Bit/byte helpers ──────────────────────────────────────────────────────────

/// Pack LSB-first bits into bytes.  Trailing incomplete bytes are zero-padded.
pub(crate) fn bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |acc, (i, &b)| acc | ((b as u8) << i))
        })
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modulate::bytes_to_bits;

    #[test]
    fn bits_to_bytes_round_trip() {
        let original = b"Hello";
        let bits = bytes_to_bits(original);
        let back = bits_to_bytes(&bits);
        assert_eq!(&back[..original.len()], original);
    }

    #[test]
    fn differential_decode_same_phase() {
        // All same phase → all 0 bits
        let iq: Vec<(f32, f32)> = vec![(1.0, 0.0); 9];
        let bits = differential_decode(&iq);
        assert!(bits.iter().all(|&b| !b));
    }

    #[test]
    fn differential_decode_alternating_phase() {
        // Alternating phases → alternating 1,0,1,0,...
        // Actually alternating +1/-1 means:
        // (1,0),(−1,0): dot=−1 → 1
        // (−1,0),(1,0): dot=−1 → 1
        // all 1s
        let iq: Vec<(f32, f32)> = (0..9)
            .map(|i| (if i % 2 == 0 { 1.0 } else { -1.0 }, 0.0))
            .collect();
        let bits = differential_decode(&iq);
        assert!(bits.iter().all(|&b| b));
    }

    #[test]
    fn loopback_round_trip_bpsk250_non_rrc() {
        // Regression guard: BPSK250 non-RRC (Hann) path must round-trip cleanly.
        // This path does NOT apply LMS equalization (Hann integration is sufficient;
        // LMS decision-directed degrades fading-channel FEC performance).
        use crate::modulate::bpsk_modulate;
        let cfg = ModulationConfig {
            mode: "BPSK250".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            ..ModulationConfig::default()
        };
        let original = b"OpenPulseHF";
        let samples = bpsk_modulate(original, &cfg).unwrap();
        let recovered = bpsk_demodulate(&samples, &cfg).unwrap();
        assert!(
            recovered.len() >= original.len(),
            "Recovered {} bytes, expected at least {}",
            recovered.len(),
            original.len()
        );
        assert_eq!(
            &recovered[..original.len()],
            original,
            "BPSK250 non-RRC clean loopback must recover payload exactly"
        );
    }

    #[test]
    fn loopback_round_trip() {
        use crate::modulate::bpsk_modulate;
        let cfg = ModulationConfig {
            mode: "BPSK100".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            ..ModulationConfig::default()
        };
        let original = b"AB";
        let samples = bpsk_modulate(original, &cfg).unwrap();
        let recovered = bpsk_demodulate(&samples, &cfg).unwrap();
        assert_eq!(&recovered[..original.len()], original);
    }

    #[test]
    fn afc_estimate_near_zero_for_matched_carrier() {
        use crate::modulate::bpsk_modulate;
        let cfg = ModulationConfig {
            mode: "BPSK250".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            ..ModulationConfig::default()
        };
        let samples = bpsk_modulate(b"HelloWorld", &cfg).unwrap();
        // Estimate AFC with the correct carrier — should be near zero.
        let offset = afc_estimate_hz(&samples, &cfg).expect("afc estimate");
        assert!(
            offset.abs() < 5.0,
            "expected near-zero AFC offset, got {offset:.2} Hz"
        );
    }

    #[test]
    fn afc_estimate_detects_known_offset() {
        use crate::modulate::bpsk_modulate;
        // Modulate at 1500 Hz, then estimate AFC with a 20 Hz lower reference.
        // The estimator should report ≈ +20 Hz (signal is above reference).
        let true_fc = 1500.0f32;
        let ref_fc = 1480.0f32;
        let cfg_tx = ModulationConfig {
            mode: "BPSK250".to_string(),
            sample_rate: 8000,
            center_frequency: true_fc,
            ..ModulationConfig::default()
        };
        let cfg_rx = ModulationConfig {
            mode: "BPSK250".to_string(),
            sample_rate: 8000,
            center_frequency: ref_fc,
            ..ModulationConfig::default()
        };
        let samples = bpsk_modulate(b"HelloWorld", &cfg_tx).unwrap();
        let offset = afc_estimate_hz(&samples, &cfg_rx).expect("afc estimate");
        // Allow ±8 Hz tolerance (estimator range is baud/4 = 62.5 Hz for BPSK250).
        assert!(
            (offset - 20.0).abs() < 8.0,
            "expected ≈+20 Hz AFC offset, got {offset:.2} Hz"
        );
    }

    #[cfg(feature = "gpu")]
    #[test]
    fn gpu_demodulate_matches_cpu() {
        use crate::modulate::bpsk_modulate;
        let cfg = ModulationConfig {
            mode: "BPSK250".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            ..ModulationConfig::default()
        };
        let payload = b"AB";
        let samples = bpsk_modulate(payload, &cfg).unwrap();

        let cpu_out = bpsk_demodulate(&samples, &cfg).unwrap();

        let Some(ctx) = openpulse_gpu::GpuContext::init() else {
            eprintln!("skipping gpu_demodulate_matches_cpu: no compatible adapter");
            return;
        };
        let gpu_out = bpsk_demodulate_with_gpu(&samples, &cfg, &ctx).unwrap();

        assert_eq!(
            &cpu_out[..payload.len()],
            payload,
            "CPU path should recover payload"
        );
        assert_eq!(cpu_out, gpu_out, "GPU demodulation must match CPU output");
    }

    // ── LMS profile and Watterson channel stress tests ──────────────────────

    #[test]
    fn lms_profile_bpsk250_uses_dfe() {
        let (fwd, dfe, mu) = lms_profile("BPSK250");
        assert_eq!(fwd, 9);
        assert_eq!(dfe, 2);
        assert!(mu < 0.02, "BPSK250 mu should be tighter than baseline");
    }

    #[test]
    fn lms_profile_narrow_modes_use_baseline() {
        for mode in ["BPSK31", "BPSK63", "BPSK100"] {
            let (fwd, dfe, mu) = lms_profile(mode);
            assert_eq!(fwd, 7, "{mode}: expect 7-tap fwd");
            assert_eq!(dfe, 0, "{mode}: expect no DFE");
            assert!((mu - 0.02).abs() < 1e-6, "{mode}: expect mu=0.02");
        }
    }

    #[test]
    fn bpsk250_watterson_moderate_f1_decode_coverage() {
        use crate::modulate::bpsk_modulate;
        use openpulse_channel::watterson::WattersonChannel;
        use openpulse_channel::{ChannelModel, WattersonConfig};

        let cfg = ModulationConfig {
            mode: "BPSK250".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            ..ModulationConfig::default()
        };
        let payload: Vec<u8> = (0..96u8).map(|v| v ^ 0x5A).collect();
        let tx = bpsk_modulate(&payload, &cfg).expect("modulate");

        let bit_error_rate = |expected: &[u8], got: &[u8]| -> f32 {
            let n = expected.len().min(got.len());
            if n == 0 {
                return 1.0;
            }
            let bit_errors: u32 = expected
                .iter()
                .zip(got.iter())
                .take(n)
                .map(|(&a, &b)| (a ^ b).count_ones())
                .sum();
            bit_errors as f32 / (n as f32 * 8.0)
        };

        let mut decoded = 0usize;
        let mut good_ber = 0usize;
        let mut best_ber = f32::INFINITY;
        for seed in [
            0x6101u64, 0x6102, 0x6103, 0x6104, 0x6105, 0x6106, 0x6107, 0x6108,
        ] {
            let mut ch = WattersonChannel::new(WattersonConfig::moderate_f1(Some(seed)))
                .expect("watterson moderate f1");
            let rx = ch.apply(&tx);
            if let Ok(recovered) = bpsk_demodulate(&rx, &cfg) {
                if recovered.len() >= payload.len() {
                    decoded += 1;
                    let ber = bit_error_rate(&payload, &recovered[..payload.len()]);
                    best_ber = best_ber.min(ber);
                    if ber <= 0.12 {
                        good_ber += 1;
                    }
                }
            }
        }

        assert!(
            decoded >= 6,
            "BPSK250 moderate_f1 should decode payload length in most trials, decoded={decoded}/8"
        );
        assert!(
            good_ber >= 2,
            "BPSK250 moderate_f1 should include at least two low-BER decodes, good_ber={good_ber}/8, best_ber={best_ber:.3}"
        );
    }

    #[test]
    fn bpsk250_watterson_poor_f1_decode_presence() {
        use crate::modulate::bpsk_modulate;
        use openpulse_channel::watterson::WattersonChannel;
        use openpulse_channel::{ChannelModel, WattersonConfig};

        let cfg = ModulationConfig {
            mode: "BPSK250".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            ..ModulationConfig::default()
        };
        let payload: Vec<u8> = (0..96u8).collect();
        let tx = bpsk_modulate(&payload, &cfg).expect("modulate");

        let mut decoded = 0usize;
        let mut best_ber = f32::INFINITY;
        for seed in [0x6201u64, 0x6202, 0x6203, 0x6204, 0x6205, 0x6206] {
            let mut ch = WattersonChannel::new(WattersonConfig::poor_f1(Some(seed)))
                .expect("watterson poor f1");
            let rx = ch.apply(&tx);
            if let Ok(recovered) = bpsk_demodulate(&rx, &cfg) {
                if recovered.len() >= payload.len() {
                    decoded += 1;
                    let ber: f32 = payload
                        .iter()
                        .zip(recovered.iter())
                        .take(payload.len())
                        .map(|(&a, &b)| (a ^ b).count_ones() as f32)
                        .sum::<f32>()
                        / (payload.len() as f32 * 8.0);
                    best_ber = best_ber.min(ber);
                }
            }
        }

        assert!(
            decoded >= 1,
            "BPSK250 poor_f1 should produce at least one full-length decode, decoded={decoded}/6"
        );
        assert!(
            best_ber < 0.5,
            "BPSK250 poor_f1 best BER must beat random (0.5), got best_ber={best_ber:.3}"
        );
    }
}
