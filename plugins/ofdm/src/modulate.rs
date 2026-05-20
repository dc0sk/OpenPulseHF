//! OFDM modulation: payload → length-prefix → IFFT frames → clip → samples.

use num_complex::Complex32;
use rustfft::FftPlanner;

use crate::channel::is_pilot;
use crate::params::{
    params_for_mode, OfdmParams, CLIP_MAX_ITER, CP, FFT_SIZE, PILOT_AMPLITUDE, TARGET_PAPR_DB,
};

pub fn ofdm_modulate(payload: &[u8], mode: &str) -> Vec<f32> {
    let p = params_for_mode(mode).expect("caller must validate mode before ofdm_modulate");
    modulate_with_params(payload, &p)
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
    // Prepend 2-byte LE length prefix.
    let len_bytes = (payload.len() as u16).to_le_bytes();
    let mut data = Vec::with_capacity(2 + payload.len());
    data.extend_from_slice(&len_bytes);
    data.extend_from_slice(payload);

    let bits = bytes_to_bits(&data);
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
            // Pack 2 bits (QPSK) for this SC.
            let mut sym_bits = 0u8;
            for b in 0..2 {
                if bit_idx < bits.len() {
                    sym_bits |= (bits[bit_idx] as u8) << b;
                    bit_idx += 1;
                }
            }
            let sym = qpsk_mod(sym_bits);
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

    clip_iterative(&out, TARGET_PAPR_DB, CLIP_MAX_ITER)
}

// ── QPSK constellation ────────────────────────────────────────────────────────

const INV_SQRT2: f32 = std::f32::consts::FRAC_1_SQRT_2;

fn qpsk_mod(bits: u8) -> Complex32 {
    match bits & 0x3 {
        0 => Complex32::new(INV_SQRT2, INV_SQRT2),
        1 => Complex32::new(-INV_SQRT2, INV_SQRT2),
        2 => Complex32::new(INV_SQRT2, -INV_SQRT2),
        _ => Complex32::new(-INV_SQRT2, -INV_SQRT2),
    }
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
        let decoded = ofdm_demodulate(&i_ch, "OFDM16");
        assert_eq!(decoded, payload);
    }
}
