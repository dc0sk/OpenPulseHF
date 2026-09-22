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

use num_complex::Complex32;
use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, PulseShape};
use openpulse_dsp::acquisition::goertzel_carrier_scan;
use openpulse_dsp::constellation::{additive_snr_db_windowed, differential_llr_scale};
use openpulse_dsp::equalizer::LmsEqualizer;
use openpulse_dsp::farrow::FarrowTimingLoop;
use openpulse_dsp::filter::FirFilter;
use openpulse_dsp::pll::CarrierPll;
use openpulse_dsp::rrc::generate_rrc_coefficients;

use crate::modulate::{
    nrzi_encode, samples_per_symbol, PREAMBLE_SYMS, RRC_SPAN_SYMBOLS, TAIL_SYMS,
};
use crate::parse_baud_rate;

// ── Public entry point ────────────────────────────────────────────────────────

/// Demodulate audio `samples` and return the recovered bytes.
/// The symbol stream the decoder sees: matched-filtered, timing-recovered baseband I/Q.
///
/// Shared by `bpsk_demodulate` and `estimate_snr_db` so the SNR is measured on exactly the symbols
/// that were decoded, not on a separately-derived approximation.
fn symbol_stream(
    samples: &[f32],
    config: &ModulationConfig,
) -> Result<(Vec<f32>, Vec<f32>), ModemError> {
    symbol_stream_with_expected(samples, config, &expected_preamble_symbols(PREAMBLE_SYMS))
}

/// [`symbol_stream`] whose timing lock uses a supplied expectation.
///
/// The `-RRC` path is **not** parameterised: it locks timing with Gardner+LMS
/// rather than a preamble correlation, so a candidate sequence reaches it only
/// through training, which this seam does not cover.
fn symbol_stream_with_expected(
    samples: &[f32],
    config: &ModulationConfig,
    expected: &[f32],
) -> Result<(Vec<f32>, Vec<f32>), ModemError> {
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
    if samples.len() < n * (expected.len() + 1) {
        return Err(ModemError::Demodulation("signal too short".into()));
    }
    // Apply matched RRC RX filter for -RRC modes.
    // For RRC: downmix to baseband I/Q first, then apply the RRC as a low-pass
    // matched filter.  (Applying the baseband RRC to the passband signal would
    // place fc far outside the filter passband and attenuate the signal to ~0.)
    if let Some(alpha) = rrc_alpha {
        // The RRC path locks timing with Gardner+LMS and trains on the SHIPPED
        // sequence, so it cannot honour a candidate expectation. Refuse loudly
        // rather than silently decode a different geometry — a doc comment here
        // would be exactly the fidelity-by-comment this seam exists to remove.
        if expected != expected_preamble_symbols(PREAMBLE_SYMS) {
            return Err(ModemError::Demodulation(
                "the -RRC path cannot honour a candidate preamble: its timing is \
                 Gardner+LMS trained on the shipped sequence"
                    .into(),
            ));
        }
        Ok(bpsk_demodulate_rrc(
            samples,
            n,
            baud,
            fc,
            fs,
            alpha,
            &config.mode,
        ))
    } else {
        let offset = find_timing_offset_with_expected(samples, n, fc, fs, expected);
        let (mut iv, mut qv) = demodulate_iq(samples, n, fc, fs, offset);
        // The overlapping half-Hann modulator is a crossfade, so the one-slot matched filter recovers
        // `r_k = a_k + β·a_{k+1}` (β = 1/3). Left in, that `+β` term adds a constant positive bias to the
        // differential dot product `r_k·r_{k-1}` (a_k²=1), eroding the flip-bit margin by several dB.
        // Cancel it here (crossfade path only; the -RRC path uses Gardner+LMS and does not crossfade).
        cancel_crossfade_isi(&mut iv, &mut qv);
        Ok((iv, qv))
    }
}

/// Absolute additive SNR (dB) of a received BPSK frame — the rate controller's input.
///
/// BPSK had **no** symbol-domain estimator, so the engine fell back to the waveform-blind M2M4
/// moment estimator. M2M4 assumes a constant-modulus envelope, which a fade destroys: on Watterson
/// `moderate_f1` it read a **flat ≈ −6.6 dB from 15 dB of true SNR upward** — no information at any
/// SNR. `hpx_hf`'s SL2–SL5 are all BPSK (SL2 is the rung every session starts on), so the controller
/// saw a sub-floor number on frames that decoded perfectly and fast-downshifted the link to the
/// bottom rung, delivering nothing on a routine HF fade (issue #934).
///
/// The fix is the same one `openpulse_channel::estimate_additive_snr_db` applies to raw audio:
/// remove the *multiplicative* channel with a per-window least-squares gain before measuring the
/// residual. BPSK is differentially decoded, so the transmitted ±1 sequence is reconstructed from the
/// decisions the decoder already made; its arbitrary global sign is absorbed by the per-window gain.
pub fn estimate_snr_db(samples: &[f32], config: &ModulationConfig) -> Option<f32> {
    let (i_syms, q_syms) = symbol_stream(samples, config).ok()?;
    if i_syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return None;
    }
    let range_start = PREAMBLE_SYMS - 1;
    let end = i_syms.len() - TAIL_SYMS;
    if range_start >= end {
        return None;
    }
    let rx: Vec<Complex32> = i_syms[range_start..end]
        .iter()
        .zip(q_syms[range_start..end].iter())
        .map(|(&i, &q)| Complex32::new(i, q))
        .collect();
    let iq: Vec<(f32, f32)> = rx.iter().map(|z| (z.re, z.im)).collect();
    let bits = differential_decode(&iq);
    // Rebuild the transmitted symbols from the differential decisions: each "1" flips the phase.
    // The starting sign is unknown and does not matter — the per-window LS gain absorbs it.
    let mut decisions = Vec::with_capacity(rx.len());
    let mut cur = Complex32::new(1.0, 0.0);
    decisions.push(cur);
    for &flip in &bits {
        if flip {
            cur = -cur;
        }
        decisions.push(cur);
    }
    const WINDOW_SYMS: usize = 16;
    let es_n0 = additive_snr_db_windowed(&rx, &decisions, WINDOW_SYMS);

    // Convert symbol-domain Es/N0 to the *channel* SNR scale the rate ladder's floors are written in.
    // The estimate is taken after the matched filter, so it carries the mode's processing gain — a
    // 31-baud rung reads ~17 dB above the channel SNR and a 250-baud rung ~8 dB. Left unconverted the
    // receiver over-recommends badly (a 2 dB AWGN channel drove the ladder to SL5), which is just the
    // #934 scale defect wearing a different hat: never compare one scale's number against another's.
    //
    // Measured on AWGN, the offset is `10·log10(fs/baud) − MATCHED_FILTER_LOSS_DB` and the constant
    // holds to ~0.3 dB across BPSK31 and BPSK250 — an order of magnitude apart in baud — so it is the
    // pulse's noise-bandwidth loss, not a per-mode fudge. It IS pulse-specific: it is fitted to this
    // plugin's Hann/crossfade matched filter. `bpsk_snr_awgn_scale_matches_channel_snr` pins it.
    const MATCHED_FILTER_LOSS_DB: f32 = 7.1;
    let baud = parse_baud_rate(&config.mode).ok()?;
    let fs = config.sample_rate as f32;
    let processing_gain_db = 10.0 * (fs / baud).log10();
    Some(es_n0 - processing_gain_db + MATCHED_FILTER_LOSS_DB)
}

pub fn bpsk_demodulate(samples: &[f32], config: &ModulationConfig) -> Result<Vec<u8>, ModemError> {
    bpsk_demodulate_with_expected(samples, config, &expected_preamble_symbols(PREAMBLE_SYMS))
}

/// [`bpsk_demodulate`] whose timing lock and framing use a supplied expectation.
///
/// Both the timing correlation and the preamble/data symbol boundaries follow
/// `expected.len()`, so a candidate preamble is decoded end-to-end on its own
/// geometry — the column that decides whether a candidate is viable at all.
pub fn bpsk_demodulate_with_expected(
    samples: &[f32],
    config: &ModulationConfig,
    expected: &[f32],
) -> Result<Vec<u8>, ModemError> {
    let preamble_syms = expected.len();
    // Front-end (matched filter + timing) lives in `symbol_stream`, shared with `estimate_snr_db`
    // so the SNR is measured on exactly the symbols that get decoded.
    let (i_syms, q_syms) = symbol_stream_with_expected(samples, config, expected)?;

    if i_syms.len() <= preamble_syms + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".into(),
        ));
    }

    // Differential phase detection (handles absolute-phase ambiguity).
    // We take consecutive (I,Q) pairs and compute Re(z[k] * conj(z[k-1])).
    // Positive → same phase → NRZI "0" (no flip); negative → "1" (flip).
    let data_syms_start = preamble_syms;
    let data_syms_end = i_syms.len() - TAIL_SYMS;

    if data_syms_start >= data_syms_end {
        return Ok(vec![]);
    }

    // Build the full range including the last preamble symbol as the reference
    // for the first data bit.
    let range_start = preamble_syms - 1; // include prev preamble symbol as reference
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

/// Wide-range carrier frequency estimator using the Goertzel algorithm on the
/// squared signal.
///
/// Squaring removes BPSK modulation, leaving a tone at 2×fc.  A Goertzel
/// search in 25 Hz steps over 2×fc ± 800 Hz (= fc ± 400 Hz at baseband)
/// locates the dominant peak.  **Acquisition range: ±400 Hz.**
fn estimate_carrier_hz_wide(samples: &[f32], config: &ModulationConfig) -> Option<f32> {
    let baud = crate::parse_baud_rate(&config.mode).ok()?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = crate::modulate::samples_per_symbol(fs, baud).ok()?;

    if samples.len() < n * PREAMBLE_SYMS {
        return None;
    }

    // Limit to 4× preamble length to keep per-call cost bounded.
    let window_len = (n * PREAMBLE_SYMS * 4).min(samples.len());
    goertzel_carrier_scan(&samples[..window_len], fs, fc, 2, 400.0, 12.5)
}

/// Run a lightweight demodulation pass to estimate the carrier frequency offset.
///
/// **Two-stage estimator:**
/// 1. Goertzel coarse search (±400 Hz, 12.5 Hz resolution at baseband) to handle
///    large initial offsets — e.g., VHF crystal errors of up to ±3 ppm on 144 MHz.
/// 2. IQ-squaring fine correction at the Goertzel-corrected centre frequency to
///    recover the sub-step residual (≤ 6.25 Hz) for accurate tracking.
///
/// Returns `None` if the buffer is too short for either path.
pub fn afc_estimate_hz(samples: &[f32], config: &ModulationConfig) -> Option<f32> {
    afc_estimate_hz_with_expected(samples, config, &expected_preamble_symbols(PREAMBLE_SYMS))
}

/// [`afc_estimate_hz`] whose stage-2 timing lock uses a supplied expectation.
///
/// Stage 2 calls [`find_timing_offset`], which correlates against the expected
/// preamble — so estimating AFC on candidate-preamble audio through the shipped
/// entry point measures a TX/RX mismatch, not the candidate.
pub fn afc_estimate_hz_with_expected(
    samples: &[f32],
    config: &ModulationConfig,
    expected: &[f32],
) -> Option<f32> {
    let baud = crate::parse_baud_rate(&config.mode).ok()?;
    let fs = config.sample_rate as f32;
    let n = crate::modulate::samples_per_symbol(fs, baud).ok()?;

    // Stage 1: coarse Goertzel acquisition (±400 Hz).
    let coarse = estimate_carrier_hz_wide(samples, config);

    if samples.len() < n * (expected.len() + 1) {
        return coarse;
    }

    // Stage 2: fine IQ-squaring at the Goertzel-corrected centre frequency to
    // eliminate the sub-step quantisation error (≤ 6.25 Hz).
    let c = coarse.unwrap_or(0.0);
    let corrected_fc = config.center_frequency + c;
    let offset = find_timing_offset_with_expected(samples, n, corrected_fc, fs, expected);
    let (i_syms, q_syms) = demodulate_iq(samples, n, corrected_fc, fs, offset);
    let residual = estimate_frequency_offset(&i_syms, &q_syms, baud);

    // The residual should be within ±baud/4 of 0 after Goertzel correction.
    // If it is, combine both stages; otherwise fall back to the coarse estimate.
    if residual.abs() < baud / 4.0 {
        Some(c + residual)
    } else {
        coarse
    }
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
        // NOTE: crossfade-ISI cancellation is deliberately NOT applied on the soft path. BPSK is
        // *differential*, so the backward-substitution recursion inflates the noise LLRs of a deeply
        // faded attempt instead of suppressing them — that breaks the LLR calibration HARQ MAP
        // combining relies on (regressed `llr_calibration::a_deeply_faded_extra_attempt_does_not_hurt`).
        // The cancellation stays on the hard differential path (`bpsk_demodulate`), where it restores the
        // decision margin without disturbing any soft-combining scale.
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
    let mut llrs: Vec<f32> = iq
        .windows(2)
        .map(|w| {
            let (i0, q0) = w[0];
            let (i1, q1) = w[1];
            i1 * i0 + q1 * q0
        })
        .collect();
    // cross = Im(z[k] × conj(z[k-1])): mean zero, so it carries only noise.
    let crosses: Vec<f32> = iq
        .windows(2)
        .map(|w| {
            let (i0, q0) = w[0];
            let (i1, q1) = w[1];
            q1 * i0 - i1 * q0
        })
        .collect();

    // Calibrate the soft values into *true* log-likelihood ratios. The target is the DBPSK LLR
    // slope `2A²/var(dot)`, NOT `1/σ²` — that is only its high-SNR limit, and holding it all the
    // way down is what let a signal-free attempt vote at full strength (#1364). Nothing that
    // decodes a single frame notices — soft Viterbi, min-sum LDPC and max-log turbo are all
    // scale-invariant — but HARQ soft combining across receive attempts does: uncalibrated, an attempt
    // from a deep fade votes as loudly as a clean one. See `openpulse_core::fec::combine_llrs_map`.
    let scale = differential_llr_scale(&llrs, &crosses);
    for l in llrs.iter_mut() {
        *l *= scale;
    }
    Ok(llrs)
}

