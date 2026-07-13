//! Window decoder: many decodes per 15 s slot (JS8Call decodes dozens of overlapping stations).
//!
//! `decode_window` runs the Costas sync search across the passband, decodes each strong candidate
//! (soft demod → BP), keeps only those whose **CRC-12 checks** (the guard against false decodes), and
//! dedups by content. This is the single entry the discovery service drives per received slot.

use crate::demodulate::{demodulate_soft, goertzel_energy, symbol_tone_energies};
use crate::ldpc174::{bp_decode, encode174, K};
use crate::message::check_info_crc;
use crate::submode::{SubmodeParams, SAMPLE_RATE};
use crate::sync::sync_score;
use crate::tones::data_positions;

/// Floor for the SNR estimate (dB, 2500 Hz ref); returned when the noise measurement degenerates.
const SNR_FLOOR_DB: f32 = -30.0;
/// Calibration offset (dB) folding the Goertzel bin's equivalent-noise-bandwidth and the GFSK
/// pulse's out-of-bin energy spreading into the estimate. Fitted against the B-6 calibrated-AWGN
/// harness so the estimate tracks the injected 2500 Hz-referenced SNR; see `snr_estimate.rs`.
const SNR_CAL_OFFSET_DB: f32 = 0.5;
/// Guard tone-index offsets (relative to the base tone) for the noise-floor measurement — out of the
/// 0..=7 signal band and ≥2 bins from either edge so the GFSK pulse tails don't contaminate them.
const GUARD_TONE_OFFSETS: [i32; 4] = [-3, -2, 9, 10];

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
    /// Estimated SNR (dB) in the 2500 Hz reference bandwidth (WSJT-X/JS8 convention).
    pub snr_db: f32,
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
    /// Coarse frequency step (Hz) for two-stage acquisition. When `> base_step`, `decode_window` scans a
    /// coarse time×freq grid and then refines each candidate to `base_step`; `0` = single-pass at
    /// `base_step` (the default — byte-identical to the pre-two-stage behaviour).
    pub base_step_coarse: f32,
    /// Smallest slot-start offset to search (samples). Lets a caller skip a known-silent slot lead-in.
    pub min_offset: usize,
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
            base_step_coarse: 0.0,
            min_offset: 0,
            max_offset: 0,
            offset_step: 1,
            min_sync_score: 12.0,
            max_candidates: 32,
            bp_iterations: 50,
        }
    }
}

/// Estimate a decoded frame's SNR (dB, 2500 Hz ref BW). Matched to the transmitted data tones,
/// re-encoded from the decoded info bits: per data symbol, the Goertzel energy at the sent tone is
/// signal+noise and the mean of the other seven is the per-bin noise. The aggregate noise-corrected
/// signal-to-noise ratio is measured in the Goertzel bin bandwidth (`fs / samples_per_symbol`) and
/// scaled up to the 2500 Hz reference; `SNR_CAL_OFFSET_DB` absorbs the bin ENBW + pulse spreading.
fn estimate_snr_db(
    audio: &[f32],
    base_freq_hz: f32,
    off: usize,
    params: &SubmodeParams,
    info: &[u8; K],
) -> f32 {
    let sps = params.samples_per_symbol;
    let fs = SAMPLE_RATE as f32;
    let cw = encode174(info);
    let mut sum_sig = 0.0f64;
    let mut sum_noise = 0.0f64;
    for (j, &pos) in data_positions().iter().enumerate() {
        let start = off + pos * sps;
        let Some(win) = audio.get(start..start + sps) else {
            continue;
        };
        let e = symbol_tone_energies(win, base_freq_hz, params.tone_spacing_hz, fs);
        let tone = ((cw[3 * j] << 2) | (cw[3 * j + 1] << 1) | cw[3 * j + 2]) as usize;
        let e_sig = e[tone];
        // Noise floor from fixed out-of-band guard bins (tone indices below 0 / above 7). Only one
        // tone is active per symbol, and the wide GFSK pulse leaks into its in-band neighbours, so
        // in-band "noise" grows with signal power and the estimate saturates; guard bins ≥2 away from
        // the band edges stay decoupled from the signal.
        let e_noise = GUARD_TONE_OFFSETS
            .iter()
            .map(|&g| goertzel_energy(win, base_freq_hz + g as f32 * params.tone_spacing_hz, fs))
            .sum::<f32>()
            / GUARD_TONE_OFFSETS.len() as f32;
        sum_sig += (e_sig - e_noise).max(0.0) as f64;
        sum_noise += e_noise as f64;
    }
    if sum_noise <= 0.0 {
        return SNR_FLOOR_DB;
    }
    let bin_bw = fs / sps as f32;
    let snr_bin_db = 10.0 * (sum_sig / sum_noise).max(1e-9).log10() as f32;
    (snr_bin_db + 10.0 * (bin_bw / 2500.0).log10() + SNR_CAL_OFFSET_DB).max(SNR_FLOOR_DB)
}

