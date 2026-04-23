//! BPSK demodulator.
//!
//! The demodulation pipeline is:
//!
//! ```text
//! audio samples
//!   → multiply by I/Q reference carriers
//!   → matched-filter (Hann window) integration per symbol period
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
use openpulse_core::plugin::ModulationConfig;

use crate::modulate::{nrzi_encode, samples_per_symbol, PREAMBLE_SYMS, TAIL_SYMS};
use crate::parse_baud_rate;

// ── Public entry point ────────────────────────────────────────────────────────

/// Demodulate audio `samples` and return the recovered bytes.
pub fn bpsk_demodulate(samples: &[f32], config: &ModulationConfig) -> Result<Vec<u8>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    if samples.len() < n * (PREAMBLE_SYMS + 1) {
        return Err(ModemError::Demodulation("signal too short".into()));
    }

    // Find the best timing offset by maximising preamble energy.
    let offset = find_timing_offset(samples, n, fc, fs);

    // Demodulate all symbols from the aligned position.
    let (i_syms, q_syms) = demodulate_iq(samples, n, fc, fs, offset);

    if i_syms.len() <= PREAMBLE_SYMS + TAIL_SYMS {
        return Err(ModemError::Demodulation("no data symbols after preamble".into()));
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

// ── Timing search ─────────────────────────────────────────────────────────────

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
/// (Hann window) and integrate over each symbol period.
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

            // Matched filter: same Hann window used by the modulator.
            let window = 0.5 * (1.0 - (two_pi * i as f32 / n as f32).cos());

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
        let iq: Vec<(f32, f32)> = (0..9).map(|i| (if i % 2 == 0 { 1.0 } else { -1.0 }, 0.0)).collect();
        let bits = differential_decode(&iq);
        assert!(bits.iter().all(|&b| b));
    }

    #[test]
    fn loopback_round_trip() {
        use crate::modulate::bpsk_modulate;
        let cfg = ModulationConfig {
            mode: "BPSK100".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
        };
        let original = b"AB";
        let samples = bpsk_modulate(original, &cfg).unwrap();
        let recovered = bpsk_demodulate(&samples, &cfg).unwrap();
        assert_eq!(&recovered[..original.len()], original);
    }
}
