//! OFDM modulation: payload → length-prefix → IFFT frames → clip → samples.

use num_complex::Complex32;
use rustfft::FftPlanner;

use crate::channel::is_pilot;
use openpulse_core::len_prefix::{encode_len_prefix, LEN_PREFIX_BYTES};
use openpulse_dsp::constellation::map_symbol;

use crate::params::{
    params_for_mode, preamble_sign, OfdmParams, CLIP_MAX_ITER, CP, FFT_SIZE, PILOT_AMPLITUDE,
    PREAMBLE_AMPLITUDE, TARGET_PAPR_DB,
};

pub fn ofdm_modulate(payload: &[u8], mode: &str) -> Vec<f32> {
    match params_for_mode(mode) {
        Some(p) => modulate_with_params(payload, &p),
        None => vec![],
    }
}

/// Return interleaved I/Q samples at complex baseband (fc = 0 Hz).
///
/// OFDM uses Hermitian symmetry so the IFFT output is real; the Q channel is
/// identically zero.  Interleaved layout: [I₀, Q₀, I₁, Q₁, …].
pub fn ofdm_modulate_iq(payload: &[u8], mode: &str) -> Vec<f32> {
    let real = ofdm_modulate(payload, mode);
    real.iter().flat_map(|&s| [s, 0.0_f32]).collect()
}