/// Refine a coarse-grid `(offset, freq)` sync peak to full precision: search ±`ostep` in time (at
/// `sps/8`) and ±`coarse` in frequency (at `cfg.base_step`), returning the best `(score, offset, freq)`.
fn refine_sync(
    audio: &[f32],
    off0: usize,
    f0: f32,
    ostep: usize,
    coarse: f32,
    cfg: &DecodeCfg,
    params: &SubmodeParams,
) -> (f32, usize, f32) {
    let ostep_fine = (params.samples_per_symbol / 8).max(1);
    let lo = off0.saturating_sub(ostep).max(cfg.min_offset);
    let hi = (off0 + ostep).min(cfg.max_offset);
    let flo = (f0 - coarse).max(cfg.base_min);
    let fhi = (f0 + coarse).min(cfg.base_max);
    let mut best = (f32::MIN, off0, f0);
    let mut off = lo;
    while off <= hi {
        let mut f = flo;
        while f <= fhi {
            if let Some(s) = sync_score(audio, off, f, params) {
                if s > best.0 {
                    best = (s, off, f);
                }
            }
            f += cfg.base_step.max(0.1);
        }
        off += ostep_fine;
    }
    best
}

/// Decode every JS8 frame found in `audio` (one 15 s slot's worth) under `cfg`. Returns CRC-verified,
/// content-deduped decodes, strongest sync first.
pub fn decode_window(audio: &[f32], params: &SubmodeParams, cfg: &DecodeCfg) -> Vec<Js8Decode> {
    let sps = params.samples_per_symbol;
    let ostep = cfg.offset_step.max(1);
    let fine = cfg.base_step.max(0.1);
    // Two-stage acquisition: a coarse freq grid (cheap over a wide time search) then a per-candidate
    // refine to `fine`. Keeps a time search (needed for real off-air overs, which start ~0.5 s into the
    // slot) affordable — an exhaustive fine grid over the timing slack is ~40× the single-offset cost.
    let coarse = if cfg.base_step_coarse > fine {
        cfg.base_step_coarse
    } else {
        fine
    };
    let two_stage = coarse > fine;

    // Gather all above-threshold sync candidates on the coarse grid.
    let mut cands: Vec<(f32, usize, f32)> = Vec::new();
    let mut offset = cfg.min_offset;
    while offset <= cfg.max_offset {
        let mut f = cfg.base_min;
        while f <= cfg.base_max {
            if let Some(score) = sync_score(audio, offset, f, params) {
                if score >= cfg.min_sync_score {
                    cands.push((score, offset, f));
                }
            }
            f += coarse;
        }
        offset += ostep;
    }
    cands.sort_by(|a, b| b.0.total_cmp(&a.0));

    // Greedily keep well-separated peaks (≥ half a symbol in time; ≥ half a tone, or a coarse cell,
    // in frequency).
    let half_tone = params.tone_spacing_hz / 2.0;
    let freq_sep = if two_stage {
        coarse.max(half_tone)
    } else {
        half_tone
    };
    let mut picked: Vec<(f32, usize, f32)> = Vec::new();
    for c in cands {
        let clash = picked.iter().any(|p| {
            (c.1 as isize - p.1 as isize).unsigned_abs() < sps / 2 && (c.2 - p.2).abs() < freq_sep
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
        // Refine the coarse (offset, freq) to full precision before demodulating.
        let (score, off, freq) = if two_stage {
            refine_sync(audio, off, freq, ostep, coarse, cfg, params)
        } else {
            (score, off, freq)
        };
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
        let snr_db = estimate_snr_db(audio, freq, off, params, &d.info);
        out.push(Js8Decode {
            payload,
            i3bit,
            base_freq_hz: freq,
            sample_offset: off,
            sync_score: score,
            snr_db,
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
            base_step_coarse: 0.0,
            min_offset: 0,
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
