//! Non-coherent 8-FSK soft demodulation: per-symbol tone energies → 174 bit LLRs.
//!
//! Given a slot's audio already aligned to symbol 0 at a known base tone frequency (fine sync is a
//! later unit), this measures the eight tone energies of each data symbol (Goertzel), then converts
//! them to max-log bit LLRs over the **direct-binary** tone map (bit `i` set ⇒ that weight is in the
//! tone index; see `tones.rs`). LLR sign matches the decoder: `> 0` ⇒ bit 1.

use crate::ldpc174::N;
use crate::submode::{SubmodeParams, NUM_TONES};
use crate::tones::data_positions;

/// Goertzel energy of `win` at `freq` (Hz).
pub fn goertzel_energy(win: &[f32], freq: f32, fs: f32) -> f32 {
    let w = std::f32::consts::TAU * freq / fs;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for &v in win {
        let s0 = v + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0)
}

/// The eight tone energies of one symbol window (`win` = one symbol's samples).
pub fn symbol_tone_energies(win: &[f32], base_freq_hz: f32, spacing_hz: f32, fs: f32) -> [f32; 8] {
    let mut e = [0f32; NUM_TONES];
    for (t, slot) in e.iter_mut().enumerate() {
        *slot = goertzel_energy(win, base_freq_hz + t as f32 * spacing_hz, fs);
    }
    e
}

/// Max-log bit LLRs `[b0, b1, b2]` (MSB first) from one symbol's tone energies, scaled by `1/noise`.
/// For each bit, `LLR = (max energy over tones with that bit = 1) − (… = 0)) / noise`.
pub fn tone_energies_to_llrs(energies: &[f32; 8], noise: f32) -> [f32; 3] {
    let inv = 1.0 / noise.max(1e-9);
    let mut llr = [0f32; 3];
    for (bit, slot) in llr.iter_mut().enumerate() {
        let weight = 1 << (2 - bit); // bit 0 → 4, bit 1 → 2, bit 2 → 1
        let mut e1 = f32::NEG_INFINITY;
        let mut e0 = f32::NEG_INFINITY;
        for (t, &e) in energies.iter().enumerate() {
            if t & weight != 0 {
                e1 = e1.max(e);
            } else {
                e0 = e0.max(e);
            }
        }
        *slot = (e1 - e0) * inv;
    }
    llr
}

/// Soft-demodulate a slot's audio (aligned to symbol 0, lowest tone at `base_freq_hz`) into 174 bit
/// LLRs in codeword order. The noise scale is estimated from each symbol's non-peak tone energies
/// (the seven tones that are not the winner are mostly noise).
pub fn demodulate_soft(audio: &[f32], base_freq_hz: f32, params: &SubmodeParams) -> [f32; N] {
    let sps = params.samples_per_symbol;
    let fs = crate::submode::SAMPLE_RATE as f32;
    let positions = data_positions();

    // Per-symbol energies + a global noise estimate = mean of all non-max tone energies.
    let mut sym_energies = [[0f32; 8]; 58];
    let mut noise_acc = 0.0f64;
    let mut noise_n = 0u32;
    for (j, &pos) in positions.iter().enumerate() {
        let start = pos * sps;
        let win = audio.get(start..start + sps).unwrap_or(&[]);
        let e = symbol_tone_energies(win, base_freq_hz, params.tone_spacing_hz, fs);
        sym_energies[j] = e;
        let max = e.iter().cloned().fold(0.0f32, f32::max);
        for &v in &e {
            if v < max {
                noise_acc += v as f64;
                noise_n += 1;
            }
        }
    }
    let noise = if noise_n > 0 {
        (noise_acc / noise_n as f64) as f32
    } else {
        1.0
    }
    .max(1e-9);

    let mut llr = [0f32; N];
    for (j, e) in sym_energies.iter().enumerate() {
        let bits = tone_energies_to_llrs(e, noise);
        llr[3 * j..3 * j + 3].copy_from_slice(&bits);
    }
    llr
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::costas::CostasKind;
    use crate::ldpc174::{bp_decode, encode174, K};
    use crate::message::js8_info_bits;
    use crate::modulate::{modulate_tones, GfskParams};
    use crate::submode::{params, Submode};
    use crate::tones::message_to_tones;

    fn msg(seed: u64) -> [u8; K] {
        let p = payload9(seed);
        js8_info_bits(&p, (seed % 8) as u8)
    }
    fn payload9(seed: u64) -> [u8; 9] {
        let mut s = seed;
        let mut p = [0u8; 9];
        for b in p.iter_mut() {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
            *b = (s >> 40) as u8;
        }
        p
    }

    #[test]
    fn tone_llrs_have_the_right_sign() {
        // Energy concentrated on tone 5 (0b101) → b0=1, b1=0, b2=1.
        let mut e = [1.0f32; 8];
        e[5] = 100.0;
        let l = tone_energies_to_llrs(&e, 1.0);
        assert!(l[0] > 0.0, "bit0 (weight4) set");
        assert!(l[1] < 0.0, "bit1 (weight2) clear");
        assert!(l[2] > 0.0, "bit2 (weight1) set");
    }

    #[test]
    fn clean_round_trip_encode_modulate_demod_decode() {
        // The first true RX round-trip: message → tones → GFSK audio → soft demod → BP decode → message.
        let sm = params(Submode::Normal);
        let base = 1500.0;
        for seed in [1u64, 7, 99, 4242] {
            let m = msg(seed);
            let tones = message_to_tones(&m, CostasKind::Original);
            let audio = modulate_tones(&tones, base, &GfskParams::from_submode(&sm));
            let llr = demodulate_soft(&audio, base, &sm);
            let d = bp_decode(&llr, 50).expect("decode clean round-trip");
            assert_eq!(d.info, m, "seed {seed}");
        }
    }

    #[test]
    fn round_trip_survives_moderate_noise() {
        let sm = params(Submode::Normal);
        let base = 1200.0;
        let m = msg(31337);
        let tones = message_to_tones(&m, CostasKind::Original);
        let mut audio = modulate_tones(&tones, base, &GfskParams::from_submode(&sm));
        // Add white noise (LCG-driven) at a moderate level.
        let mut s = 0xabcdef_u64;
        for v in audio.iter_mut() {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
            let n = ((s >> 40) as f32 / (1u32 << 24) as f32) - 0.5;
            *v += 0.35 * n;
        }
        let llr = demodulate_soft(&audio, base, &sm);
        let d = bp_decode(&llr, 80).expect("decode noisy round-trip");
        assert_eq!(d.info, m);
    }

    #[test]
    fn energies_peak_on_the_transmitted_tone() {
        let sm = params(Submode::Normal);
        let base = 1500.0;
        let cw = encode174(&msg(5));
        let tones = message_to_tones(&msg(5), CostasKind::Original);
        let _ = cw;
        let audio = modulate_tones(&tones, base, &GfskParams::from_submode(&sm));
        let sps = sm.samples_per_symbol;
        let positions = data_positions();
        for (j, &pos) in positions.iter().enumerate() {
            let win = &audio[pos * sps..(pos + 1) * sps];
            let e = symbol_tone_energies(win, base, sm.tone_spacing_hz, 8000.0);
            let argmax = (0..8).max_by(|&a, &b| e[a].total_cmp(&e[b])).unwrap();
            assert_eq!(argmax as u8, tones[pos], "data symbol {j}");
        }
    }
}
