//! SC-FDMA modulation: payload → DFT-spread IFFT frames → samples.
//!
//! Unlike OFDM, no PAPR clipping is needed: DFT precoding spreads each
//! symbol across all data subcarriers so the transmitted signal resembles
//! a single-carrier waveform (3–4 dB lower PAPR than plain OFDM).

use num_complex::Complex32;
use rustfft::FftPlanner;

use crate::channel::is_pilot;
use crate::params::{params_for_mode, ScFdmaParams, CP, FFT_SIZE, PILOT_AMPLITUDE, SYM_LEN};

const PREAMBLE_SYMBOLS: usize = 4;
const PREAMBLE_PATTERN: &[u8] = b"SCFDMA-SYNC-ACQ";

pub fn scfdma_modulate(payload: &[u8], mode: &str) -> Vec<f32> {
    let p = params_for_mode(mode).expect("caller must validate mode before scfdma_modulate");
    let mut out = modulate_with_params(&preamble_payload(&p), &p);
    out.extend(modulate_with_params(payload, &p));
    out
}

pub(crate) fn preamble_payload(p: &ScFdmaParams) -> Vec<u8> {
    let bytes = (p.bits_per_symbol() * PREAMBLE_SYMBOLS) / 8;
    PREAMBLE_PATTERN
        .iter()
        .copied()
        .cycle()
        .take(bytes)
        .collect()
}

