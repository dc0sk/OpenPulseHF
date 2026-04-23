//! BPSK modulator.
//!
//! The modulation pipeline is:
//!
//! ```text
//! bytes → bits (LSB-first) → NRZI encode → symbols (+1/−1)
//!       → raised-cosine pulse shaping → carrier mix → audio samples
//! ```

use std::f32::consts::PI;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::ModulationConfig;

use crate::parse_baud_rate;

/// Number of preamble symbols prepended to every transmission.
pub const PREAMBLE_SYMS: usize = 32;
/// Number of tail symbols appended after data to let the signal decay.
pub const TAIL_SYMS: usize = 8;

// ── Public entry point ────────────────────────────────────────────────────────

/// Modulate `data` bytes to a vector of normalised PCM samples.
pub fn bpsk_modulate(data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
    let baud = parse_baud_rate(&config.mode)?;
    let fs = config.sample_rate as f32;
    let fc = config.center_frequency;
    let n = samples_per_symbol(fs, baud)?;

    // Build the bit stream: preamble (all 1s → alternating phases) + data + tail
    let mut bits: Vec<bool> = Vec::new();
    // Preamble: alternating 1/0 bits so NRZI gives +1,-1,+1,−1 …
    for i in 0..PREAMBLE_SYMS {
        bits.push(i % 2 == 0); // 1,0,1,0,...
    }
    bits.extend(bytes_to_bits(data));
    // Tail: all zeros (no phase change) so signal fades smoothly
    bits.extend(std::iter::repeat_n(false, TAIL_SYMS));

    // NRZI encode
    let symbols = nrzi_encode(&bits);

    // Render samples
    let total = symbols.len() * n;
    let mut out = vec![0.0f32; total];
    let two_pi = 2.0 * PI;

    for (sym_idx, &phase_neg) in symbols.iter().enumerate() {
        let amplitude = if phase_neg { -1.0f32 } else { 1.0f32 };
        let sym_start = sym_idx * n;

        for i in 0..n {
            // Raised-cosine (Hann) amplitude envelope – smoothly ramps 0→1→0
            // across the symbol period, eliminating abrupt phase-change clicks.
            let envelope = 0.5 * (1.0 - (two_pi * i as f32 / n as f32).cos());

            // The global sample index determines the carrier phase.
            let t = (sym_start + i) as f32 / fs;
            let carrier = (two_pi * fc * t).cos();

            out[sym_start + i] = amplitude * envelope * carrier;
        }
    }

    Ok(out)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert bytes to LSB-first bits.
pub(crate) fn bytes_to_bits(bytes: &[u8]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for shift in 0..8u8 {
            bits.push((b >> shift) & 1 == 1);
        }
    }
    bits
}

/// NRZI encoding: bit `true` ("1") → flip phase; `false` ("0") → keep phase.
/// Returns `true` for negative phase (180°), `false` for positive (0°).
pub(crate) fn nrzi_encode(bits: &[bool]) -> Vec<bool> {
    let mut phase_neg = false;
    bits.iter()
        .map(|&flip| {
            if flip {
                phase_neg = !phase_neg;
            }
            phase_neg
        })
        .collect()
}

/// Compute integer samples-per-symbol, returning an error when the ratio
/// would be less than 4 (DSP cannot work reliably below that).
pub(crate) fn samples_per_symbol(sample_rate: f32, baud: f32) -> Result<usize, ModemError> {
    let n = (sample_rate / baud).round() as usize;
    if n < 4 {
        return Err(ModemError::Configuration(format!(
            "sample rate {sample_rate} Hz is too low for {baud} baud \
             (need at least 4 samples/symbol)"
        )));
    }
    Ok(n)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_core::plugin::ModulationConfig;

    #[test]
    fn bytes_to_bits_lsb_first() {
        let bits = bytes_to_bits(&[0b10110001]);
        assert_eq!(
            bits,
            vec![true, false, false, false, true, true, false, true]
        );
    }

    #[test]
    fn nrzi_flip_on_one() {
        // bits: 1,0,1,1 → phases: flip, same, flip, flip
        let phases = nrzi_encode(&[true, false, true, true]);
        assert_eq!(phases, vec![true, true, false, true]);
    }

    #[test]
    fn modulate_produces_correct_length() {
        let cfg = ModulationConfig {
            mode: "BPSK100".to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
        };
        let data = b"Hi";
        let samples = bpsk_modulate(data, &cfg).unwrap();
        let n = samples_per_symbol(8000.0, 100.0).unwrap(); // 80
        let expected_syms = PREAMBLE_SYMS + data.len() * 8 + TAIL_SYMS;
        assert_eq!(samples.len(), expected_syms * n);
    }

    #[test]
    fn samples_within_range() {
        let cfg = ModulationConfig::default();
        let samples = bpsk_modulate(b"test", &cfg).unwrap();
        for &s in &samples {
            assert!(s >= -1.0 && s <= 1.0, "sample {s} out of range");
        }
    }
}