fn modulate_with_params(payload: &[u8], p: &OfdmParams) -> Vec<f32> {
    // Prepend the majority-protected length prefix (3 LE copies).
    let len_bytes = encode_len_prefix(payload.len() as u16);
    let mut data = Vec::with_capacity(LEN_PREFIX_BYTES + payload.len());
    data.extend_from_slice(&len_bytes);
    data.extend_from_slice(payload);

    // Whiten the bit stream so no payload (zero-runs, RS padding) can produce an all-identical-subcarrier
    // impulse-train symbol that the engine's CE-SSB peak-stretch would crush. See `crate::scramble`.
    let mut bits = bytes_to_bits(&data);
    crate::scramble::scramble_bits(&mut bits);
    let bits_per_sym = p.bits_per_symbol();
    let n_syms = if bits_per_sym == 0 {
        1
    } else {
        bits.len().div_ceil(bits_per_sym)
    };

    let mut planner = FftPlanner::<f32>::new();
    let ifft = planner.plan_fft_inverse(FFT_SIZE);

    let scale = 1.0 / (FFT_SIZE as f32).sqrt();
    let mut out = Vec::with_capacity(n_syms * (FFT_SIZE + CP));
    let mut bit_idx = 0usize;

    for _ in 0..n_syms {
        let mut freq = vec![Complex32::new(0.0, 0.0); FFT_SIZE];

        for sc in p.first_sc..=p.last_sc {
            if is_pilot(p, sc) {
                // Pilot: known BPSK +1 at positive frequency; conjugate at mirror.
                freq[sc] = Complex32::new(PILOT_AMPLITUDE, 0.0);
                freq[FFT_SIZE - sc] = Complex32::new(PILOT_AMPLITUDE, 0.0);
                continue;
            }
            // Pack bits_per_sc bits for this subcarrier and map to its constellation.
            let mut sym_bits = 0u8;
            for b in 0..p.bits_per_sc {
                if bit_idx < bits.len() {
                    sym_bits |= (bits[bit_idx] as u8) << b;
                    bit_idx += 1;
                }
            }
            let sym = map_symbol(sym_bits, p.bits_per_sc);
            freq[sc] = sym;
            // Hermitian symmetry → real IFFT output.
            freq[FFT_SIZE - sc] = sym.conj();
        }

        // DC and Nyquist remain zero.
        ifft.process(&mut freq);

        let time: Vec<f32> = freq.iter().map(|c| c.re * scale).collect();

        // Cyclic prefix: last CP samples of the symbol body.
        let cp_start = FFT_SIZE - CP;
        out.extend_from_slice(&time[cp_start..]);
        out.extend_from_slice(&time);
    }

    // Clip the data symbols on their own so their post-clip SNR is identical to a
    // no-preamble waveform — then prepend the timing-acquisition preamble, itself
    // clipped separately so its high comb-PAPR cannot raise the data clip
    // threshold (which would otherwise degrade the data subcarriers).
    //
    // Only QPSK is clipped.  Clipping injects broadband distortion that the dense
    // higher-order constellations cannot absorb — it breaks 64QAM even on a clean
    // channel (its minimum distance is too small).  Higher-order OFDM instead keeps
    // its natural ~12 dB PAPR and relies on TX leveling/backoff (the same backoff
    // SSB rigs already apply), which is the strategy's premise for OFDM HOM.
    let data = if p.bits_per_sc == 2 {
        clip_iterative(&out, TARGET_PAPR_DB, CLIP_MAX_ITER)
    } else {
        out
    };
    let mut preamble = clip_iterative(
        &build_preamble(p, &ifft, scale),
        TARGET_PAPR_DB,
        CLIP_MAX_ITER,
    );

    // Equalize section RMS: each section is clipped to the PAPR target against
    // its OWN rms, but the frame PAPR is measured against the POOLED rms — a
    // quieter section dilutes the pool and pushes the frame PAPR above target.
    // Scaling the preamble to the data rms leaves both acquisition stages
    // unaffected (Schmidl-Cox M(d) and the matched-filter ρ are scale-
    // invariant) and guarantees frame PAPR = max(section PAPRs) ≤ target.
    let rms = |x: &[f32]| (x.iter().map(|&s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt();
    let (rms_pre, rms_data) = (rms(&preamble), rms(&data));
    if rms_pre > 1e-9 && rms_data > 1e-9 {
        let k = rms_data / rms_pre;
        for s in &mut preamble {
            *s *= k;
        }
    }

    let mut frame = Vec::with_capacity(preamble.len() + data.len());
    frame.extend_from_slice(&preamble);
    frame.extend_from_slice(&data);

    // DAC-safe peak normalisation for the un-clipped higher-order modes.  Without
    // clipping, OFDM's ~12 dB PAPR drives peaks past full scale, so a real DAC
    // hard-clips them and shreds the dense constellation (acquisition still locks,
    // but 16QAM/64QAM decode to garbage on hardware).  Scaling the whole frame so
    // its peak sits at 0.9 fits the DAC WITHOUT clipping distortion — the inherent
    // PAPR backoff, applied uniformly so the demodulator (which equalizes) is
    // unaffected.  QPSK keeps its clip-bounded peak (it already fits).
    if p.bits_per_sc != 2 {
        let peak = frame.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        if peak > 0.9 {
            let g = 0.9 / peak;
            for s in &mut frame {
                *s *= g;
            }
        }
    }
    frame
}

/// The clean (pre-clip) preamble symbol waveform, used by the receiver as a
/// matched-filter template for sample-accurate timing acquisition.
pub(crate) fn preamble_template(p: &OfdmParams) -> Vec<f32> {
    let mut planner = FftPlanner::<f32>::new();
    let ifft = planner.plan_fft_inverse(FFT_SIZE);
    let scale = 1.0 / (FFT_SIZE as f32).sqrt();
    build_preamble(p, &ifft, scale)
}

/// Build the timing-acquisition preamble symbol (CP + body) in the time domain.
///
/// Only even occupied subcarriers are loaded (with a known BPSK PN sequence and
/// Hermitian symmetry), giving an IFFT output whose two halves are identical.
fn build_preamble(
    p: &OfdmParams,
    ifft: &std::sync::Arc<dyn rustfft::Fft<f32>>,
    scale: f32,
) -> Vec<f32> {
    let mut freq = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
    for sc in p.first_sc..=p.last_sc {
        if !sc.is_multiple_of(2) {
            continue;
        }
        let val = Complex32::new(PREAMBLE_AMPLITUDE * preamble_sign(sc), 0.0);
        freq[sc] = val;
        freq[FFT_SIZE - sc] = val.conj();
    }
    ifft.process(&mut freq);
    let time: Vec<f32> = freq.iter().map(|c| c.re * scale).collect();
    let cp_start = FFT_SIZE - CP;
    let mut out = Vec::with_capacity(FFT_SIZE + CP);
    out.extend_from_slice(&time[cp_start..]);
    out.extend_from_slice(&time);
    out
}

// ── Bit packing ───────────────────────────────────────────────────────────────

pub fn bytes_to_bits(bytes: &[u8]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for shift in 0..8u8 {
            bits.push((b >> shift) & 1 == 1);
        }
    }
    bits
}

// ── PAPR clipping ─────────────────────────────────────────────────────────────

/// Iterative clip until peak/RMS ≤ `10^(target_db/20)` or `max_iter` reached.
///
/// Energy is monotonically non-increasing each iteration, so convergence is
/// guaranteed.
pub fn clip_iterative(samples: &[f32], target_papr_db: f32, max_iter: usize) -> Vec<f32> {
    if samples.is_empty() {
        return vec![];
    }
    let mut signal = samples.to_vec();
    let factor = 10.0_f32.powf(target_papr_db / 20.0);
    for _ in 0..max_iter {
        let mean_sq = signal.iter().map(|&s| s * s).sum::<f32>() / signal.len() as f32;
        let rms = mean_sq.sqrt();
        let threshold = rms * factor;
        let peak = signal.iter().map(|&s| s.abs()).fold(0.0_f32, f32::max);
        if peak <= threshold * 1.001 {
            break;
        }
        for s in &mut signal {
            *s = s.clamp(-threshold, threshold);
        }
    }
    signal
}

/// Measure PAPR in dB.
pub fn measure_papr(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let peak_sq = samples.iter().map(|&s| s * s).fold(0.0_f32, f32::max);
    let mean_sq = samples.iter().map(|&s| s * s).sum::<f32>() / samples.len() as f32;
    if mean_sq < 1e-12 {
        return 0.0;
    }
    10.0 * (peak_sq / mean_sq).log10()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demodulate::ofdm_demodulate;

    #[test]
    fn clip_reduces_papr_to_target() {
        let samples = ofdm_modulate(b"papr test payload for clipping", "OFDM52");
        // Clipping is applied inside ofdm_modulate; verify result is within target.
        let papr = measure_papr(&samples);
        assert!(papr <= TARGET_PAPR_DB + 0.5, "PAPR={papr:.1} dB > target");
    }

    #[test]
    fn ofdm16_iq_i_channel_matches_modulate() {
        let payload = b"IQ test payload";
        let iq = ofdm_modulate_iq(payload, "OFDM16");
        let real = ofdm_modulate(payload, "OFDM16");
        assert_eq!(iq.len(), real.len() * 2);
        let i_ch: Vec<f32> = iq.iter().step_by(2).copied().collect();
        assert_eq!(i_ch, real, "I channel must match ofdm_modulate output");
        let q_ch: Vec<f32> = iq.iter().skip(1).step_by(2).copied().collect();
        assert!(q_ch.iter().all(|&q| q == 0.0), "Q channel must be zero");
    }

    #[test]
    fn ofdm16_iq_round_trip() {
        let payload = b"round trip IQ ofdm16";
        let iq = ofdm_modulate_iq(payload, "OFDM16");
        let i_ch: Vec<f32> = iq.iter().step_by(2).copied().collect();
        let decoded = ofdm_demodulate(&i_ch, "OFDM16").expect("demodulate");
        assert_eq!(decoded, payload);
    }
}
