use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, PreambleTemplate, PulseShape};
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
    if crate::is_differential(&config.mode) {
        symbols.extend(differential_encode(&bytes_to_bits(data)));
    } else {
        symbols.extend(bits_to_symbols(&bytes_to_bits(data)));
    }
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

/// The modulated preamble alone, plus the ρ constants measured for `config.mode` (#1053).
///
/// `None` where no correlation veto is safe for the mode — see [`preamble_rho_threshold`].
///
/// Built by modulating an empty payload and keeping the preamble span, so it comes out of the
/// **same** code path as a real transmission. A hand-rolled copy drifts out of step with the
/// modulator the first time either changes, and a template that no longer matches the wire stops
/// corroborating settles silently rather than failing.
///
/// The last preamble symbol is dropped: the plain QPSK pulse is a raised-cosine *crossfade*, so the
/// final preamble symbol period already carries a third of the first data symbol (see the
/// crossfade-ISI sharp edge in `CLAUDE.md`), which differs frame to frame.
pub fn qpsk_preamble_template(
    config: &ModulationConfig,
) -> Result<Option<PreambleTemplate>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let Some(threshold) = preamble_rho_threshold(&config.mode, baud) else {
        return Ok(None);
    };
    let n = samples_per_symbol(config.sample_rate as f32, baud)?;
    let full = qpsk_modulate(&[], config)?;
    let span = n * (PREAMBLE_SYMS - 1);
    if full.len() < span {
        return Err(ModemError::Demodulation(
            "modulated preamble shorter than its own symbol span".into(),
        ));
    }
    Ok(Some(PreambleTemplate::new(
        full[..span].to_vec(),
        threshold,
        PREAMBLE_RHO_GRID_HZ,
    )))
}

/// Half-width of the residual-frequency grid the preamble correlation searches, in Hz.
///
/// Bounded below by the AFC settle's residual (≤ 0.3 Hz measured), which is what this has to cover
/// — the settle supplies the frequency, the correlation confirms the waveform.
///
/// **The upper bound that constrains BPSK does not constrain QPSK, and that is a measured
/// difference, not an oversight.** BPSK's preamble is 32 *alternating* symbols: a square-wave
/// -modulated carrier with two spectral lines at `fc ± baud/2`, so a grid reaching that far rotates
/// a line onto plain carrier and a steady tone starts scoring like a preamble (0.017 at ±20 Hz →
/// 0.659 at ±160 Hz). QPSK's preamble is an *aperiodic* designed sequence with no such structure,
/// and a tone's ρ barely moves with grid width — measured across QPSK125/250/500, ±20 Hz through
/// ±450 Hz: 0.014→0.524, 0.316→0.524, 0.506→0.524, i.e. the correlator finds a partial alignment at
/// any width. So the tone score is a property of the sequence here, not of the grid.
///
/// It stays at ±20 Hz anyway, because nothing asks for more and every extra grid point is another
/// full correlation *and* another maximisation that lifts the noise floor of ρ itself.
pub const PREAMBLE_RHO_GRID_HZ: f32 = 20.0;

