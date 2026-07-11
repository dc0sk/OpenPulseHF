//! Window decoder: many decodes per 15 s slot (JS8Call decodes dozens of overlapping stations).
//!
//! `decode_window` runs the Costas sync search across the passband, decodes each strong candidate
//! (soft demod → BP), keeps only those whose **CRC-12 checks** (the guard against false decodes), and
//! dedups by content. This is the single entry the discovery service drives per received slot.

use crate::demodulate::demodulate_soft;
use crate::ldpc174::bp_decode;
use crate::message::check_info_crc;
use crate::submode::SubmodeParams;
use crate::sync::sync_score;

/// One decoded JS8 frame from a window.
#[derive(Debug, Clone)]
pub struct Js8Decode {
    /// The 72-bit message payload (9 bytes, MSB-first).
    pub payload: [u8; 9],
    /// The 3-bit transmission flags.
    pub i3bit: u8,
    /// Recovered base tone frequency (Hz).
    pub base_freq_hz: f32,
    /// Sample offset of the slot start within the window.
    pub sample_offset: usize,
    /// Costas sync score (0..=21) of the acquisition.
    pub sync_score: f32,
}

/// Search parameters for [`decode_window`].
#[derive(Debug, Clone, Copy)]
pub struct DecodeCfg {
    /// Lowest base tone frequency to search (Hz).
    pub base_min: f32,
    /// Highest base tone frequency to search (Hz).
    pub base_max: f32,
    /// Frequency search step (Hz).
    pub base_step: f32,
    /// Largest slot-start offset to search (samples).
    pub max_offset: usize,
    /// Time search step (samples).
    pub offset_step: usize,
    /// Minimum Costas score to attempt a decode.
    pub min_sync_score: f32,
    /// Cap on decode attempts (strongest candidates first).
    pub max_candidates: usize,
    /// BP decoder iteration cap.
    pub bp_iterations: u32,
}

impl Default for DecodeCfg {
    fn default() -> Self {
        Self {
            base_min: 300.0,
            base_max: 2500.0,
            base_step: 3.125,
            max_offset: 0,
            offset_step: 1,
            min_sync_score: 12.0,
            max_candidates: 32,
            bp_iterations: 50,
        }
    }
}

/// Decode every JS8 frame found in `audio` (one 15 s slot's worth) under `cfg`. Returns CRC-verified,
/// content-deduped decodes, strongest sync first.
pub fn decode_window(audio: &[f32], params: &SubmodeParams, cfg: &DecodeCfg) -> Vec<Js8Decode> {
    let sps = params.samples_per_symbol;
    let ostep = cfg.offset_step.max(1);
    let fstep = cfg.base_step.max(0.1);

    // Gather all above-threshold sync candidates.
    let mut cands: Vec<(f32, usize, f32)> = Vec::new();
    let mut offset = 0;
    while offset <= cfg.max_offset {
        let mut f = cfg.base_min;
        while f <= cfg.base_max {
            if let Some(score) = sync_score(audio, offset, f, params) {
                if score >= cfg.min_sync_score {
                    cands.push((score, offset, f));
                }
            }
            f += fstep;
        }
        offset += ostep;
    }
    cands.sort_by(|a, b| b.0.total_cmp(&a.0));

    // Greedily keep well-separated peaks (≥ half a symbol in time, ≥ half a tone in frequency).
    let half_tone = params.tone_spacing_hz / 2.0;
    let mut picked: Vec<(f32, usize, f32)> = Vec::new();
    for c in cands {
        let clash = picked.iter().any(|p| {
            (c.1 as isize - p.1 as isize).unsigned_abs() < sps / 2 && (c.2 - p.2).abs() < half_tone
        });
        if !clash {
            picked.push(c);
            if picked.len() >= cfg.max_candidates {
                break;
            }
        }
    }

    // Decode each candidate; keep CRC-valid, content-deduped frames.
    let mut out: Vec<Js8Decode> = Vec::new();
    for (score, off, freq) in picked {
        let llr = demodulate_soft(&audio[off..], freq, params);
        let Some(d) = bp_decode(&llr, cfg.bp_iterations) else {
            continue;
        };
        let Some((payload, i3bit)) = check_info_crc(&d.info) else {
            continue;
        };
        if out.iter().any(|e| e.payload == payload && e.i3bit == i3bit) {
            continue;
        }
        out.push(Js8Decode {
            payload,
            i3bit,
            base_freq_hz: freq,
            sample_offset: off,
            sync_score: score,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::costas::CostasKind;
    use crate::message::js8_info_bits;
    use crate::modulate::{modulate_tones, GfskParams};
    use crate::submode::{params, Submode};
    use crate::tones::message_to_tones;

    fn payload9(seed: u64) -> [u8; 9] {
        let mut s = seed;
        let mut p = [0u8; 9];
        for b in p.iter_mut() {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
            *b = (s >> 40) as u8;
        }
        p
    }

    fn signal(base: f32, seed: u64) -> Vec<f32> {
        let sm = params(Submode::Normal);
        let info = js8_info_bits(&payload9(seed), (seed % 8) as u8);
        let tones = message_to_tones(&info, CostasKind::Original);
        modulate_tones(&tones, base, &GfskParams::from_submode(&sm))
    }

    fn cfg(base_min: f32, base_max: f32, max_offset: usize, step: usize) -> DecodeCfg {
        DecodeCfg {
            base_min,
            base_max,
            base_step: 3.125,
            max_offset,
            offset_step: step,
            min_sync_score: 12.0,
            max_candidates: 16,
            bp_iterations: 60,
        }
    }

    #[test]
    fn decodes_a_single_frame() {
        let sm = params(Submode::Normal);
        let sps = sm.samples_per_symbol;
        let mut audio = vec![0f32; 2 * sps];
        audio.extend_from_slice(&signal(1500.0, 42));
        audio.extend(std::iter::repeat_n(0.0, sps));

        let d = decode_window(&audio, &sm, &cfg(1480.0, 1520.0, 4 * sps, sps));
        assert_eq!(d.len(), 1, "exactly one CRC-valid decode");
        assert_eq!(d[0].payload, payload9(42));
        assert_eq!(d[0].sample_offset, 2 * sps);
    }

    #[test]
    fn decodes_two_stations_at_different_frequencies() {
        // Two overlapping frames at distinct base tones in one window.
        let sm = params(Submode::Normal);
        let a = signal(1000.0, 11);
        let b = signal(1800.0, 22);
        let mut audio = vec![0f32; a.len().max(b.len())];
        for (i, v) in a.iter().enumerate() {
            audio[i] += v;
        }
        for (i, v) in b.iter().enumerate() {
            audio[i] += v;
        }

        let decodes = decode_window(&audio, &sm, &cfg(900.0, 1900.0, 0, 1));
        let payloads: Vec<_> = decodes.iter().map(|d| d.payload).collect();
        assert!(payloads.contains(&payload9(11)), "station A decoded");
        assert!(payloads.contains(&payload9(22)), "station B decoded");
    }

    #[test]
    fn pure_noise_yields_no_decodes() {
        let sm = params(Submode::Normal);
        let mut s = 0xdead_u64;
        let mut audio = vec![0f32; sm.samples_per_period() + sm.samples_per_symbol];
        for v in audio.iter_mut() {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
            *v = ((s >> 40) as f32 / (1u32 << 24) as f32) - 0.5;
        }
        // The CRC-12 gate makes a false decode astronomically unlikely.
        assert!(decode_window(&audio, &sm, &cfg(300.0, 2500.0, 0, 1)).is_empty());
    }
}