/// GPU RRC demodulation: downmix on CPU, matched RRC filter on GPU, timing + LMS on CPU.
#[cfg(feature = "gpu")]
fn bpsk_demodulate_rrc_gpu(
    samples: &[f32],
    config: &ModulationConfig,
    ctx: &openpulse_gpu::GpuContext,
) -> Result<Vec<u8>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;
    let alpha = if let PulseShape::Rrc { alpha } = config.pulse_shape {
        alpha
    } else {
        0.35
    };

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return Err(ModemError::Demodulation("signal too short".into()));
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

    // GPU FIR: pad tail by group_delay, filter, then trim.
    let gpu_rrc = |mix: Vec<f32>| -> Option<Vec<f32>> {
        let padded: Vec<f32> = mix
            .iter()
            .copied()
            .chain(std::iter::repeat_n(0.0, group_delay))
            .collect();
        let filtered = openpulse_gpu::gpu_rrc_fir(ctx, &padded, &coeffs)?;
        Some(filtered[group_delay..].to_vec())
    };

    let (i_bb, q_bb) = match (gpu_rrc(i_mix), gpu_rrc(q_mix)) {
        (Some(i), Some(q)) => (i, q),
        // GPU error — fall back to CPU path.
        _ => return bpsk_demodulate(samples, config),
    };

    let initial_timing = find_timing_offset_bb(&i_bb, &q_bb, n);
    // De-rotate to the preamble carrier phase before LMS (see bpsk_demodulate_rrc).
    let phase_0 = coarse_baseband_phase(&i_bb, &q_bb, n, initial_timing);
    let (sin0, cos0) = (-phase_0).sin_cos();
    let (i_rot, q_rot): (Vec<f32>, Vec<f32>) = i_bb
        .iter()
        .zip(q_bb.iter())
        .map(|(&i, &q)| (i * cos0 - q * sin0, i * sin0 + q * cos0))
        .unzip();
    let (i_out, q_out) = gardner_sample_rrc(&i_rot, &q_rot, n, initial_timing);
    let (i_syms, q_syms) = bpsk_lms_equalize(&i_out, &q_out, &config.mode);

    if i_syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation(
            "no data symbols after preamble".into(),
        ));
    }

    let data_syms_end = i_syms.len() - TAIL_SYMS;
    let range_start = PREAMBLE_SYMS - 1;
    let iq: Vec<(f32, f32)> = i_syms[range_start..data_syms_end]
        .iter()
        .zip(q_syms[range_start..data_syms_end].iter())
        .map(|(&i, &q)| (i, q))
        .collect();

    let bits = differential_decode(&iq);
    Ok(bits_to_bytes(&bits))
}

