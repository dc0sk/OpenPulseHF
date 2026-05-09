//! SC-FDMA modulation: payload → DFT-spread IFFT frames → samples.
//!
//! Unlike OFDM, no PAPR clipping is needed: DFT precoding spreads each
//! symbol across all data subcarriers so the transmitted signal resembles
//! a single-carrier waveform (3–4 dB lower PAPR than plain OFDM).

use num_complex::Complex32;
use rustfft::FftPlanner;

use crate::channel::is_pilot;
use crate::params::{params_for_mode, ScFdmaParams, CP, FFT_SIZE, PILOT_AMPLITUDE, SYM_LEN};

pub fn scfdma_modulate(payload: &[u8], mode: &str) -> Vec<f32> {
    let p = params_for_mode(mode).expect("caller must validate mode before scfdma_modulate");
    modulate_with_params(payload, &p)
}

fn modulate_with_params(payload: &[u8], p: &ScFdmaParams) -> Vec<f32> {
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
    // N_data-point DFT for precoding (may be non-power-of-two; rustfft handles it).
    let dft = planner.plan_fft_forward(p.n_data);
    // 256-point IFFT to convert frequency domain to time domain.
    let ifft = planner.plan_fft_inverse(FFT_SIZE);

    let dft_scale = 1.0 / (p.n_data as f32).sqrt();
    let ifft_scale = 1.0 / (FFT_SIZE as f32).sqrt();

    let mut out = Vec::with_capacity(n_syms * SYM_LEN);
    let mut bit_idx = 0usize;

    for _ in 0..n_syms {
        // Step 1: QPSK-map N_data data bits-pairs into N_data complex symbols.
        let mut data_syms: Vec<Complex32> = (0..p.n_data)
            .map(|_| {
                let mut sym_bits = 0u8;
                for b in 0..2 {
                    if bit_idx < bits.len() {
                        sym_bits |= (bits[bit_idx] as u8) << b;
                        bit_idx += 1;
                    }
                }
                qpsk_mod(sym_bits)
            })
            .collect();

        // Step 2: DFT(N_data) — spread each symbol across all data subcarriers.
        dft.process(&mut data_syms);
        let spread: Vec<Complex32> = data_syms.iter().map(|c| c * dft_scale).collect();

        // Step 3: Place spread symbols and pilots in the 256-bin frequency domain.
        let mut freq = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
        let mut data_idx = 0usize;
        for sc in p.first_sc..=p.last_sc {
            if is_pilot(p, sc) {
                freq[sc] = Complex32::new(PILOT_AMPLITUDE, 0.0);
                freq[FFT_SIZE - sc] = Complex32::new(PILOT_AMPLITUDE, 0.0);
            } else {
                let sym = spread[data_idx];
                data_idx += 1;
                freq[sc] = sym;
                freq[FFT_SIZE - sc] = sym.conj(); // Hermitian symmetry → real output
            }
        }

        // Step 4: IFFT(256) → real time-domain samples.
        ifft.process(&mut freq);
        let time: Vec<f32> = freq.iter().map(|c| c.re * ifft_scale).collect();

        // Step 5: Prepend cyclic prefix (last CP samples).
        let cp_start = FFT_SIZE - CP;
        out.extend_from_slice(&time[cp_start..]);
        out.extend_from_slice(&time);
        // No PAPR clipping — DFT precoding keeps PAPR inherently low.
    }

    out
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
