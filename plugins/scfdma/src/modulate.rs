//! SC-FDMA modulation: payload → DFT-spread IFFT frames → samples.
//!
//! Unlike OFDM, no PAPR clipping is needed: DFT precoding spreads each
//! symbol across all data subcarriers so the transmitted signal resembles
//! a single-carrier waveform (3–4 dB lower PAPR than plain OFDM).

use num_complex::Complex32;
use openpulse_core::len_prefix::{encode_len_prefix, LEN_PREFIX_BYTES};
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

/// Return interleaved I/Q samples at complex baseband (fc = 0 Hz).
///
/// SC-FDMA uses Hermitian symmetry so the IFFT output is real; the Q channel is
/// identically zero.  Interleaved layout: [I₀, Q₀, I₁, Q₁, …].
pub fn scfdma_modulate_iq(payload: &[u8], mode: &str) -> Vec<f32> {
    let real = scfdma_modulate(payload, mode);
    real.iter().flat_map(|&s| [s, 0.0_f32]).collect()
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
    // Prepend the majority-protected length prefix (3 LE copies).
    let len_bytes = encode_len_prefix(payload.len() as u16);
    let mut data = Vec::with_capacity(LEN_PREFIX_BYTES + payload.len());
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

// ── Constellation mapping (shared with all multicarrier plugins) ──────────────
//
// The Gray map, hard demapper, and soft-LLR math live in
// `openpulse_dsp::constellation` (QPSK/8PSK/16QAM/32QAM-cross/64QAM, byte-for-byte
// the mapping these modes were validated against).
use openpulse_dsp::constellation::map_symbol;

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

/// Measure PAPR in dB on the real passband signal (peak/mean over the record).
///
/// NOTE: this is a real-bandpass record-max — it bakes in ~3 dB of carrier term (a constant-envelope
/// tone reads ~3 dB, not 0) and is a high-variance single sample. For PA-relevant PAPR use
/// [`measure_envelope_papr_ccdf`], which is what low-PAPR mode decisions should be judged on.
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

/// PA-relevant PAPR in dB: the complex-envelope power exceeded with probability `quantile` (the CCDF
/// point), over the mean envelope power.
///
/// The instantaneous envelope is `|x + j·H{x}|` (analytic signal via the FFT Hilbert transform), so
/// this removes the carrier term that inflates [`measure_papr`] by ~3 dB and gives the quantity a
/// linear PA actually backs off against. Using a CCDF quantile (e.g. 1e-3) instead of the record max
/// makes it low-variance and length-robust. Over an SSB radio the transmitted audio is real, so this
/// captures the true envelope excursion without needing an IQ/complex-baseband path.
pub fn measure_envelope_papr_ccdf(samples: &[f32], quantile: f32) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let q = openpulse_dsp::acquisition::quadrature(samples);
    let mut inst: Vec<f32> = samples
        .iter()
        .zip(q.iter())
        .map(|(&i, &qq)| i * i + qq * qq)
        .collect();
    let mean = inst.iter().sum::<f32>() / inst.len() as f32;
    if mean < 1e-12 {
        return 0.0;
    }
    inst.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let quantile = quantile.clamp(0.0, 1.0);
    let idx = (((1.0 - quantile) * inst.len() as f32) as usize).min(inst.len() - 1);
    10.0 * (inst[idx] / mean).log10()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demodulate::scfdma_demodulate;
    use crate::params::{ScFdmaParams, SCFDMA52, SCFDMA52_LP};

    // Constellation-property tests (unit power, point distinctness, Gray round-trips)
    // live with the shared mapper in `openpulse_dsp::constellation`.

    /// The envelope-CCDF metric strips the carrier term that inflates the real-passband max: a
    /// constant-envelope tone reads ~0 dB envelope-CCDF but ~3 dB real-passband, and SCFDMA52's
    /// envelope PAPR sits well below its real-passband number.
    #[test]
    fn envelope_ccdf_removes_carrier_bakein() {
        // 1500 Hz tone at 8 kHz — constant envelope.
        let tone: Vec<f32> = (0..8000)
            .map(|n| (std::f32::consts::TAU * 1500.0 * n as f32 / 8000.0).sin())
            .collect();
        let real = measure_papr(&tone);
        let env = measure_envelope_papr_ccdf(&tone, 1e-3);
        assert!(
            real > 2.5,
            "real-passband tone PAPR should be ~3 dB: {real:.2}"
        );
        assert!(
            env.abs() < 0.7,
            "constant-envelope tone CCDF should be ~0 dB: {env:.2}"
        );

        // A real SCFDMA52 frame: the PA-relevant envelope figure is well below the real-passband max.
        let payload = b"envelope ccdf vs real passband papr for scfdma52";
        let frame = scfdma_modulate(payload, "SCFDMA52");
        assert!(
            measure_envelope_papr_ccdf(&frame, 1e-3) < measure_papr(&frame) - 1.5,
            "envelope CCDF should be materially below the real-passband max"
        );
    }

    /// Attribution guard for SCFDMA52-LP's PAPR win: it is MOSTLY pilot dilution (4 vs 13 pilots),
    /// NOT the localized contiguous mapping. Locks the honest narrative against drift.
    #[test]
    fn papr_ablation() {
        let papr = |p: &ScFdmaParams| -> f32 {
            (0..16u32)
                .map(|seed| {
                    let payload: Vec<u8> = (0..200)
                        .map(|i| {
                            ((i as u32)
                                .wrapping_mul(2_654_435_761)
                                .wrapping_add(seed * 7)) as u8
                        })
                        .collect();
                    measure_papr(&modulate_with_params(&payload, p))
                })
                .sum::<f32>()
                / 16.0
        };
        // Controls: 4 interleaved pilots (non-contiguous data), and 13 block pilots (contiguous).
        let four_interleaved = ScFdmaParams {
            n_data: 61,
            n_pilots: 4,
            pilot_spacing: 16,
            ..SCFDMA52
        };
        let thirteen_block = ScFdmaParams {
            localized: true,
            ..SCFDMA52
        };
        let (p13i, p4i, p13b, plp) = (
            papr(&SCFDMA52),
            papr(&four_interleaved),
            papr(&thirteen_block),
            papr(&SCFDMA52_LP),
        );
        // Dropping 13→4 pilots (mapping still interleaved) captures most of the reduction...
        let dilution = p13i - p4i;
        // ...while localizing the data (4 interleaved → 4 block) adds only a little.
        let localization = p4i - plp;
        assert!(
            dilution > 1.0,
            "pilot dilution should give >1 dB: {dilution:.2}"
        );
        assert!(
            dilution > localization,
            "pilot dilution ({dilution:.2}) must dominate localization ({localization:.2})"
        );
        // Contiguous data WITH 13 pilots recovers ~nothing — refutes "interleaved pilots are the
        // PAPR root cause": pilot count, not placement, dominates.
        assert!(
            p13b > p13i - 0.5,
            "contiguous-data + 13 pilots ({p13b:.2}) should be ~= interleaved ({p13i:.2}), not lower"
        );
    }

    #[test]
    fn scfdma52_iq_i_channel_matches_modulate() {
        let payload = b"IQ test payload";
        let iq = scfdma_modulate_iq(payload, "SCFDMA52");
        let real = scfdma_modulate(payload, "SCFDMA52");
        assert_eq!(iq.len(), real.len() * 2);
        let i_ch: Vec<f32> = iq.iter().step_by(2).copied().collect();
        assert_eq!(i_ch, real, "I channel must match scfdma_modulate output");
        let q_ch: Vec<f32> = iq.iter().skip(1).step_by(2).copied().collect();
        assert!(q_ch.iter().all(|&q| q == 0.0), "Q channel must be zero");
    }

    #[test]
    fn scfdma52_iq_round_trip() {
        let payload = b"round trip IQ scfdma52";
        let iq = scfdma_modulate_iq(payload, "SCFDMA52");
        let i_ch: Vec<f32> = iq.iter().step_by(2).copied().collect();
        let decoded = scfdma_demodulate(&i_ch, "SCFDMA52").expect("demodulate");
        assert_eq!(decoded, payload);
    }
}