/// GPU-accelerated demodulation path.
#[cfg(feature = "gpu")]
pub fn bpsk_demodulate_with_gpu(
    samples: &[f32],
    config: &ModulationConfig,
    ctx: &openpulse_gpu::GpuContext,
) -> Result<Vec<u8>, ModemError> {
    // RRC path: downmix on CPU, matched RRC filter on GPU, timing + LMS on CPU.
    if matches!(config.pulse_shape, PulseShape::Rrc { .. }) || config.mode.ends_with("-RRC") {
        return bpsk_demodulate_rrc_gpu(samples, config, ctx);
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
    let initial_timing = find_timing_offset_bb(&i_bb, &q_bb, n);

    // 3b. De-rotate the baseband to the preamble's carrier phase BEFORE the
    // LMS stage.  The equalizer trains against the REAL (±1, 0) preamble
    // targets; at a ~90° carrier phase the symbol energy lives in Q and 32
    // training symbols are not enough for the complex taps to converge from a
    // full quadrature rotation — decision-directed mode then destabilises.
    // The known preamble gives the phase directly (same pattern as 64QAM).
    let phase_0 = coarse_baseband_phase(&i_bb, &q_bb, n, initial_timing);
    let (sin0, cos0) = (-phase_0).sin_cos();
    let (i_rot, q_rot): (Vec<f32>, Vec<f32>) = i_bb
        .iter()
        .zip(q_bb.iter())
        .map(|(&i, &q)| (i * cos0 - q * sin0, i * sin0 + q * cos0))
        .unzip();

    // 4. Adaptive timing recovery via Gardner detector starting from the acquired offset.
    let (i_out, q_out) = gardner_sample_rrc(&i_rot, &q_rot, n, initial_timing);

    // 5. LMS equalizer: train on the known preamble symbols, then decision-directed.
    // RRC path: DFE enabled for BPSK250 to handle multipath ISI.
    let (i_eq, q_eq) = bpsk_lms_equalize(&i_out, &q_out, mode);

    (i_eq, q_eq)
}

/// Carrier phase of the preamble at `timing`: arg of the complex correlation
/// `Σ (i + jq)·e` against the real expected preamble amplitudes.
fn coarse_baseband_phase(i_bb: &[f32], q_bb: &[f32], n: usize, timing: usize) -> f32 {
    let expected = expected_preamble_symbols(PREAMBLE_SYMS);
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (s, &e) in expected.iter().enumerate() {
        let idx = timing + s * n;
        if idx >= i_bb.len() {
            break;
        }
        re += i_bb[idx] * e;
        im += q_bb.get(idx).copied().unwrap_or(0.0) * e;
    }
    im.atan2(re)
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
    // Train-then-freeze: unsupervised DD adaptation drifts to a wrong
    // self-consistent equilibrium (gain creep + Q contamination) over long
    // frames even on clean input; the residual carrier is already tracked by
    // the Costas PLL ahead of this stage, so frozen taps stay valid.
    let mut eq = LmsEqualizer::new(fwd_len, dfe_len, mu).with_frozen_dd();
    eq.process_frame(i_syms, q_syms, &training, &training_q, |i, _q| {
        (if i >= 0.0 { 1.0 } else { -1.0 }, 0.0)
    })
}

// ── Timing search ─────────────────────────────────────────────────────────────

/// Interpolating symbol sampling with true timing-drift tracking.
///
/// `initial_timing` seeds the start position from brute-force preamble
/// correlation; the Farrow loop then tracks fractional timing AND the actual
/// samples-per-symbol period for the remainder of the frame.  The previous
/// fixed-stride GardnerDetector could not adjust the sampling instant at all
/// (its mu clamp kept the strobe interval at exactly `n`), so a sound-card
/// sample-rate offset slid long frames into heavy ISI.
fn gardner_sample_rrc(
    i_bb: &[f32],
    q_bb: &[f32],
    n: usize,
    initial_timing: usize,
) -> (Vec<f32>, Vec<f32>) {
    let start = initial_timing.min(i_bb.len());
    // Stronger-than-default gains: at ≤ 250 baud the HF delay spread is
    // sub-symbol, so the multipath bias that forces the conservative default
    // gains on high-baud modes does not apply — and the long low-baud frames
    // need the loop to track 150 ppm with ≤ a few % of period lag.
    let (i_out, q_out) = FarrowTimingLoop::new(n)
        .with_gains(0.05, 0.002)
        .process(i_bb, q_bb, start);

    // Track residual carrier frequency with a BPSK Costas PLL.  A sample-rate
    // offset shifts the carrier too (150 ppm ⇒ −0.225 Hz at 1500 Hz), which
    // rotates the constellation ~90° per ~300 symbols at 250 baud — the LMS
    // stage trains on REAL (±1, 0) targets and cannot follow a rotation, so
    // without the PLL its decision-directed mode collapses mid-frame.  The
    // PLL's 180° ambiguity is harmless: decoding is differential.
    // Level-normalise before the Costas loop: its BPSK discriminant `q·sgn(i)` scales with the symbol
    // amplitude, so a quiet station's small symbols weaken the loop until it can't acquire a sub-deadband
    // residual offset. A no-op at nominal amplitude (RMS ≈ 1); a uniform scale, so decisions are unchanged.
    let ms = i_out
        .iter()
        .zip(q_out.iter())
        .map(|(&i, &q)| i * i + q * q)
        .sum::<f32>()
        / i_out.len().max(1) as f32;
    let k = if ms > 1e-18 { 1.0 / ms.sqrt() } else { 1.0 };

    let mut pll = CarrierPll::new(0.02, 1);
    let mut i_trk = Vec::with_capacity(i_out.len());
    let mut q_trk = Vec::with_capacity(q_out.len());
    for (&s_i, &s_q) in i_out.iter().zip(q_out.iter()) {
        let (s_i, s_q) = (s_i * k, s_q * k);
        pll.update(s_i, s_q);
        let (ci, cq) = pll.correct(s_i, s_q);
        i_trk.push(ci);
        q_trk.push(cq);
    }
    (i_trk, q_trk)
}

/// Brute-force timing search on the baseband I/Q signal (after downmix + RRC).
///
/// Tries every offset in 0..n, samples the baseband at positions
/// `offset + k*n`, and correlates with the expected preamble pattern using the
/// carrier-phase-invariant squared magnitude `(Σ I·e)² + (Σ Q·e)²`.  The old
/// I-only signed correlation collapsed at a ~90° carrier phase (preamble
/// energy entirely in Q) and was additionally polarity-sensitive — the same
/// bug class fixed in QPSK/8PSK/SCFDMA/OFDM.
fn find_timing_offset_bb(i_bb: &[f32], q_bb: &[f32], n: usize) -> usize {
    let expected = expected_preamble_symbols(PREAMBLE_SYMS);
    let mut best_off = 0usize;
    let mut best_score = f32::NEG_INFINITY;

    for off in 0..n {
        if i_bb.len() < off + n * PREAMBLE_SYMS {
            break;
        }
        let (re, im) = (0..PREAMBLE_SYMS).fold((0.0f32, 0.0f32), |(re, im), s| {
            let idx = off + s * n;
            (
                re + i_bb[idx] * expected[s],
                im + q_bb.get(idx).copied().unwrap_or(0.0) * expected[s],
            )
        });
        let score = re * re + im * im;
        if score > best_score {
            best_score = score;
            best_off = off;
        }
    }
    best_off
}

/// Try every possible timing offset within one symbol period.  Return the
/// offset that gives the maximum preamble correlation magnitude.
fn find_timing_offset(samples: &[f32], n: usize, fc: f32, fs: f32) -> usize {
    find_timing_offset_with_expected(
        samples,
        n,
        fc,
        fs,
        &expected_preamble_symbols(PREAMBLE_SYMS),
    )
}

/// [`find_timing_offset`] correlating against a supplied expectation.
///
/// The search span follows `expected.len()` rather than [`PREAMBLE_SYMS`], so a
/// candidate preamble is searched over its own length.
pub fn find_timing_offset_with_expected(
    samples: &[f32],
    n: usize,
    fc: f32,
    fs: f32,
    expected: &[f32],
) -> usize {
    let mut best_energy = f32::NEG_INFINITY;
    let mut best_offset = 0usize;
    let syms = expected.len();

    for offset in 0..n {
        if samples.len() < offset + n * syms {
            break;
        }
        // Demodulate ONLY the preamble span at this offset (the slice may be
        // multi-second; demodulating all of it per offset is O(offsets × N)).
        let span_end = (offset + n * syms).min(samples.len());
        let (i_syms, q_syms) = demodulate_iq(&samples[..span_end], n, fc, fs, offset);
        if i_syms.len() < syms {
            continue;
        }

        // Correlate the first PREAMBLE_SYMS symbols with the expected
        // alternating pattern that NRZI-encoding the preamble bits produces,
        // using the carrier-phase-invariant magnitude (Σ I·e)² + (Σ Q·e)².
        // The previous |Σ I·e| handled the 180° polarity ambiguity but
        // collapsed at a ~90° carrier phase where the preamble energy lives in
        // Q.  The differential BPSK decoder handles polarity after timing lock.
        let (re, im) = i_syms[..syms]
            .iter()
            .zip(q_syms[..syms].iter())
            .zip(expected.iter())
            .fold((0.0f32, 0.0f32), |(re, im), ((&i, &q), &e)| {
                (re + i * e, im + q * e)
            });
        let energy = re * re + im * im;

        if energy > best_energy {
            best_energy = energy;
            best_offset = offset;
        }
    }

    best_offset
}

/// Build the expected I-channel amplitudes for the preamble.
pub fn expected_preamble_symbols(len: usize) -> Vec<f32> {
    // The preamble bits are 1,0,1,0,… → NRZI gives phase_neg = T,T,F,F,T,T,…
    // but we want the raw alternating for correlation: +1,−1,+1,−1,…
    // Actually NRZI(1,0,1,0,…):
    //   bit1: flip → phase_neg=true  → amplitude −1
    //   bit0: keep → phase_neg=true  → amplitude −1
    //   bit1: flip → phase_neg=false → amplitude +1
    //   bit0: keep → phase_neg=false → amplitude +1
    // → pattern: −1,−1,+1,+1,−1,−1,+1,+1,…
    // Pre-compute this via nrzi_encode.
    expected_symbols_for(&crate::modulate::preamble_bits(len))
}

/// Build the expected I-channel amplitudes for an arbitrary preamble bit pattern.
pub fn expected_symbols_for(bits: &[bool]) -> Vec<f32> {
    let phases = nrzi_encode(bits);
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

/// Crossfade-ISI coefficient for the overlapping half-Hann pulse: the one-slot matched filter recovers
/// `r_k = a_k + β·a_{k+1}` where `β = Σ(w_head·w_tail)/Σw_tail² = 1/3` (same integrals as rectangular QPSK).
const CROSSFADE_ISI_BETA: f32 = 1.0 / 3.0;

/// Remove the crossfade ISI from the recovered symbol stream in place by stable backward substitution:
/// `r_k = a_k + β·a_{k+1}` is bidiagonal, so `a_k = r_k − β·a_{k+1}`. The tail symbols (successor→0) give
/// the terminal; noise is amplified by only `1/(1−β²) = 1.125` (+0.5 dB), far less than the several-dB
/// differential-margin loss the uncancelled `+β` bias costs. Crossfade (non-RRC) path only.
fn cancel_crossfade_isi(i_syms: &mut [f32], q_syms: &mut [f32]) {
    let beta = CROSSFADE_ISI_BETA;
    let n = i_syms.len().min(q_syms.len());
    for k in (0..n.saturating_sub(1)).rev() {
        i_syms[k] -= beta * i_syms[k + 1];
        q_syms[k] -= beta * q_syms[k + 1];
    }
}

// ── Differential phase detection (NRZI decode) ───────────────────────────────
/// Decode bits from consecutive complex (I, Q) symbol pairs.
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

    /// Timing lock at a 90° carrier phase: with the old I-only correlation the
    /// preamble energy lives entirely in Q at this phase, the metric collapses
    /// to ~0 for every offset, and the search picks an arbitrary (wrong)
    /// timing.  fc=2000 Hz at fs=8000 gives a carrier phase step of exactly
    /// π/2 per sample, so one prepended silent sample puts the preamble start
    /// at carrier phase 90°.
    #[test]
    fn bpsk250_timing_correct_at_carrier_90_degrees() {
        use crate::modulate::bpsk_modulate;
        let fc = 2000.0f32;
        let cfg = ModulationConfig {
            mode: "BPSK250".to_string(),
            sample_rate: 8000,
            center_frequency: fc,
            ..ModulationConfig::default()
        };
        let payload: Vec<u8> = (0u8..64).collect();
        let signal = bpsk_modulate(&payload, &cfg).unwrap();
        let mut samples = vec![0.0f32; 1];
        samples.extend_from_slice(&signal);

        let recovered = bpsk_demodulate(&samples, &cfg).unwrap();
        assert_eq!(&recovered[..payload.len()], &payload[..]);
    }

    /// Same carrier-90° regression for the RRC path, whose old baseband search
    /// was additionally polarity-sensitive (signed I-only score, no abs).
    #[test]
    fn bpsk250_rrc_timing_correct_at_carrier_90_degrees() {
        use crate::modulate::bpsk_modulate;
        let fc = 2000.0f32;
        let cfg = ModulationConfig {
            mode: "BPSK250-RRC".to_string(),
            sample_rate: 8000,
            center_frequency: fc,
            ..ModulationConfig::default()
        };
        let payload: Vec<u8> = (0u8..64).collect();
        let signal = bpsk_modulate(&payload, &cfg).unwrap();
        let mut samples = vec![0.0f32; 1];
        samples.extend_from_slice(&signal);

        let recovered = bpsk_demodulate(&samples, &cfg).unwrap();
        assert_eq!(&recovered[..payload.len()], &payload[..]);
    }

    /// Sample-rate-offset acceptance (review A13): a full 255-byte BPSK250-RRC
    /// frame decodes through a 150 ppm resampling — the two-free-running-
    /// sound-card condition.  The frame is ~66k samples, so 150 ppm drifts the
    /// ISI-free sampling instant by ~10 samples (31% of the 32-sample symbol
    /// period); the fixed-stride timing path cannot decode this.
    #[test]
    fn bpsk250_rrc_decodes_through_150ppm_sample_rate_offset() {
        use crate::modulate::bpsk_modulate;
        let cfg = ModulationConfig {
            mode: "BPSK250-RRC".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            ..ModulationConfig::default()
        };
        let payload: Vec<u8> = (0..255u8).collect();
        let tx = bpsk_modulate(&payload, &cfg).unwrap();

        // Linear resampling models the receiver clock running 150 ppm fast.
        let ratio = 1.0f64 + 150e-6;
        let out_len = (tx.len() as f64 / ratio) as usize;
        let rx: Vec<f32> = (0..out_len)
            .map(|k| {
                let pos = k as f64 * ratio;
                let base = pos.floor() as usize;
                let t = (pos - base as f64) as f32;
                if base + 1 < tx.len() {
                    tx[base] * (1.0 - t) + tx[base + 1] * t
                } else {
                    tx[tx.len() - 1]
                }
            })
            .collect();

        let recovered = bpsk_demodulate(&rx, &cfg).unwrap();
        assert!(
            recovered.len() >= payload.len(),
            "recovered only {} bytes",
            recovered.len()
        );
        let first_bad = recovered[..payload.len()]
            .iter()
            .zip(payload.iter())
            .position(|(a, b)| a != b);
        let n_bad = recovered[..payload.len()]
            .iter()
            .zip(payload.iter())
            .filter(|(a, b)| a != b)
            .count();
        let bit_errs: u32 = recovered[..payload.len()]
            .iter()
            .zip(payload.iter())
            .map(|(a, b)| (a ^ b).count_ones())
            .sum();
        assert_eq!(
            &recovered[..payload.len()],
            &payload[..],
            "BPSK250-RRC 150 ppm SRO: first mismatch byte {first_bad:?}, {n_bad}/255 bad bytes, {bit_errs} bit errors"
        );
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

    #[test]
    fn afc_wide_detects_large_offset() {
        use crate::modulate::bpsk_modulate;
        // 150 Hz offset — beyond the narrow ±62.5 Hz range; wide Goertzel must acquire.
        let true_fc = 1650.0f32;
        let ref_fc = 1500.0f32;
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
        let samples = bpsk_modulate(b"HelloWorldABCDEFGHIJKLMNOP", &cfg_tx).unwrap();
        let offset = afc_estimate_hz(&samples, &cfg_rx).expect("afc estimate");
        // Goertzel step is 12.5 Hz at baseband; allow ±15 Hz tolerance.
        assert!(
            (offset - 150.0).abs() < 15.0,
            "expected ≈+150 Hz AFC offset, got {offset:.2} Hz"
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

#[cfg(test)]
mod carrier_dip_tiebreak {
    //! #1363 MEASUREMENT — is the harm from `cancel_crossfade_isi` a carrier-dip TIE-BREAK?
    //!
    //! **The issue body's hypothesis is dead and this does not test it.** The body proposes that
    //! preamble-locked timing "sits a quarter symbol off for stretches"; the thread retired that on
    //! 2026-09-13, and the structural reason is decisive — this path has NO timing tracking. One lock
    //! per frame (`find_timing_offset_with_expected`), then fixed slicing at `offset + k*n`. The
    //! channel moves under a fixed lock; nothing drifts.
    //!
    //! **The live mechanism is a tie-break.** Where the composite response at the carrier collapses,
    //! the symbol response loses its DC term, so every NO-FLIP pair yields `r_k ~ 0` — an exact
    //! decision tie — while flips still yield large `r`. The backward substitution fills each tied
    //! slot with `-beta * a'_{k+1}`, so consecutive fills ALTERNATE IN SIGN and a differential
    //! detector reads alternation as flips. Every no-flip bit inside the dip decodes as 1.
    //!
    //! **Prediction:** in dips the hard path's errors concentrate on true bit 0 (-> 1.0) and sit at
    //! **1/3 on bit 1** — a flip decodes correctly only when the NEXT bit is also a flip, so
    //! err|b1 = 1/2 * 2/3 = 1/3. The soft path (no cancellation) is symmetric.
    //!
    //! An earlier version of this comment said "~0.0 on bit 1", which is wrong and would have made a
    //! measured 0.35 read as a MISS when it is the prediction hit. Corrected in review.
    //! **Falsifier:** in-dip hard errors NOT concentrated on bit 0.
    //!
    //! In-crate because `demodulate_iq`, `cancel_crossfade_isi` and `differential_decode` are private.
    //! A probe needing non-public access is a unit test, never an exported accessor — and it must not
    //! RE-IMPLEMENT the chain, so `the_composed_arm_matches_the_shipped_demodulator` asserts byte
    //! identity against `bpsk_demodulate` in the DEFAULT run.
    use super::*;
    use crate::modulate::bpsk_modulate;
    use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};

    const MODE: &str = "BPSK250";
    const FC: f32 = 1500.0;
    const FS: f32 = 8000.0;
    const BAUD: f32 = 250.0;

    fn cfg() -> ModulationConfig {
        ModulationConfig {
            mode: MODE.to_string(),
            sample_rate: FS as u32,
            center_frequency: FC,
            ..ModulationConfig::default()
        }
    }

    /// The two arms, composed from the SHIPPED private pieces. `cancel` is the ONLY difference.
    fn arm_bits(samples: &[f32], n: usize, offset: usize, cancel: bool) -> Vec<bool> {
        let (mut iv, mut qv) = demodulate_iq(samples, n, FC, FS, offset);
        if cancel {
            cancel_crossfade_isi(&mut iv, &mut qv);
        }
        let iq: Vec<(f32, f32)> = iv.iter().copied().zip(qv.iter().copied()).collect();
        differential_decode(&iq)
    }

    /// FIDELITY, in the DEFAULT run: the composed hard arm must reproduce `bpsk_demodulate` exactly.
    /// Without this the probe measures its own chain rather than the product's — the defect that put
    /// a wire-format argument on a template that did not exist (CLAUDE.md verification rule 5).
    #[test]
    fn the_composed_arm_matches_the_shipped_demodulator() {
        let payload: Vec<u8> = (0..48u8).map(|i| i.wrapping_mul(7)).collect();
        let c = cfg();
        let tx = bpsk_modulate(&payload, &c).expect("modulate");
        let n = samples_per_symbol(FS, BAUD).expect("sps");
        let offset = find_timing_offset_with_expected(
            &tx,
            n,
            FC,
            FS,
            &expected_preamble_symbols(PREAMBLE_SYMS),
        );
        let bits = arm_bits(&tx, n, offset, true);
        assert!(
            bits.len() > PREAMBLE_SYMS + TAIL_SYMS,
            "composed arm produced too few symbols to compare"
        );
        let composed = bits_to_bytes(&bits[PREAMBLE_SYMS - 1..bits.len() - TAIL_SYMS]);
        let shipped = bpsk_demodulate(&tx, &c).expect("shipped demodulator");
        let k = shipped.len().min(composed.len());
        assert!(k > 0, "nothing to compare");
        assert_eq!(
            &composed[..k],
            &shipped[..k],
            "the composed hard arm has DRIFTED from the shipped demodulator — every number this \
             module reports would be about a chain the product does not run"
        );
    }

    /// INSTRUMENT CHECK for the |H(fc,t)| recovery, which the whole measurement rests on.
    ///
    /// The method (from the issue thread) is to push a pure carrier through a SECOND `WattersonChannel`
    /// built from the same seed with noise off, and read its envelope. That is only valid if the same
    /// seed really does reproduce the same fading realisation — an assumption nobody had tested. This
    /// asserts it directly, and it is why the measurement below can be believed at all.
    #[test]
    fn the_same_seed_reproduces_the_same_fading_realisation() {
        let probe: Vec<f32> = (0..8000)
            .map(|k| (2.0 * std::f32::consts::PI * FC * k as f32 / FS).cos())
            .collect();
        let mk = |snr: f32| {
            let mut c = WattersonConfig::moderate_f1(Some(20260917));
            c.snr_db = snr;
            WattersonChannel::new(c).expect("watterson")
        };
        let a = mk(200.0).apply(&probe);
        let b = mk(200.0).apply(&probe);
        assert_eq!(
            a.len(),
            b.len(),
            "same seed produced different lengths — the recovery method is void"
        );
        let worst = a
            .iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max);
        assert!(
            worst < 1e-6,
            "the same seed did NOT reproduce the same realisation (worst sample delta {worst:.3e}) — \
             the |H(fc,t)| recovery this measurement depends on cannot work"
        );

        // AND the invariance the recovery ACTUALLY relies on, which the above does not test: the
        // FADING must not depend on `snr_db`. The probe reads the envelope at snr 200 and applies it
        // to a frame faded at snr 16; if the noise draw perturbed the envelope, the two would be
        // different realisations and every bin would be mis-assigned. Flagged in review — the first
        // version asserted same-config determinism and called it the recovery's premise.
        let noisy = mk(16.0).apply(&probe);
        let sig: f32 = (a.iter().map(|x| x * x).sum::<f32>() / a.len() as f32).sqrt();
        let diff: f32 = (noisy
            .iter()
            .zip(a.iter())
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f32>()
            / a.len() as f32)
            .sqrt();
        let in_rms: f32 = (probe.iter().map(|x| x * x).sum::<f32>() / probe.len() as f32).sqrt();
        let expected_noise = in_rms / 10f32.powf(16.0 / 20.0);
        assert!(
            (diff / expected_noise - 1.0).abs() < 0.25,
            "the difference between a noisy and a noise-free pass ({diff:.4}) is not the expected \
             additive noise ({expected_noise:.4}) — the envelope is NOT independent of `snr_db`, so \
             reading it at snr 200 and applying it to a snr-16 frame mis-assigns every bin \
             (signal rms {sig:.4})"
        );
    }

    /// Per-symbol |H(fc,t)|, recovered by pushing a pure carrier through the SAME fading
    /// realisation with noise off — validated by `the_same_seed_reproduces_the_same_fading_realisation`.
    ///
    /// Envelope by windowed RMS over one symbol period, scaled by sqrt(2) for a sinusoid. Stated
    /// plainly because it is an approximation: it is adequate for BINNING BY DEPTH, which is all the
    /// tabulation needs, and it avoids a Hilbert transform whose own correctness would then need
    /// establishing before any number here could be read.
    fn carrier_envelope(seed: u64, len: usize, n: usize, offset: usize) -> Vec<f32> {
        let probe: Vec<f32> = (0..len)
            .map(|k| (2.0 * std::f32::consts::PI * FC * k as f32 / FS).cos())
            .collect();
        let mut c = WattersonConfig::moderate_f1(Some(seed));
        c.snr_db = 200.0;
        let faded = WattersonChannel::new(c).expect("watterson").apply(&probe);
        let mut out = Vec::new();
        let mut start = offset;
        while start + n <= faded.len() {
            let ms: f32 = faded[start..start + n].iter().map(|s| s * s).sum::<f32>() / n as f32;
            out.push(ms.sqrt() * std::f32::consts::SQRT_2);
            start += n;
        }
        out
    }

    /// THE MEASUREMENT. `#[ignore]`d: it is a research harness that prints a table rather than
    /// asserting a threshold, and its runtime is seconds per seed.
    ///
    ///   cargo test -p bpsk-plugin --no-default-features --lib \
    ///       carrier_dip_tiebreak::measure -- --ignored --nocapture
    ///
    /// Truth comes from the CLEAN channel, and the run asserts the two arms agree there — if they
    /// disagree with no channel at all, the reference is not truth and nothing below means anything.
    #[test]
    #[ignore = "research harness: prints a table, asserts no threshold (#1363)"]
    fn measure_in_dip_error_by_true_bit() {
        const SEEDS: u64 = 96;
        let payload: Vec<u8> = (0..200u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        let c = cfg();
        let tx = bpsk_modulate(&payload, &c).expect("modulate");
        let n = samples_per_symbol(FS, BAUD).expect("sps");
        let expected = expected_preamble_symbols(PREAMBLE_SYMS);

        // Truth from the clean channel, with both arms required to agree.
        let off0 = find_timing_offset_with_expected(&tx, n, FC, FS, &expected);
        let truth = arm_bits(&tx, n, off0, false);
        let truth_hard = arm_bits(&tx, n, off0, true);
        let k = truth.len().min(truth_hard.len());
        let clean_disagree = (0..k).filter(|&i| truth[i] != truth_hard[i]).count();
        assert_eq!(
            clean_disagree, 0,
            "the arms disagree on a CLEAN channel ({clean_disagree} of {k} bits) — the reference is \
             not truth and every number below would be meaningless"
        );
        // Arm agreement alone is a SELF-CONSISTENT reference: both could agree and both be wrong.
        // Tie it to the payload the modulator was handed. Flagged in review as archetype A.
        assert_eq!(off0, 0, "a clean frame should lock at offset 0; got {off0}");
        let recovered = bits_to_bytes(&truth[PREAMBLE_SYMS - 1..truth.len() - TAIL_SYMS]);
        assert_eq!(
            &recovered[..payload.len().min(recovered.len())],
            &payload[..payload.len().min(recovered.len())],
            "the clean reference does not reproduce the TRANSMITTED payload — it is a decode that \
             agrees with itself, not truth"
        );

        // Bins of ABSOLUTE |H|. A per-seed MEDIAN normalisation was used first and dropped in
        // review: `doppler_envelope` normalises to unit mean-square, so E|H|^2 = 1 by construction
        // and absolute values are already comparable across seeds — while a per-seed median jitters
        // 10-15 % and, worse, is a THIRD normalisation in this issue ("of a ray", "peak", "median")
        // that nothing converts between. The FIRST attempt used
        // [0.15, 0.30, 0.50] and its deepest bin came back EMPTY — the envelope never dropped that
        // far in 24 seeds, so the regime where the prediction is sharpest was never sampled and the
        // signature was averaged away. That is the dilution error the thread already made and
        // corrected once (its dip window was 3x too wide); these edges reach the bottom.
        let edges = [0.0f32, 0.02, 0.05, 0.10, 0.20, 0.35, 1.0e9];
        let mut errs = [[[0u64; 2]; 2]; 6]; // [bin][arm 0=soft 1=hard][bit]
        let mut tot = [[[0u64; 2]; 2]; 6];
        let mut locks: Vec<usize> = Vec::new();
        // BITS, not bytes — this probe has no byte or frame metric, and the thread's
        // instruction was to count bytes. Naming it honestly rather than implying one.
        let mut bits_soft = 0u64;
        let mut bits_hard = 0u64;

        for seed in 0..SEEDS {
            let mut ch = WattersonConfig::moderate_f1(Some(seed));
            ch.snr_db = 16.0;
            let faded = WattersonChannel::new(ch).expect("watterson").apply(&tx);
            let off = find_timing_offset_with_expected(&faded, n, FC, FS, &expected);
            locks.push(off);
            let env = carrier_envelope(seed, tx.len(), n, off);
            if env.is_empty() {
                continue;
            }
            for (arm_i, cancel) in [(0usize, false), (1usize, true)] {
                let bits = arm_bits(&faded, n, off, cancel);
                let m = bits.len().min(truth.len());
                let mut bad_bits = 0u64;
                for i in 0..m {
                    // symbol index of bit i is i+1 (differential pairs); clamp into the envelope
                    let si = (i + 1).min(env.len().saturating_sub(1));
                    let d = env[si]; // absolute; see the binning note
                    let b = edges
                        .iter()
                        .position(|&e| d < e)
                        .unwrap_or(5)
                        .saturating_sub(1)
                        .min(5);
                    let bit = usize::from(truth[i]);
                    tot[b][arm_i][bit] += 1;
                    if bits[i] != truth[i] {
                        errs[b][arm_i][bit] += 1;
                        bad_bits += 1;
                    }
                }
                if arm_i == 0 {
                    bits_soft += bad_bits
                } else {
                    bits_hard += bad_bits
                }
            }
        }

        println!("\nPROBE-1363  moderate_f1, 16 dB, {SEEDS} seeds, BPSK250, no FEC");
        println!("  timing locks observed: {:?}", {
            let mut l = locks.clone();
            l.sort_unstable();
            l.dedup();
            l
        });
        println!("  |H| (absolute)        soft b0   soft b1   HARD b0   HARD b1      n");
        let names = [
            "[0.00,0.02)",
            "[0.02,0.05)",
            "[0.05,0.10)",
            "[0.10,0.20)",
            "[0.20,0.35)",
            "[0.35,inf)",
        ];
        for b in 0..6 {
            let r = |a: usize, bit: usize| {
                if tot[b][a][bit] == 0 {
                    f64::NAN
                } else {
                    errs[b][a][bit] as f64 / tot[b][a][bit] as f64
                }
            };
            println!(
                "  {:<20}  {:7.3}   {:7.3}   {:7.3}   {:7.3}  {:6}",
                names[b],
                r(0, 0),
                r(0, 1),
                r(1, 0),
                r(1, 1),
                tot[b][0][0] + tot[b][0][1]
            );
        }
        println!(
            "  wrong-BIT totals (no byte/frame metric here): soft {bits_soft}, hard {bits_hard}"
        );
        println!(
            "  PREDICTION (tie-break): HARD b0 -> 1.0, HARD b1 -> 1/3 (a flip survives only if"
        );
        println!("  the next bit also flips); soft symmetric. FALSIFIED if hard errors are NOT");
        println!("  concentrated on bit 0.\n");
    }

    /// Per-symbol `(e0, e1)` — the two Watterson rays as COMPLEX values, LABELLED, without
    /// private access.
    ///
    /// `ray_envelopes` draws `env0` then `env1` from the seeded RNG BEFORE any noise sample and
    /// reads neither `delay_spread_ms` nor `snr_db`, so one seed gives identical rays under any
    /// delay. Two `apply_complex` calls then separate them:
    ///
    /// * delay 0, DC probe → `(e0 + e1)/√2`
    /// * delay 1 ms, complex 1500 Hz tone, demixed → `(e0 − e1)/√2`, because 1500 Hz × 1 ms is 1.5
    ///   cycles and `e^{−j3π} = −1`
    ///
    /// Sum and difference recover both. **Magnitudes alone are not enough** — `|e0+e1|` and
    /// `|e0−e1|` give the power sum and the cross term, from which the rays come out at best as an
    /// unlabelled pair. That mistake is why this measurement was recorded as impossible for a day.
    ///
    /// `the_ray_split_agrees_with_the_envelope` pins it against `carrier_envelope` in the default
    /// run, so a silently wrong split cannot feed a table.
    fn ray_split(
        seed: u64,
        len: usize,
        n: usize,
        offset: usize,
    ) -> Vec<(num_complex::Complex<f32>, num_complex::Complex<f32>)> {
        use num_complex::Complex;
        let w = 2.0 * std::f32::consts::PI * FC / FS;
        let ones = vec![1.0f32; len];
        let zeros = vec![0.0f32; len];
        let mut c0 = WattersonConfig::moderate_f1(Some(seed));
        c0.snr_db = 200.0;
        c0.delay_spread_ms = 0.0;
        let (si, sq) = WattersonChannel::new(c0)
            .expect("w")
            .apply_complex(&ones, &zeros);
        let pi_: Vec<f32> = (0..len).map(|k| (w * k as f32).cos()).collect();
        let pq: Vec<f32> = (0..len).map(|k| (w * k as f32).sin()).collect();
        let mut c1 = WattersonConfig::moderate_f1(Some(seed));
        c1.snr_db = 200.0;
        let (di, dq) = WattersonChannel::new(c1)
            .expect("w")
            .apply_complex(&pi_, &pq);
        let mut out = Vec::new();
        let mut start = offset;
        while start + n <= len {
            let (mut sum, mut dif) = (Complex::new(0.0f32, 0.0), Complex::new(0.0f32, 0.0));
            for t in start..start + n {
                sum += Complex::new(si[t], sq[t]);
                dif += Complex::new(di[t], dq[t])
                    * Complex::new((w * t as f32).cos(), -(w * t as f32).sin());
            }
            sum /= n as f32;
            dif /= n as f32;
            let e0 = (sum + dif) / std::f32::consts::SQRT_2;
            let e1 = (sum - dif) / std::f32::consts::SQRT_2;
            out.push((e0, e1));
            start += n;
        }
        out
    }

    /// The split is only usable if it agrees with the instrument the tables already bin by, so this
    /// runs in the DEFAULT suite rather than inside the `#[ignore]`d harness. `|e0 − e1|` is what a
    /// carrier at `fc` sees through the 1 ms channel, which is exactly `carrier_envelope`.
    #[test]
    fn the_ray_split_agrees_with_the_envelope() {
        let len = 40_000usize;
        let n = samples_per_symbol(FS, BAUD).expect("sps");
        // Through `ray_split` itself. A first version re-derived the demixed difference INLINE and
        // compared that to `carrier_envelope` — which exercises the algebra but never the function
        // the tables call, so a scaled demix inside the helper passed it. Sabotage-verified after
        // the fix: `dif * 0.5` in the helper fails this.
        let (mut worst, mut checked) = (0.0f32, 0usize);
        for seed in 0..8u64 {
            let env = carrier_envelope(seed, len, n, 0);
            for (k, (e0, e1)) in ray_split(seed, len, n, 0).iter().enumerate().skip(1) {
                if k >= env.len() {
                    break;
                }
                // The 1 ms channel a carrier sees is e0 − e1 (1500 Hz x 1 ms = 1.5 cycles).
                worst = worst.max((((*e0 - *e1) / std::f32::consts::SQRT_2).norm() - env[k]).abs());
                checked += 1;
            }
        }
        assert!(
            checked > 5_000,
            "only {checked} symbols compared — the sweep is too small to mean anything"
        );
        assert!(
            worst < 0.02,
            "the rays recovered by `ray_split` depart from `carrier_envelope` by {worst:.4} over \
             {checked} symbols — the split is not measuring the channel the tables bin by"
        );

        // The rays are unit-mean-square **in the population**, not per seed: one frame is a single
        // Rayleigh realisation and reads anywhere from ~0.7 to ~1.6, so asserting it per seed fails
        // on seed 0 (1.561/0.747) — which this assertion caught before the claim shipped. What must
        // hold is that NEITHER ray is systematically larger, or "delayed-dominant" is not a
        // comparison between like quantities and every tap-split row is meaningless.
        let (mut m0, mut m1) = (0.0f32, 0.0f32);
        for seed in 0..8u64 {
            let split = ray_split(seed, len, n, 0);
            let tail = &split[1..];
            m0 += tail.iter().map(|(a, _)| a.norm_sqr()).sum::<f32>() / tail.len() as f32;
            m1 += tail.iter().map(|(_, b)| b.norm_sqr()).sum::<f32>() / tail.len() as f32;
        }
        m0 /= 8.0;
        m1 /= 8.0;
        assert!(
            (m0 / m1 - 1.0).abs() < 0.30,
            "mean ray powers {m0:.3}/{m1:.3} differ by more than 30% across 8 seeds, so one ray is \
             systematically larger and `|e1| > |e0|` does not mean what the tap-split rows claim"
        );
    }

    /// Transmitted symbols `a_k` reconstructed from the decoded truth stream, up to a global sign.
    ///
    /// The wire is differential: bit `i` is the flip between symbol `i` and `i+1`. The global sign
    /// is unobservable and irrelevant — every quantity below is a ratio or a differential product.
    fn symbols_from_truth(truth: &[bool]) -> Vec<f32> {
        let mut a = Vec::with_capacity(truth.len() + 1);
        a.push(1.0f32);
        for &flip in truth {
            let last = *a.last().expect("seeded");
            a.push(if flip { -last } else { last });
        }
        a
    }

    /// Per-symbol composite taps by sliding-window CORRELATION — **the defective instrument,
    /// retained as a control**, not the one to use. Prefer [`estimate_taps_ls`].
    ///
    /// Its "cross terms average out" premise fails on a real payload: 1/√41 ≈ 0.16 of relative
    /// self-noise. Measured on a CLEAN frame with no channel, no noise and genie symbols, it puts
    /// `Re(g_next/g_cur)` at p05 0.043 / p50 0.285 / p95 0.532 (truth: 0.308) and fires the sign
    /// predicate **4.3 %** of the time against least-squares' 0.0 %. It is kept because the
    /// comparison is itself the finding — it is what made a "genie" arm look like a floor rather
    /// than a ceiling in the first #1363 gate table.
    ///
    /// `r_k = Σ_m g[m]·a_{k+m} + noise`, and `a_k ∈ {±1}` with `a² = 1`, so for a near-random symbol
    /// stream `⟨r_k · a_{k+m}⟩` over a window estimates `g[m]` directly — the cross terms average
    /// out. This is deliberately the same shape a receiver's decision-directed estimator would take,
    /// which is the open question for any β-based fix: here it is fed GENIE symbols, so what it
    /// measures is the ceiling, not an achievable estimate.
    ///
    /// Window 41 symbols: a 1 Hz fade at 250 baud has a coherence time of hundreds of symbols, so
    /// the taps are ~constant across it, while 41 is long enough for the cross terms to average.
    fn estimate_taps_correlation(
        iq: &[(f32, f32)],
        a: &[f32],
        window: usize,
    ) -> Vec<[num_complex::Complex<f32>; 4]> {
        use num_complex::Complex;
        let half = window / 2;
        let n = iq.len().min(a.len());
        let mut out = vec![[Complex::new(0.0f32, 0.0); 4]; n];
        // Indexed on purpose: each step reads `iq[j]` against `a` at four different offsets, so an
        // iterator over one of them would still index the others.
        #[allow(clippy::needless_range_loop)]
        for k in 0..n {
            let lo = k.saturating_sub(half);
            let hi = (k + half + 1).min(n);
            let mut acc = [Complex::new(0.0f32, 0.0); 4];
            let mut cnt = 0.0f32;
            for j in lo..hi {
                // m = -1, 0, +1, +2  ->  a index j + m
                let idx = [j as isize - 1, j as isize, j as isize + 1, j as isize + 2];
                if idx[0] < 0 || idx[3] >= n as isize {
                    continue;
                }
                let r = Complex::new(iq[j].0, iq[j].1);
                for (t, &ix) in idx.iter().enumerate() {
                    acc[t] += r * a[ix as usize];
                }
                cnt += 1.0;
            }
            if cnt > 0.0 {
                for t in 0..4 {
                    out[k][t] = acc[t] / cnt;
                }
            }
        }
        out
    }

    /// Backward substitution with a PER-SYMBOL complex β — the generalisation of
    /// `cancel_crossfade_isi`, which is this with `β ≡ 1/3` real.
    ///
    /// `the_variable_canceller_reproduces_the_shipped_one` pins that equivalence in the DEFAULT run,
    /// so the alternative arms below are measured against the real transform rather than against a
    /// re-implementation of it (CLAUDE.md verification rule 5).
    fn cancel_variable(iq: &mut [(f32, f32)], beta: &[num_complex::Complex<f32>]) {
        use num_complex::Complex;
        let n = iq.len().min(beta.len());
        for k in (0..n.saturating_sub(1)).rev() {
            let next = Complex::new(iq[k + 1].0, iq[k + 1].1);
            let cur = Complex::new(iq[k].0, iq[k].1) - beta[k] * next;
            iq[k] = (cur.re, cur.im);
        }
    }

    /// The variable-β canceller must BE the shipped one at β = 1/3, or every arm below is measured
    /// against a re-implementation instead of the product.
    #[test]
    fn the_variable_canceller_reproduces_the_shipped_one() {
        use num_complex::Complex;
        let payload: Vec<u8> = (0..64u8).map(|i| i.wrapping_mul(11)).collect();
        let c = cfg();
        let tx = bpsk_modulate(&payload, &c).expect("modulate");
        let n = samples_per_symbol(FS, BAUD).expect("sps");
        let (mut iv, mut qv) = demodulate_iq(&tx, n, FC, FS, 0);
        let mut mine: Vec<(f32, f32)> = iv.iter().copied().zip(qv.iter().copied()).collect();
        cancel_crossfade_isi(&mut iv, &mut qv);
        let betas = vec![Complex::new(CROSSFADE_ISI_BETA, 0.0); mine.len()];
        cancel_variable(&mut mine, &betas);
        let worst = mine
            .iter()
            .zip(iv.iter().zip(qv.iter()))
            .map(|((mi, mq), (si, sq))| (mi - si).abs().max((mq - sq).abs()))
            .fold(0.0f32, f32::max);
        assert!(
            worst < 1e-5,
            "cancel_variable at beta=1/3 differs from cancel_crossfade_isi by {worst:e} — the \
             alternative arms would be measured against a re-implementation, not the product"
        );
        assert!(mine.len() > 100, "fixture too short to mean anything");
    }

    /// (lock × dominance) — the slice #1363 named next, and the one that tells apart two
    /// explanations the thread has been carrying side by side.
    ///
    /// The thread's claim is that the real variable is not which ray is larger in the ABSOLUTE
    /// sense, but **where the timing lock sits relative to the dominant ray**. Those are not
    /// separable from observational data: with one lock per frame and taps at 0 and 8 samples,
    /// `rel = lock − tap` is nearly determined by dominance once the observed lock distribution is
    /// fixed, so a cross-tab of found locks would look like a 2×2 while being collinear.
    ///
    /// So the lock is FORCED, and only to the two values that are physically meaningful — each ray's
    /// own arrival. `n = 32` samples per symbol and 1 ms = 8 samples, so lock 0 samples aligned to
    /// the direct ray and lock 8 aligned to the delayed one; both are a quarter-symbol apart, well
    /// inside one symbol, so bit `i` still corresponds to transmitted bit `i` in both. That gives a
    /// genuine 2×2 where alignment and dominance vary independently:
    ///
    /// | | direct dominant | delayed dominant |
    /// |---|---|---|
    /// | lock 0 | aligned | early by ¼ symbol |
    /// | lock 8 | late by ¼ symbol | aligned |
    ///
    /// Noise-free and restricted to `|H| ∈ [0.05,0.20)`, because that is where the frames are
    /// actually lost (the deep bin holds ~2.6 bits per frame) and because the noise-free row is the
    /// one that showed the cost does not close at infinite SNR.
    ///
    ///   cargo test -p bpsk-plugin --no-default-features --lib \
    ///       carrier_dip_tiebreak::measure_lock -- --ignored --nocapture
    #[test]
    #[ignore = "research harness: prints a table, asserts no threshold (#1363)"]
    fn measure_lock_relative_to_dominant_tap() {
        const SEEDS: u64 = 96;
        const DELAY_SAMPLES: usize = 8; // 1 ms at 8 kHz, a quarter symbol at BPSK250
        let payload: Vec<u8> = (0..200u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        let c = cfg();
        let tx = bpsk_modulate(&payload, &c).expect("modulate");
        let n = samples_per_symbol(FS, BAUD).expect("sps");
        let expected = expected_preamble_symbols(PREAMBLE_SYMS);
        let off0 = find_timing_offset_with_expected(&tx, n, FC, FS, &expected);
        let truth = arm_bits(&tx, n, off0, false);

        // [arm][lock 0|8][dominance 0=direct 1=delayed][bit]
        let mut err = [[[[0u64; 2]; 2]; 2]; 2];
        let mut tot = [[[[0u64; 2]; 2]; 2]; 2];

        for seed in 0..SEEDS {
            let mut ch = WattersonConfig::moderate_f1(Some(seed));
            ch.snr_db = 200.0;
            let faded = WattersonChannel::new(ch).expect("w").apply(&tx);
            for (li, &lock) in [0usize, DELAY_SAMPLES].iter().enumerate() {
                let env = carrier_envelope(seed, tx.len(), n, lock);
                let rays = ray_split(seed, tx.len(), n, lock);
                if env.is_empty() || rays.is_empty() {
                    continue;
                }
                for (arm_i, cancel) in [(0usize, false), (1usize, true)] {
                    let bits = arm_bits(&faded, n, lock, cancel);
                    let m = bits.len().min(truth.len());
                    for i in 0..m {
                        let si = (i + 1).min(env.len() - 1);
                        let d = env[si];
                        if !(0.05..0.20).contains(&d) {
                            continue;
                        }
                        let (e0, e1) = rays[si.min(rays.len() - 1)];
                        let dom = usize::from(e1.norm() > e0.norm());
                        let bit = usize::from(truth[i]);
                        tot[arm_i][li][dom][bit] += 1;
                        if bits[i] != truth[i] {
                            err[arm_i][li][dom][bit] += 1;
                        }
                    }
                }
            }
        }

        println!("\nPROBE-1363-LOCK  moderate_f1, NOISE-FREE, {SEEDS} seeds, |H| in [0.05,0.20)");
        println!("  lock is FORCED to each ray's own arrival, so alignment and dominance vary");
        println!("  independently. lock 0 = direct ray's arrival; lock 8 = delayed ray's.\n");
        println!(
            "  lock | dominant | alignment        | soft b0/b1      | HARD b0/b1      |     n"
        );
        for li in 0..2 {
            for dom in 0..2 {
                let aligned = li == dom;
                let label = if aligned {
                    "aligned"
                } else if li == 0 {
                    "early by 1/4 sym"
                } else {
                    "late by 1/4 sym"
                };
                let r = |a: usize, bit: usize| {
                    if tot[a][li][dom][bit] == 0 {
                        f64::NAN
                    } else {
                        err[a][li][dom][bit] as f64 / tot[a][li][dom][bit] as f64
                    }
                };
                println!(
                    "  {:4} | {:8} | {:16} | {:6.3} / {:6.3} | {:6.3} / {:6.3} | {:5}",
                    if li == 0 { 0 } else { DELAY_SAMPLES },
                    if dom == 0 { "direct" } else { "delayed" },
                    label,
                    r(0, 0),
                    r(0, 1),
                    r(1, 0),
                    r(1, 1),
                    tot[0][li][dom][0] + tot[0][li][dom][1]
                );
            }
        }
        println!(
            "\n  READ IT THIS WAY: if the thread is right that ALIGNMENT is the variable, the"
        );
        println!("  hard arm's penalty tracks the `aligned`/`early`/`late` column and not the");
        println!(
            "  `dominant` one. If instead the penalty follows `delayed` in BOTH lock rows, then"
        );
        println!("  absolute dominance is the variable and the lock is a bystander.\n");
    }

    /// The three measurements this thread named as outstanding, in one pass (#1363).
    ///
    /// 1. **An SNR axis** (16/24/32 dB and noise-free, same seeds) — the thread required it "before
    ///    any claim about a graded onset", because at 16 dB symbol-domain noise (σ ≈ 0.039) is only
    ///    1.4× the tie-break fill (β·g_next ≈ 0.053), so noise breaks the tie about as often as the
    ///    recursion does.
    /// 2. **Bytes, not bits** — RS(255,223) corrects t=16 BYTES, and the hard arm makes FEWER wrong
    ///    bits while losing MORE frames. Counted on the payload-ALIGNED slice: grouping from
    ///    differential bit 0 starts 31 bits before the payload's byte boundary and sweeps in
    ///    preamble bytes, which moved the threshold count by two frames and produced a spurious
    ///    "pinned at exactly 21".
    /// 3. **Which tap dominates** — see `ray_split_powers`. This was called impossible once; it is
    ///    not, and it turned out to be the measurement that located the harm.
    ///
    /// Prints tables, asserts no threshold. The assertions that make it trustworthy run in the
    /// DEFAULT suite: the two fidelity tests above, plus `the_ray_split_agrees_with_the_envelope`.
    ///
    ///   cargo test -p bpsk-plugin --no-default-features --lib \
    ///       carrier_dip_tiebreak::measure_snr_axis -- --ignored --nocapture
    #[test]
    #[ignore = "research harness: prints a table, asserts no threshold (#1363)"]
    fn measure_snr_axis_tap_split_and_bytes() {
        const SEEDS: u64 = 96;
        const RS_T: usize = 16;
        let payload: Vec<u8> = (0..200u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        let c = cfg();
        let tx = bpsk_modulate(&payload, &c).expect("modulate");
        let n = samples_per_symbol(FS, BAUD).expect("sps");
        let expected = expected_preamble_symbols(PREAMBLE_SYMS);
        let off0 = find_timing_offset_with_expected(&tx, n, FC, FS, &expected);
        assert_eq!(off0, 0);
        let truth = arm_bits(&tx, n, off0, false);
        let truth_hard = arm_bits(&tx, n, off0, true);
        assert_eq!(
            (0..truth.len().min(truth_hard.len()))
                .filter(|&i| truth[i] != truth_hard[i])
                .count(),
            0
        );
        let data_lo = PREAMBLE_SYMS - 1;
        let data_hi = truth.len() - TAIL_SYMS;
        let recovered = bits_to_bytes(&truth[data_lo..data_hi]);
        assert_eq!(
            &recovered[..],
            &payload[..],
            "aligned slice must reproduce the payload"
        );
        // distance from bit i (truth 0) to the next true flip
        let mut dist = vec![usize::MAX; truth.len()];
        let mut next = usize::MAX;
        for i in (0..truth.len()).rev() {
            if truth[i] {
                next = i;
            }
            dist[i] = if next == usize::MAX { 99 } else { next - i };
        }

        // ---- per-seed: envelope (SNR independent), env0/env1 split, noise-free lock
        struct SeedInfo {
            env: Vec<f32>,
            delayed_dom: Vec<bool>,
            lock200: usize,
            split_ok: bool,
            p0: f32,
            p1: f32,
        }
        let mut infos: Vec<SeedInfo> = Vec::new();
        for seed in 0..SEEDS {
            let mut ch = WattersonConfig::moderate_f1(Some(seed));
            ch.snr_db = 200.0;
            let faded = WattersonChannel::new(ch).expect("w").apply(&tx);
            let lock200 = find_timing_offset_with_expected(&faded, n, FC, FS, &expected);
            // The harness's own instrument, at the noise-free lock.
            let env = carrier_envelope(seed, tx.len(), n, lock200);
            // One implementation, the one `the_ray_split_agrees_with_the_envelope` pins.
            let rays = ray_split(seed, tx.len(), n, lock200);
            let mut delayed_dom = Vec::new();
            let mut worst = 0.0f32;
            let (mut p0, mut p1, mut cnt) = (0.0f32, 0.0f32, 0usize);
            for (k, (e0, e1)) in rays.iter().enumerate() {
                if k > 0 && k < env.len() {
                    let d = (*e0 - *e1) / std::f32::consts::SQRT_2;
                    worst = worst.max((d.norm() - env[k]).abs());
                    p0 += e0.norm_sqr();
                    p1 += e1.norm_sqr();
                    cnt += 1;
                }
                delayed_dom.push(e1.norm() > e0.norm());
            }
            let split_ok = worst < 0.02;
            infos.push(SeedInfo {
                env,
                delayed_dom,
                lock200,
                split_ok,
                p0: p0 / cnt as f32,
                p1: p1 / cnt as f32,
            });
        }
        let bad_split = infos.iter().filter(|i| !i.split_ok).count();
        let mp0 = infos.iter().map(|i| i.p0).sum::<f32>() / SEEDS as f32;
        let mp1 = infos.iter().map(|i| i.p1).sum::<f32>() / SEEDS as f32;
        println!("\nPROBE-1363-TAPS  env0/env1 split: seeds failing cross-check vs carrier_envelope (>0.02): {bad_split}/{SEEDS}; mean|e0|^2={mp0:.3} mean|e1|^2={mp1:.3}");
        let dd: usize = infos
            .iter()
            .map(|i| i.delayed_dom.iter().filter(|&&b| b).count())
            .sum();
        let tt: usize = infos.iter().map(|i| i.delayed_dom.len()).sum();
        println!(
            "  delayed-ray dominant fraction of symbols: {:.3}",
            dd as f64 / tt as f64
        );
        println!("  lock200 set: {:?}", {
            let mut l: Vec<usize> = infos.iter().map(|i| i.lock200).collect();
            l.sort();
            l.dedup();
            l
        });

        let fine = [0.0f32, 0.01, 0.02, 0.05];
        for &snr in &[16.0f32, 24.0, 32.0, 200.0] {
            for lockmode in 0..2 {
                // [arm][bit][finebin]
                let mut fe = [[[0u64; 3]; 2]; 2];
                let mut ft = [[[0u64; 3]; 2]; 2];
                // distance table deep(<0.05) b0: [arm][dbin 0..4]
                let mut de = [[0u64; 4]; 2];
                let mut dt = [[0u64; 4]; 2];
                // dominant tap deep(<0.05): [arm][bit][dom]
                let mut oe = [[[0u64; 2]; 2]; 2];
                let mut ot = [[[0u64; 2]; 2]; 2];
                // also mid bin [0.05,0.20) by dom
                let mut me = [[[0u64; 2]; 2]; 2];
                let mut mt = [[[0u64; 2]; 2]; 2];
                let mut bad_al = [0u64; 2];
                let mut bad_raw = [0u64; 2];
                let mut lost_al = [Vec::<u64>::new(), Vec::new()];
                let mut lost_raw = [0u64; 2];
                let mut hist = [[0u64; 6]; 2];
                let mut bits_wrong = [0u64; 2];
                let mut lock_diff = 0usize;
                let mut locks_noisy = Vec::new();
                for seed in 0..SEEDS {
                    let info = &infos[seed as usize];
                    let mut ch = WattersonConfig::moderate_f1(Some(seed));
                    ch.snr_db = snr;
                    let faded = WattersonChannel::new(ch).expect("w").apply(&tx);
                    let lock_n = find_timing_offset_with_expected(&faded, n, FC, FS, &expected);
                    if lock_n != info.lock200 {
                        lock_diff += 1;
                    }
                    locks_noisy.push(lock_n);
                    let off = if lockmode == 0 { lock_n } else { info.lock200 };
                    // envelope at the lock actually used
                    let env = if off == info.lock200 {
                        info.env.clone()
                    } else {
                        carrier_envelope(seed, tx.len(), n, off)
                    };
                    for (arm_i, cancel) in [(0usize, false), (1usize, true)] {
                        let bits = arm_bits(&faded, n, off, cancel);
                        let m = bits.len().min(truth.len());
                        for i in 0..m {
                            let si = (i + 1).min(env.len() - 1);
                            let d = env[si];
                            let wrong = bits[i] != truth[i];
                            if wrong {
                                bits_wrong[arm_i] += 1;
                            }
                            let bit = usize::from(truth[i]);
                            let dom =
                                usize::from(info.delayed_dom[si.min(info.delayed_dom.len() - 1)]);
                            if d < 0.05 {
                                let fb = if d < 0.01 {
                                    0
                                } else if d < 0.02 {
                                    1
                                } else {
                                    2
                                };
                                ft[arm_i][bit][fb] += 1;
                                if wrong {
                                    fe[arm_i][bit][fb] += 1;
                                }
                                ot[arm_i][bit][dom] += 1;
                                if wrong {
                                    oe[arm_i][bit][dom] += 1;
                                }
                                if bit == 0 {
                                    let db = (dist[i].min(4)) - 1;
                                    dt[arm_i][db] += 1;
                                    if wrong {
                                        de[arm_i][db] += 1;
                                    }
                                }
                            } else if d < 0.20 {
                                mt[arm_i][bit][dom] += 1;
                                if wrong {
                                    me[arm_i][bit][dom] += 1;
                                }
                            }
                        }
                        // bytes, aligned to the payload
                        let hi = data_hi.min(m);
                        let got = bits_to_bytes(&bits[data_lo..hi]);
                        let want = bits_to_bytes(&truth[data_lo..hi]);
                        let nb = got.len().min(want.len());
                        let bad = (0..nb).filter(|&j| got[j] != want[j]).count();
                        bad_al[arm_i] += bad as u64;
                        if bad > RS_T {
                            lost_al[arm_i].push(seed);
                        }
                        let hb = match bad {
                            0..=8 => 0,
                            9..=12 => 1,
                            13..=16 => 2,
                            17..=20 => 3,
                            21..=30 => 4,
                            _ => 5,
                        };
                        hist[arm_i][hb] += 1;
                        // bytes, the harness's grouping (from differential bit 0)
                        let got = bits_to_bytes(&bits[..m]);
                        let want = bits_to_bytes(&truth[..m]);
                        let nb = got.len().min(want.len());
                        let bad = (0..nb).filter(|&j| got[j] != want[j]).count();
                        bad_raw[arm_i] += bad as u64;
                        if bad > RS_T {
                            lost_raw[arm_i] += 1;
                        }
                    }
                }
                let lm = if lockmode == 0 {
                    "noisy-lock"
                } else {
                    "fixed-lock(200)"
                };
                println!("\n== snr {snr} [{lm}]  seeds whose noisy lock != lock200: {lock_diff}; noisy locks {:?}", { let mut l = locks_noisy.clone(); l.sort(); l.dedup(); l });
                let r = |e: u64, t: u64| {
                    if t == 0 {
                        f64::NAN
                    } else {
                        e as f64 / t as f64
                    }
                };
                println!("  fine deep bins   err|b0 (n)          err|b1 (n)");
                for fb in 0..3 {
                    for arm_i in 0..2 {
                        println!(
                            "   [{:.2},{:.2}) {}  {:.3} ({:4})   {:.3} ({:4})",
                            fine[fb],
                            fine[fb + 1],
                            if arm_i == 0 { "soft" } else { "HARD" },
                            r(fe[arm_i][0][fb], ft[arm_i][0][fb]),
                            ft[arm_i][0][fb],
                            r(fe[arm_i][1][fb], ft[arm_i][1][fb]),
                            ft[arm_i][1][fb]
                        );
                    }
                }
                println!("  deep(<0.05) b0 err by distance to next true flip d=1,2,3,4+:");
                for arm_i in 0..2 {
                    println!(
                        "   {}  {}",
                        if arm_i == 0 { "soft" } else { "HARD" },
                        (0..4)
                            .map(|k| format!(
                                "d{}={:.3}({})",
                                k + 1,
                                r(de[arm_i][k], dt[arm_i][k]),
                                dt[arm_i][k]
                            ))
                            .collect::<Vec<_>>()
                            .join("  ")
                    );
                }
                println!("  deep(<0.05) by dominant tap  [direct-dom b0/b1 | delayed-dom b0/b1]");
                for arm_i in 0..2 {
                    println!(
                        "   {}  {:.3}/{:.3} (n {},{}) | {:.3}/{:.3} (n {},{})",
                        if arm_i == 0 { "soft" } else { "HARD" },
                        r(oe[arm_i][0][0], ot[arm_i][0][0]),
                        r(oe[arm_i][1][0], ot[arm_i][1][0]),
                        ot[arm_i][0][0],
                        ot[arm_i][1][0],
                        r(oe[arm_i][0][1], ot[arm_i][0][1]),
                        r(oe[arm_i][1][1], ot[arm_i][1][1]),
                        ot[arm_i][0][1],
                        ot[arm_i][1][1]
                    );
                }
                println!(
                    "  mid [0.05,0.20) by dominant tap  [direct-dom b0/b1 | delayed-dom b0/b1]"
                );
                for arm_i in 0..2 {
                    println!(
                        "   {}  {:.3}/{:.3} (n {},{}) | {:.3}/{:.3} (n {},{})",
                        if arm_i == 0 { "soft" } else { "HARD" },
                        r(me[arm_i][0][0], mt[arm_i][0][0]),
                        r(me[arm_i][1][0], mt[arm_i][1][0]),
                        mt[arm_i][0][0],
                        mt[arm_i][1][0],
                        r(me[arm_i][0][1], mt[arm_i][0][1]),
                        r(me[arm_i][1][1], mt[arm_i][1][1]),
                        mt[arm_i][0][1],
                        mt[arm_i][1][1]
                    );
                }
                for arm_i in 0..2 {
                    println!("  {}: wrong bits {}  bad bytes aligned {} (raw-grouping {})  lost>16 aligned {} (raw {})  hist[0-8,9-12,13-16,17-20,21-30,31+]={:?}",
                        if arm_i==0 {"soft"} else {"HARD"}, bits_wrong[arm_i], bad_al[arm_i], bad_raw[arm_i], lost_al[arm_i].len(), lost_raw[arm_i], hist[arm_i]);
                    println!("     lost seeds (aligned): {:?}", lost_al[arm_i]);
                }
            }
        }
    }

    // ═══════════════════════ REVIEW VARIANT (not for merge) ═══════════════════════
    // Controls and off-band cells for the six gate arms. Reuses every helper above unchanged.

    fn lcg(seed: &mut u64) -> u64 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *seed >> 11
    }

    /// Fisher–Yates permutation of a fire mask: same count, positions randomised.
    fn shuffle_mask(mask: &[bool], mut seed: u64) -> Vec<bool> {
        let mut out = mask.to_vec();
        for i in (1..out.len()).rev() {
            let j = (lcg(&mut seed) % (i as u64 + 1)) as usize;
            out.swap(i, j);
        }
        out
    }

    /// Shuffle WITHIN each stratum so the duty cycle is matched per cell, not per frame.
    fn shuffle_stratified(mask: &[bool], seed: u64, strata: Option<&[u8]>) -> Vec<bool> {
        let Some(st) = strata else {
            return shuffle_mask(mask, seed);
        };
        let mut out = mask.to_vec();
        for s in 0u8..3 {
            let idx: Vec<usize> = (0..mask.len().min(st.len()))
                .filter(|&k| st[k] == s)
                .collect();
            let sub: Vec<bool> = idx.iter().map(|&k| mask[k]).collect();
            let sh = shuffle_mask(&sub, seed.wrapping_add(s as u64 * 999));
            for (j, &k) in idx.iter().enumerate() {
                out[k] = sh[j];
            }
        }
        out
    }

    /// Windowed LEAST-SQUARES taps (4 unknowns, real regressors, complex observations). On a
    /// noise-free frame this returns the exact composite taps; the correlation estimator does not,
    /// because the payload's own a_k·a_{k+m} cross terms do not vanish over 41 symbols.
    // Linear algebra over fixed 4x4 arrays: every loop indexes several parallel arrays at once, so
    // the iterator rewrite clippy suggests does not apply.
    #[allow(clippy::needless_range_loop)]
    fn estimate_taps_ls(
        iq: &[(f32, f32)],
        a: &[f32],
        window: usize,
    ) -> Vec<[num_complex::Complex<f32>; 4]> {
        use num_complex::Complex;
        fn solve4(mut m: [[f64; 4]; 4], mut b: [f64; 4]) -> [f64; 4] {
            for c in 0..4 {
                let mut p = c;
                for r in c + 1..4 {
                    if m[r][c].abs() > m[p][c].abs() {
                        p = r;
                    }
                }
                m.swap(c, p);
                b.swap(c, p);
                let d = m[c][c];
                if d.abs() < 1e-12 {
                    return [0.0; 4];
                }
                for r in 0..4 {
                    if r == c {
                        continue;
                    }
                    let f = m[r][c] / d;
                    for k in 0..4 {
                        m[r][k] -= f * m[c][k];
                    }
                    b[r] -= f * b[c];
                }
            }
            [
                b[0] / m[0][0],
                b[1] / m[1][1],
                b[2] / m[2][2],
                b[3] / m[3][3],
            ]
        }
        let half = window / 2;
        let n = iq.len().min(a.len());
        let mut out = vec![[Complex::new(0.0f32, 0.0); 4]; n];
        for k in 0..n {
            let lo = k.saturating_sub(half);
            let hi = (k + half + 1).min(n);
            let mut xtx = [[0.0f64; 4]; 4];
            let mut xtr = [0.0f64; 4];
            let mut xti = [0.0f64; 4];
            let mut cnt = 0usize;
            for j in lo..hi {
                if j < 1 || j + 2 >= n {
                    continue;
                }
                let x = [
                    a[j - 1] as f64,
                    a[j] as f64,
                    a[j + 1] as f64,
                    a[j + 2] as f64,
                ];
                for p in 0..4 {
                    for q in 0..4 {
                        xtx[p][q] += x[p] * x[q];
                    }
                    xtr[p] += x[p] * iq[j].0 as f64;
                    xti[p] += x[p] * iq[j].1 as f64;
                }
                cnt += 1;
            }
            if cnt < 8 {
                continue;
            }
            for p in 0..4 {
                xtx[p][p] += 1e-3 * cnt as f64;
            } // ridge: the preamble's period-4 run is rank-deficient
            let re = solve4(xtx, xtr);
            let im = solve4(xtx, xti);
            for t in 0..4 {
                out[k][t] = Complex::new(re[t] as f32, im[t] as f32);
            }
        }
        out
    }

    /// Least-squares taps by DEFAULT. Set `BPSK_TAPS_CORRELATION=1` to switch to the correlation
    /// estimator instead — kept only so the instrument comparison stays reproducible, since the
    /// difference between them flipped a conclusion in #1363 (a "genie" arm that was really a floor).
    fn taps_for(
        iq: &[(f32, f32)],
        a: &[f32],
        window: usize,
    ) -> Vec<[num_complex::Complex<f32>; 4]> {
        if std::env::var("BPSK_TAPS_CORRELATION").is_ok() {
            estimate_taps_correlation(iq, a, window)
        } else {
            estimate_taps_ls(iq, a, window)
        }
    }

    const ARMS: [&str; 9] = [
        "soft",
        "hard",
        "thread",
        "sign",
        "beta",
        "sign_dd",
        "beta_dd",
        "sign_shuf",
        "sdd_shuf",
    ];

    /// Per-arm β vectors plus the two fire masks (genie sign, dd sign) for a frame.
    #[allow(clippy::type_complexity)]
    fn build_betas(
        base: &[(f32, f32)],
        a_genie: &[f32],
        window: usize,
        shuffle_seed: u64,
        strata: Option<&[u8]>,
    ) -> (Vec<Vec<num_complex::Complex<f32>>>, Vec<bool>, Vec<bool>) {
        use num_complex::Complex;
        let m = base.len();
        let taps = taps_for(base, a_genie, window);
        let soft_bits = differential_decode(base);
        let a_dd = symbols_from_truth(&soft_bits);
        let taps_dd = taps_for(base, &a_dd, window);
        let b0 = Complex::new(0.0f32, 0.0);
        let bh = Complex::new(CROSSFADE_ISI_BETA, 0.0);
        let mut betas: Vec<Vec<Complex<f32>>> = vec![
            vec![b0; m],
            vec![bh; m],
            vec![bh; m],
            vec![bh; m],
            vec![b0; m],
            vec![bh; m],
            vec![b0; m],
            vec![bh; m],
            vec![bh; m],
        ];
        let mut sign_mask = vec![false; m];
        let mut sdd_mask = vec![false; m];
        for k in 0..m.min(taps.len()).min(taps_dd.len()) {
            let (gc, gn) = (taps[k][1], taps[k][2]);
            if gc.norm() < gn.norm() {
                betas[2][k] = b0;
            }
            if (gn * gc.conj()).re < 0.0 {
                betas[3][k] = b0;
                sign_mask[k] = true;
            }
            if gc.norm() > 1e-9 {
                let be = gn / gc;
                if be.norm() < 1.0 {
                    betas[4][k] = be;
                }
            }
            let (dc, dn) = (taps_dd[k][1], taps_dd[k][2]);
            if (dn * dc.conj()).re < 0.0 {
                betas[5][k] = b0;
                sdd_mask[k] = true;
            }
            if dc.norm() > 1e-9 {
                let be = dn / dc;
                if be.norm() < 1.0 {
                    betas[6][k] = be;
                }
            }
        }
        let sh = shuffle_stratified(&sign_mask, shuffle_seed ^ 0xA5A5, strata);
        let sdh = shuffle_stratified(&sdd_mask, shuffle_seed ^ 0x5A5A, strata);
        for k in 0..m {
            if sh[k] {
                betas[7][k] = b0;
            }
            if sdh[k] {
                betas[8][k] = b0;
            }
        }
        (betas, sign_mask, sdd_mask)
    }

    fn bad_bytes(bits: &[bool], truth: &[bool], lo: usize, hi: usize) -> (usize, usize) {
        let hi = hi.min(bits.len()).min(truth.len());
        let got = bits_to_bytes(&bits[lo..hi]);
        let want = bits_to_bytes(&truth[lo..hi]);
        let bad = got.iter().zip(want.iter()).filter(|(g, w)| g != w).count();
        let wrong_bits = (lo..hi).filter(|&i| bits[i] != truth[i]).count();
        (bad, wrong_bits)
    }

    fn pct(num: u64, den: u64) -> f64 {
        if den == 0 {
            f64::NAN
        } else {
            100.0 * num as f64 / den as f64
        }
    }

    ///   cargo test -p bpsk-plugin --no-default-features --lib --release \
    ///       carrier_dip_tiebreak::review_gates -- --ignored --nocapture
    #[test]
    #[ignore = "review harness"]
    fn measure_gates_controls_and_off_band() {
        use openpulse_channel::awgn::AwgnChannel;
        use openpulse_channel::AwgnConfig;
        const SEEDS: u64 = 96;
        const DELAY: usize = 8;
        const WINDOW: usize = 41;
        const RS_T: usize = 16;
        let payload: Vec<u8> = (0..200u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        let c = cfg();
        let tx = bpsk_modulate(&payload, &c).expect("modulate");
        let n = samples_per_symbol(FS, BAUD).expect("sps");
        let expected = expected_preamble_symbols(PREAMBLE_SYMS);
        let off0 = find_timing_offset_with_expected(&tx, n, FC, FS, &expected);
        assert_eq!(off0, 0);
        let truth = arm_bits(&tx, n, off0, false);
        let a = symbols_from_truth(&truth);
        let data_lo = PREAMBLE_SYMS - 1;
        let data_hi = truth.len() - TAIL_SYMS;
        assert_eq!(bits_to_bytes(&truth[data_lo..data_hi]), payload);
        let tx_rms = (tx.iter().map(|s| s * s).sum::<f32>() / tx.len() as f32).sqrt();
        println!(
            "\nPROBE-1363  estimator = {}",
            // Read the SAME switch `taps_for` reads. A label that can disagree with the
            // instrument it names is the defect this whole probe exists to catch.
            if std::env::var("BPSK_TAPS_CORRELATION").is_ok() {
                "CORRELATION (the defective control)"
            } else {
                "LEAST-SQUARES (default)"
            }
        );
        println!("PROBE-1363  tx rms {tx_rms:.4}; #821's sigma 0.9 is {:.1} dB in this crate's SNR convention",
            20.0 * (tx_rms / 0.9).log10());

        // ── Part 0: estimator control on the CLEAN frame ─────────────────────────────────
        {
            let (iv, qv) = demodulate_iq(&tx, n, FC, FS, 0);
            let base: Vec<(f32, f32)> = iv.iter().copied().zip(qv.iter().copied()).collect();
            let (_, sm, sdm) = build_betas(&base, &a, WINDOW, 1, None);
            let taps = taps_for(&base, &a, WINDOW);
            let lo = data_lo + WINDOW;
            let hi = data_hi.saturating_sub(WINDOW);
            let mut ratios: Vec<f32> = (lo..hi).map(|k| (taps[k][2] / taps[k][1]).re).collect();
            ratios.sort_by(|x, y| x.partial_cmp(y).unwrap());
            let q = |p: f64| ratios[((ratios.len() - 1) as f64 * p) as usize];
            let thread_f = (lo..hi)
                .filter(|&k| taps[k][1].norm() < taps[k][2].norm())
                .count();
            let sign_f = (lo..hi).filter(|&k| sm[k]).count();
            let sdd_f = (lo..hi).filter(|&k| sdm[k]).count();
            let mut prev: Vec<f32> = (lo..hi).map(|k| (taps[k][0] / taps[k][1]).norm()).collect();
            prev.sort_by(|x, y| x.partial_cmp(y).unwrap());
            println!(
                "PART 0  clean frame, genie taps, data region only (n={}):",
                hi - lo
            );
            println!(
                "  Re(g_next/g_cur)  p05 {:.3}  p50 {:.3}  p95 {:.3}   (shipped beta = 0.333)",
                q(0.05),
                q(0.5),
                q(0.95)
            );
            println!(
                "  |g_prev/g_cur|    p50 {:.3}  p95 {:.3}",
                prev[prev.len() / 2],
                prev[(prev.len() - 1) * 95 / 100]
            );
            println!("  thread fires {:.1}%   sign fires {:.1}%   sign_dd fires {:.1}%  (any non-zero here is estimator self-noise)",
                pct(thread_f as u64, (hi - lo) as u64), pct(sign_f as u64, (hi - lo) as u64), pct(sdd_f as u64, (hi - lo) as u64));
        }

        // ── Part A: noise-free 1 ms, forced lock, band [0.05,0.20), 9 arms ───────────────
        {
            let mut err = vec![[[[0u64; 2]; 2]; 2]; 9];
            let mut tot = vec![[[[0u64; 2]; 2]; 2]; 9];
            // per-seed, lock 0, delayed-dominant cell: paired error counts
            let mut per_seed: Vec<[u64; 9]> = Vec::new();
            let mut fire_sign = [[0u64; 2]; 2];
            let mut fire_sdd = [[0u64; 2]; 2];
            let mut fire_thr = [[0u64; 2]; 2];
            let mut seen = [[0u64; 2]; 2];
            // frame-level at the FOUND (noise-free) lock
            let mut lost = [0u64; 9];
            let mut lost_seeds: Vec<Vec<u64>> = vec![Vec::new(); 9];
            let mut lock_hist = std::collections::BTreeMap::new();
            for seed in 0..SEEDS {
                let mut ch = WattersonConfig::moderate_f1(Some(seed));
                ch.snr_db = 200.0;
                let faded = WattersonChannel::new(ch).expect("w").apply(&tx);
                let found = find_timing_offset_with_expected(&faded, n, FC, FS, &expected);
                *lock_hist.entry(found).or_insert(0u64) += 1;
                {
                    let (iv, qv) = demodulate_iq(&faded, n, FC, FS, found);
                    let base: Vec<(f32, f32)> =
                        iv.iter().copied().zip(qv.iter().copied()).collect();
                    let (betas, _, _) = build_betas(&base, &a, WINDOW, seed + 1000, None);
                    for (ai, bet) in betas.iter().enumerate() {
                        let mut iq = base.clone();
                        cancel_variable(&mut iq, bet);
                        let bits = differential_decode(&iq);
                        let (bad, _) = bad_bytes(&bits, &truth, data_lo, data_hi);
                        if bad > RS_T {
                            lost[ai] += 1;
                            lost_seeds[ai].push(seed);
                        }
                    }
                }
                let mut row = [0u64; 9];
                for (li, &lock) in [0usize, DELAY].iter().enumerate() {
                    let env = carrier_envelope(seed, tx.len(), n, lock);
                    let rays = ray_split(seed, tx.len(), n, lock);
                    if env.is_empty() || rays.is_empty() {
                        continue;
                    }
                    let (iv, qv) = demodulate_iq(&faded, n, FC, FS, lock);
                    let base: Vec<(f32, f32)> =
                        iv.iter().copied().zip(qv.iter().copied()).collect();
                    let strata: Vec<u8> = (0..base.len())
                        .map(|k| {
                            let d = env[k.min(env.len() - 1)];
                            if (0.05..0.20).contains(&d) {
                                let (e0, e1) = rays[k.min(rays.len() - 1)];
                                1 + u8::from(e1.norm() > e0.norm())
                            } else {
                                0
                            }
                        })
                        .collect();
                    let (betas, sm, sdm) =
                        build_betas(&base, &a, WINDOW, seed * 7 + li as u64, Some(&strata));
                    let tg = taps_for(&base, &a, WINDOW);
                    for (ai, bet) in betas.iter().enumerate() {
                        let mut iq = base.clone();
                        cancel_variable(&mut iq, bet);
                        let bits = differential_decode(&iq);
                        let lim = bits.len().min(truth.len());
                        for i in 0..lim {
                            let si = (i + 1).min(env.len() - 1);
                            let d = env[si];
                            if !(0.05..0.20).contains(&d) {
                                continue;
                            }
                            let (e0, e1) = rays[si.min(rays.len() - 1)];
                            let dom = usize::from(e1.norm() > e0.norm());
                            let bit = usize::from(truth[i]);
                            tot[ai][li][dom][bit] += 1;
                            let wrong = bits[i] != truth[i];
                            if wrong {
                                err[ai][li][dom][bit] += 1;
                                if li == 0 && dom == 1 {
                                    row[ai] += 1;
                                }
                            }
                            if ai == 0 {
                                seen[li][dom] += 1;
                                let kk = si.min(sm.len() - 1);
                                if sm[kk] {
                                    fire_sign[li][dom] += 1;
                                }
                                if sdm[kk] {
                                    fire_sdd[li][dom] += 1;
                                }
                                if tg[kk][1].norm() < tg[kk][2].norm() {
                                    fire_thr[li][dom] += 1;
                                }
                            }
                        }
                    }
                }
                per_seed.push(row);
            }
            println!("\nPART A  noise-free, 1 ms, forced lock, |H| in [0.05,0.20), {SEEDS} seeds — err b0/b1");
            print!("  lock | dominant ");
            for arm in ARMS {
                print!("| {arm:>11} ");
            }
            println!("| sign fires | sdd fires | thread fires");
            for li in 0..2 {
                for dom in 0..2 {
                    print!(
                        "  {:4} | {:8} ",
                        if li == 0 { 0 } else { DELAY },
                        if dom == 0 { "direct" } else { "delayed" }
                    );
                    for ai in 0..9 {
                        let r = |b: usize| {
                            if tot[ai][li][dom][b] == 0 {
                                f64::NAN
                            } else {
                                err[ai][li][dom][b] as f64 / tot[ai][li][dom][b] as f64
                            }
                        };
                        print!("| {:5.3}/{:5.3} ", r(0), r(1));
                    }
                    println!(
                        "|     {:5.1}% |   {:5.1}% |   {:5.1}%",
                        pct(fire_sign[li][dom], seen[li][dom]),
                        pct(fire_sdd[li][dom], seen[li][dom]),
                        pct(fire_thr[li][dom], seen[li][dom])
                    );
                }
            }
            // paired per-seed comparisons in the product cell (lock 0, delayed)
            let pair = |x: usize, y: usize| {
                let (mut fewer, mut equal, mut more, mut nz) = (0, 0, 0, 0);
                for r in &per_seed {
                    if r[x] + r[y] == 0 {
                        continue;
                    }
                    nz += 1;
                    match r[x].cmp(&r[y]) {
                        std::cmp::Ordering::Less => fewer += 1,
                        std::cmp::Ordering::Equal => equal += 1,
                        _ => more += 1,
                    }
                }
                format!(
                    "{} fewer / {} equal / {} more (of {} seeds with any error)",
                    fewer, equal, more, nz
                )
            };
            println!("\n  PAIRED per seed, lock 0 / delayed cell, errors(X) vs errors(Y):");
            println!("    sign_dd vs hard : {}", pair(5, 1));
            println!("    sign    vs soft : {}", pair(3, 0));
            println!("    sign_dd vs soft : {}", pair(5, 0));
            println!("    sign vs sign_shuf: {}", pair(3, 7));
            println!("    sign_dd vs sdd_shuf: {}", pair(5, 8));
            println!("\n  FRAME LEVEL at the found noise-free lock (lock hist {:?}) — lost = >{} bad payload bytes of 200:", lock_hist, RS_T);
            for ai in 0..9 {
                println!(
                    "    {:>9}: lost {:2}   seeds {:?}",
                    ARMS[ai], lost[ai], lost_seeds[ai]
                );
            }
        }

        // ── Part B: WITH NOISE, found lock, frame level ──────────────────────────────────
        println!("\nPART B  with noise, found lock per frame, {SEEDS} seeds; lost = >{RS_T} bad payload bytes; wrong bits over the payload");
        let cells: [(&str, f32); 12] = [
            ("moderate_f1 1ms", 8.0),
            ("moderate_f1 1ms", 12.0),
            ("moderate_f1 1ms", 16.0),
            ("doppler-only 0ms", 8.0),
            ("doppler-only 0ms", 12.0),
            ("awgn", -2.0),
            ("awgn", -1.0),
            ("awgn", 0.0),
            ("awgn", 2.0),
            ("awgn", 5.0),
            ("awgn", 8.0),
            ("awgn sigma0.9", 20.0 * (tx_rms / 0.9).log10()),
        ];
        for (name, snr) in cells {
            let mut lost = [0u64; 9];
            let mut wrong = [0u64; 9];
            let mut badb = [0u64; 9];
            let (mut f_sign, mut f_sdd, mut n_sym) = (0u64, 0u64, 0u64);
            for seed in 0..SEEDS {
                let faded: Vec<f32> = match name {
                    "moderate_f1 1ms" => {
                        let mut ch = WattersonConfig::moderate_f1(Some(seed));
                        ch.snr_db = snr;
                        WattersonChannel::new(ch).expect("w").apply(&tx)
                    }
                    "doppler-only 0ms" => {
                        let mut ch = WattersonConfig::moderate_f1(Some(seed));
                        ch.snr_db = snr;
                        ch.delay_spread_ms = 0.0;
                        WattersonChannel::new(ch).expect("w").apply(&tx)
                    }
                    _ => AwgnChannel::new(AwgnConfig::new(snr, Some(seed)))
                        .expect("awgn")
                        .apply(&tx),
                };
                let found = find_timing_offset_with_expected(&faded, n, FC, FS, &expected);
                let (iv, qv) = demodulate_iq(&faded, n, FC, FS, found);
                let base: Vec<(f32, f32)> = iv.iter().copied().zip(qv.iter().copied()).collect();
                let (betas, sm, sdm) = build_betas(&base, &a, WINDOW, seed + 77, None);
                for k in data_lo..data_hi.min(sm.len()) {
                    n_sym += 1;
                    if sm[k] {
                        f_sign += 1;
                    }
                    if sdm[k] {
                        f_sdd += 1;
                    }
                }
                for (ai, bet) in betas.iter().enumerate() {
                    let mut iq = base.clone();
                    cancel_variable(&mut iq, bet);
                    let bits = differential_decode(&iq);
                    let (bad, wb) = bad_bytes(&bits, &truth, data_lo, data_hi);
                    wrong[ai] += wb as u64;
                    badb[ai] += bad as u64;
                    if bad > RS_T {
                        lost[ai] += 1;
                    }
                }
            }
            println!(
                "\n  {name} @ {snr:.1} dB   sign fires {:.1}%  sign_dd fires {:.1}% (data region)",
                pct(f_sign, n_sym),
                pct(f_sdd, n_sym)
            );
            print!("    lost  :");
            for ai in 0..9 {
                print!(" {}={:<3}", ARMS[ai], lost[ai]);
            }
            println!();
            print!("    badB  :");
            for ai in 0..9 {
                print!(" {}={:<5}", ARMS[ai], badb[ai]);
            }
            println!();
            print!("    wrongb:");
            for ai in 0..9 {
                print!(" {}={:<6}", ARMS[ai], wrong[ai]);
            }
            println!();
        }
    }
}