/// Minimum normalised preamble correlation ρ for a settle to be believed, per QPSK mode (#1053).
///
/// `None` means the mode publishes no template and keeps the pre-#1049 energy-only settle.
///
/// **Nothing here is inherited from BPSK, and the reason is arithmetic.** ρ is normalised, so its
/// noise floor is fixed by the template's length: measured over the recorded idle corpus it tracks
/// `≈ 6.5/√len` for both waveforms (BPSK250's 992-sample template reads 0.205; QPSK125's 960-sample
/// one reads 0.216). QPSK's preamble is 16 symbols to BPSK's 32, so at the same baud its template is
/// half as long and its noise ceiling is √2 higher — and BPSK's 0.40 threshold sits *below*
/// QPSK500's noise ceiling of 0.429. Copying it would have corroborated settles on pure noise.
///
/// Measured 2026-08-01. Noise = every window of `ic9700-idle-hot.wav` and `ft991a-idle.wav` through
/// the engine's own window/grid; decode = `Rs`-coded frames through AWGN, ρ taken at the true onset:
///
/// | mode | template | idle-noise ceiling | weakest ρ that decodes | threshold | margins |
/// |---|---|---|---|---|---|
/// | QPSK125 | 960 | 0.216 | 0.557 (−4 dB) | **0.35** | 1.62× / 1.59× |
/// | QPSK250(-D) | 480 | 0.291 | 0.811 (−D, 3 dB) | **0.45** | 1.55× / 1.80× |
/// | QPSK500(-D) | 240 | 0.429 | 0.820 (2 dB) | **0.60** | 1.40× / 1.37× |
/// | QPSK1000 | 120 | 0.581 | 0.879 (5 dB) | *none* | 1.23× / 1.23× |
///
/// **QPSK1000 and faster publish nothing on purpose.** Their gap is too narrow to place a threshold
/// in: the geometric mean leaves 1.23× on each side, and the decode column is AWGN — a fade lowers
/// it while a hotter band raises the noise column, so the two sides close from both directions. An
/// energy-only settle is worse than a working veto but better than one that vetoes real frames.
/// Buying those modes a veto needs more processing gain in the template, i.e. a longer sync word:
/// a wire-format change, tracked with the PN/chirp preamble work.
///
/// The `-RRC` modes are excluded for a different reason: the RRC pulse spans
/// `RRC_SPAN_SYMBOLS` = 12 symbols, so the first data symbols smear back over the preamble tail and
/// the "drop the last symbol" rule that makes the crossfade template clean does not hold. Nothing
/// about them was measured, so nothing is claimed.
///
/// What would falsify a row: a channel where that mode decodes at ρ below its threshold, or a band
/// floor whose ρ ceiling exceeds it. Both are re-measurable with
/// `tests/qpsk_preamble_rho_survey.rs`.
fn preamble_rho_threshold(mode: &str, baud: f32) -> Option<f32> {
    // Only the plain (crossfade) pulse is measured; -RRC and -HF shape the preamble differently.
    if mode.ends_with("-RRC") || mode.ends_with("-HF") {
        return None;
    }
    match baud {
        b if b <= 125.0 => Some(0.35),
        b if b <= 250.0 => Some(0.45),
        b if b <= 500.0 => Some(0.60),
        _ => None,
    }
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
    if crate::is_differential(&config.mode) {
        symbols.extend(differential_encode(&bytes_to_bits(data)));
    } else {
        symbols.extend(bits_to_symbols(&bytes_to_bits(data)));
    }
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
    if crate::is_differential(&config.mode) {
        symbols.extend(differential_encode(&bytes_to_bits(data)));
    } else {
        symbols.extend(bits_to_symbols(&bytes_to_bits(data)));
    }
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

/// Gray-coded dibit → rotation index (units of 90°): 00→0, 01→1, 11→2, 10→3.
/// Adjacent rotations differ by one bit, so a ±90° carrier slip corrupts one bit, not two.
pub(crate) fn rotation_index(b0: bool, b1: bool) -> u8 {
    match (b0, b1) {
        (false, false) => 0,
        (false, true) => 1,
        (true, true) => 2,
        (true, false) => 3,
    }
}

/// Inverse of [`rotation_index`] — kept adjacent to it so the Gray table cannot drift out of sync
/// between the encoder and the differential decoder.
pub(crate) fn dibit_from_rotation_index(r: u8) -> (bool, bool) {
    match r & 0b11 {
        0 => (false, false),
        1 => (false, true),
        2 => (true, true),
        _ => (true, false),
    }
}

/// Gray-coded dibit → phase rotation (radians).
pub(crate) fn rotation_rad(b0: bool, b1: bool) -> f32 {
    rotation_index(b0, b1) as f32 * std::f32::consts::FRAC_PI_2
}

/// Differential QPSK encode: the dibit selects a phase *increment* from the previous
/// symbol, so a slow fade-induced rotation cancels in the receiver's symbol-to-symbol
/// difference (the same immunity BPSK's NRZI decode has). The reference is the last
/// preamble symbol, so the first data symbol needs no extra pilot.
pub(crate) fn differential_encode(bits: &[bool]) -> Vec<(f32, f32)> {
    // Derive the reference from the preamble rather than hardcoding its angle: the decoder
    // differences against the received preamble's last symbol, so a preamble change must move
    // both ends together or every `-D` frame silently decodes to noise.
    let (ref_i, ref_q) = preamble_symbols()
        .last()
        .copied()
        .unwrap_or((INV_SQRT_2, INV_SQRT_2));
    let mut phase = ref_q.atan2(ref_i);
    let mut out = Vec::with_capacity(bits.len().div_ceil(2));
    for pair in bits.chunks(2) {
        let b0 = pair.first().copied().unwrap_or(false);
        let b1 = pair.get(1).copied().unwrap_or(false);
        phase += rotation_rad(b0, b1);
        out.push((phase.cos(), phase.sin()));
    }
    out
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