pub(crate) fn modulate_with_params(payload: &[u8], p: &ScFdmaParams) -> Vec<f32> {
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
        // Step 1: map N_data data subcarrier symbols using the selected constellation.
        let mut data_syms: Vec<Complex32> = (0..p.n_data)
            .map(|_| {
                let mut sym_bits = 0u8;
                for b in 0..p.bits_per_sc {
                    if bit_idx < bits.len() {
                        sym_bits |= (bits[bit_idx] as u8) << b;
                        bit_idx += 1;
                    }
                }
                map_symbol(sym_bits, p.bits_per_sc)
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

// ── Constellation mappers ─────────────────────────────────────────────────────

/// Dispatch to the appropriate constellation mapper.
fn map_symbol(bits: u8, bits_per_sc: usize) -> Complex32 {
    match bits_per_sc {
        2 => qpsk_mod(bits),
        3 => psk8_mod(bits),
        4 => qam16_mod(bits),
        5 => qam32_mod(bits),
        6 => qam64_mod(bits),
        _ => qpsk_mod(bits),
    }
}

const INV_SQRT2: f32 = std::f32::consts::FRAC_1_SQRT_2;

fn qpsk_mod(bits: u8) -> Complex32 {
    match bits & 0x3 {
        0 => Complex32::new(INV_SQRT2, INV_SQRT2),
        1 => Complex32::new(-INV_SQRT2, INV_SQRT2),
        2 => Complex32::new(INV_SQRT2, -INV_SQRT2),
        _ => Complex32::new(-INV_SQRT2, -INV_SQRT2),
    }
}

/// Gray-coded 8PSK: 8 phases equally spaced on the unit circle.
///
/// Natural index k → angle k×π/4.  Gray coding: label = k XOR (k>>1).
/// Mean power = 1 (all points on unit circle).
fn psk8_mod(bits: u8) -> Complex32 {
    let k = gray3_to_natural(bits);
    let angle = k as f32 * std::f32::consts::FRAC_PI_4;
    Complex32::new(angle.cos(), angle.sin())
}

/// Convert a 3-bit Gray code to a natural (binary) index.
pub(crate) fn gray3_to_natural(g: u8) -> u8 {
    let g = g & 0x7;
    let b2 = (g >> 2) & 1;
    let b1 = ((g >> 1) ^ b2) & 1;
    let b0 = (g ^ b1) & 1;
    (b2 << 2) | (b1 << 1) | b0
}

/// Convert a natural (binary) 3-bit index to its Gray code.
pub(crate) fn natural3_to_gray(n: u8) -> u8 {
    (n ^ (n >> 1)) & 0x7
}

/// Gray-coded 16QAM: PAM-4 on I and Q, normalised to unit average power.
///
/// PAM-4 Gray encoding (2 bits → amplitude):
/// 00 → −3,  01 → −1,  11 → +1,  10 → +3   (×scale)
///
/// Scale = 1/√10 so average power per axis = (9+1+1+9)/4 = 5 → total = 10 → unit.
fn qam16_mod(bits: u8) -> Complex32 {
    const SCALE: f32 = 0.316_227_77; // 1/sqrt(10)
    fn pam4(g: u8) -> f32 {
        match g & 0x3 {
            0b00 => -3.0,
            0b01 => -1.0,
            0b11 => 1.0,
            _ => 3.0, // 0b10
        }
    }
    let i = pam4((bits >> 2) & 0x3) * SCALE;
    let q = pam4(bits & 0x3) * SCALE;
    Complex32::new(i, q)
}

/// Cross-32QAM modulator.
///
/// The constellation is a 6×6 PAM grid with the four corner points (|I|=5 and |Q|=5) removed,
/// giving 32 points.  Mean power = 20 → scale = 1/√20.
///
/// 5-bit input is a standard 5-bit Gray code.  The Gray index maps to a spatial position
/// in a raster-scan ordering (Q=5..−5 top-to-bottom, I=−5..+5 left-to-right), which gives
/// single-bit transitions for all horizontally adjacent constellation neighbours.
fn qam32_mod(bits: u8) -> Complex32 {
    let (i_raw, q_raw) = QAM32_SPATIAL[gray5_to_natural(bits) as usize];
    Complex32::new(i_raw as f32 * QAM32_SCALE, q_raw as f32 * QAM32_SCALE)
}

/// 1/√20 — normalization scale for cross-32QAM (mean power per complex sample = 1).
pub(crate) const QAM32_SCALE: f32 = 0.223_606_8; // 1/sqrt(20)

/// Raster-scan ordering of the 32 cross-32QAM points: Q=5 → −5, I=−5 → +5.
///
/// Spatial index → (I_unnorm, Q_unnorm).  Corners (|I|=5 and |Q|=5) are absent.
pub(crate) const QAM32_SPATIAL: [(i8, i8); 32] = [
    // Q = 5 (4 pts)
    (-3, 5),
    (-1, 5),
    (1, 5),
    (3, 5),
    // Q = 3 (6 pts)
    (-5, 3),
    (-3, 3),
    (-1, 3),
    (1, 3),
    (3, 3),
    (5, 3),
    // Q = 1 (6 pts)
    (-5, 1),
    (-3, 1),
    (-1, 1),
    (1, 1),
    (3, 1),
    (5, 1),
    // Q = −1 (6 pts)
    (-5, -1),
    (-3, -1),
    (-1, -1),
    (1, -1),
    (3, -1),
    (5, -1),
    // Q = −3 (6 pts)
    (-5, -3),
    (-3, -3),
    (-1, -3),
    (1, -3),
    (3, -3),
    (5, -3),
    // Q = −5 (4 pts)
    (-3, -5),
    (-1, -5),
    (1, -5),
    (3, -5),
];

/// Convert a 5-bit Gray code to a natural (binary) index.
pub(crate) fn gray5_to_natural(g: u8) -> u8 {
    let g = g & 0x1f;
    let b4 = (g >> 4) & 1;
    let b3 = ((g >> 3) ^ b4) & 1;
    let b2 = ((g >> 2) ^ b3) & 1;
    let b1 = ((g >> 1) ^ b2) & 1;
    let b0 = (g ^ b1) & 1;
    (b4 << 4) | (b3 << 3) | (b2 << 2) | (b1 << 1) | b0
}

/// Convert a natural (binary) 5-bit index to its Gray code.
pub(crate) fn natural5_to_gray(n: u8) -> u8 {
    (n ^ (n >> 1)) & 0x1f
}

/// Gray-coded 64QAM: PAM-8 on I and Q, normalised to unit average power.
///
/// Reuses the same 3-bit Gray → PAM-8 mapping as the qam64-plugin:
/// 000→−7, 001→−5, 011→−3, 010→−1, 110→+1, 111→+3, 101→+5, 100→+7  (×scale)
///
/// Scale = 1/√42 so average power = 21 per axis → 42 total → unit.
fn qam64_mod(bits: u8) -> Complex32 {
    const SCALE: f32 = 0.154_303_35; // 1/sqrt(42)
    fn pam8(g: u8) -> f32 {
        let raw: i8 = match g & 0x7 {
            0b000 => -7,
            0b001 => -5,
            0b011 => -3,
            0b010 => -1,
            0b110 => 1,
            0b111 => 3,
            0b101 => 5,
            _ => 7, // 0b100
        };
        raw as f32 * SCALE
    }
    let i = pam8((bits >> 3) & 0x7);
    let q = pam8(bits & 0x7);
    Complex32::new(i, q)
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psk8_average_power_is_unit() {
        let total: f32 = (0..8u8).map(|b| psk8_mod(b).norm_sqr()).sum::<f32>() / 8.0;
        assert!(
            (total - 1.0).abs() < 0.01,
            "8PSK average power = {total:.4}"
        );
    }

    #[test]
    fn psk8_all_points_distinct() {
        let points: Vec<(i32, i32)> = (0..8u8)
            .map(|b| {
                let c = psk8_mod(b);
                ((c.re * 1000.0) as i32, (c.im * 1000.0) as i32)
            })
            .collect();
        for i in 0..points.len() {
            for j in (i + 1)..points.len() {
                assert_ne!(points[i], points[j], "8PSK points {i} and {j} collide");
            }
        }
    }

    #[test]
    fn gray3_round_trip() {
        for n in 0u8..8 {
            assert_eq!(
                gray3_to_natural(natural3_to_gray(n)),
                n,
                "round-trip failed for n={n}"
            );
        }
    }

    #[test]
    fn qam16_average_power_is_unit() {
        let total: f32 = (0..16u8).map(|b| qam16_mod(b).norm_sqr()).sum::<f32>() / 16.0;
        assert!(
            (total - 1.0).abs() < 0.01,
            "16QAM average power = {total:.4}"
        );
    }

    #[test]
    fn qam16_all_points_distinct() {
        let points: Vec<(i32, i32)> = (0..16u8)
            .map(|b| {
                let c = qam16_mod(b);
                ((c.re * 1000.0) as i32, (c.im * 1000.0) as i32)
            })
            .collect();
        for i in 0..points.len() {
            for j in (i + 1)..points.len() {
                assert_ne!(points[i], points[j], "16QAM points {i} and {j} collide");
            }
        }
    }

    #[test]
    fn qam32_average_power_is_unit() {
        let total: f32 = (0..32u8).map(|b| qam32_mod(b).norm_sqr()).sum::<f32>() / 32.0;
        assert!(
            (total - 1.0).abs() < 0.01,
            "32QAM average power = {total:.4}"
        );
    }

    #[test]
    fn qam32_all_points_distinct() {
        let points: Vec<(i32, i32)> = (0..32u8)
            .map(|b| {
                let c = qam32_mod(b);
                ((c.re * 1000.0) as i32, (c.im * 1000.0) as i32)
            })
            .collect();
        for i in 0..points.len() {
            for j in (i + 1)..points.len() {
                assert_ne!(points[i], points[j], "32QAM points {i} and {j} collide");
            }
        }
    }

    #[test]
    fn gray5_round_trip() {
        for n in 0u8..32 {
            assert_eq!(
                gray5_to_natural(natural5_to_gray(n)),
                n,
                "round-trip failed for n={n}"
            );
        }
    }

    #[test]
    fn qam64_average_power_is_unit() {
        let total: f32 = (0..64u8).map(|b| qam64_mod(b).norm_sqr()).sum::<f32>() / 64.0;
        assert!(
            (total - 1.0).abs() < 0.01,
            "64QAM average power = {total:.4}"
        );
    }

    #[test]
    fn qam64_all_points_distinct() {
        let points: Vec<(i32, i32)> = (0..64u8)
            .map(|b| {
                let c = qam64_mod(b);
                ((c.re * 1000.0) as i32, (c.im * 1000.0) as i32)
            })
            .collect();
        for i in 0..points.len() {
            for j in (i + 1)..points.len() {
                assert_ne!(points[i], points[j], "64QAM points {i} and {j} collide");
            }
        }
    }
}
