//! Costas sync acquisition: find a slot's start offset and base tone frequency.
//!
//! A JS8 slot carries three 7-symbol Costas sync blocks at symbol positions 0–6, 36–42, 72–78. This
//! searches a (time-offset × base-frequency) grid, scoring each candidate by how much of each sync
//! symbol's energy sits on its expected Costas tone — **normalized** by the symbol's total energy so
//! a high-energy noise window can't win the argmax (DSP playbook: "acquire on the normalized
//! correlation"). A perfect match scores 21 (one per sync symbol).

use crate::demodulate::symbol_tone_energies;
use crate::submode::{SubmodeParams, COSTAS_BLOCK_STARTS, COSTAS_LEN, SAMPLE_RATE};

/// A sync candidate: where a slot starts and at what base tone frequency, with its correlation score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncCandidate {
    /// Sample offset of symbol 0.
    pub sample_offset: usize,
    /// Lowest tone frequency in Hz.
    pub base_freq_hz: f32,
    /// Normalized Costas correlation, 0..=21.
    pub score: f32,
}

/// Normalized Costas correlation of `audio` for a candidate `(offset, base_freq)`. `None` if the slot
/// would run past the buffer.
pub(crate) fn sync_score(
    audio: &[f32],
    offset: usize,
    base_freq: f32,
    params: &SubmodeParams,
) -> Option<f32> {
    let sps = params.samples_per_symbol;
    let fs = SAMPLE_RATE as f32;
    let mut score = 0.0f32;
    for (b, &start) in COSTAS_BLOCK_STARTS.iter().enumerate() {
        let tones = params.costas.block(b);
        for (i, &tone) in tones.iter().enumerate() {
            let pos = offset + (start + i) * sps;
            let win = audio.get(pos..pos + sps)?;
            let e = symbol_tone_energies(win, base_freq, params.tone_spacing_hz, fs);
            let total: f32 = e.iter().sum::<f32>() + 1e-9;
            score += e[tone as usize] / total;
        }
    }
    Some(score)
}

/// Search a `(time × frequency)` grid for the best Costas sync. `offset` runs `0..=max_offset` in
/// `offset_step` samples; `base_freq` runs `[base_min, base_max]` in `base_step` Hz. Returns the
/// highest-scoring candidate whose full slot fits in `audio`, or `None` if none fits.
#[allow(clippy::too_many_arguments)]
pub fn find_sync(
    audio: &[f32],
    params: &SubmodeParams,
    max_offset: usize,
    offset_step: usize,
    base_min: f32,
    base_max: f32,
    base_step: f32,
) -> Option<SyncCandidate> {
    let step = offset_step.max(1);
    let fstep = base_step.max(0.1);
    let mut best: Option<SyncCandidate> = None;
    let mut offset = 0;
    while offset <= max_offset {
        let mut f = base_min;
        while f <= base_max {
            if let Some(score) = sync_score(audio, offset, f, params) {
                if best.is_none_or(|b| score > b.score) {
                    best = Some(SyncCandidate {
                        sample_offset: offset,
                        base_freq_hz: f,
                        score,
                    });
                }
            }
            f += fstep;
        }
        offset += step;
    }
    best
}

/// The whole preamble span (one Costas block) in samples — the acquisition front the demod needs.
pub fn preamble_samples(params: &SubmodeParams) -> usize {
    COSTAS_LEN * params.samples_per_symbol
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::costas::CostasKind;
    use crate::demodulate::demodulate_soft;
    use crate::ldpc174::{bp_decode, K};
    use crate::message::js8_info_bits;
    use crate::modulate::{modulate_tones, GfskParams};
    use crate::submode::{params, Submode};
    use crate::tones::message_to_tones;

    fn slot_audio(base: f32, lead: usize, seed: u64) -> (Vec<f32>, [u8; K]) {
        let sm = params(Submode::Normal);
        let mut p = [0u8; 9];
        let mut s = seed;
        for b in p.iter_mut() {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
            *b = (s >> 40) as u8;
        }
        let info = js8_info_bits(&p, (seed % 8) as u8);
        let tones = message_to_tones(&info, CostasKind::Original);
        let sig = modulate_tones(&tones, base, &GfskParams::from_submode(&sm));
        let mut audio = vec![0f32; lead];
        audio.extend_from_slice(&sig);
        audio.extend(std::iter::repeat_n(0.0, sm.samples_per_symbol)); // trailing pad
        (audio, info)
    }

    #[test]
    fn finds_the_slot_offset_and_base_frequency() {
        let sm = params(Submode::Normal);
        let sps = sm.samples_per_symbol;
        let base = 1500.0;
        let lead = 3 * sps; // on the whole-symbol offset grid
        let (audio, _info) = slot_audio(base, lead, 77);

        let cand = find_sync(&audio, &sm, 6 * sps, sps, 1490.0, 1510.0, 2.0).expect("sync");
        assert_eq!(cand.sample_offset, lead, "found the slot start");
        assert!(
            (cand.base_freq_hz - base).abs() <= 2.0,
            "found base ~1500: {}",
            cand.base_freq_hz
        );
        assert!(
            cand.score > 18.0,
            "strong Costas correlation: {}",
            cand.score
        );
    }

    #[test]
    fn sync_then_demod_then_decode() {
        // Acquire on the Costas blocks, then hand the aligned slot to demod + BP decode.
        let sm = params(Submode::Normal);
        let sps = sm.samples_per_symbol;
        let base = 1200.0;
        let lead = 2 * sps;
        let (audio, info) = slot_audio(base, lead, 909);

        let cand = find_sync(&audio, &sm, 5 * sps, sps, 1190.0, 1210.0, 2.0).expect("sync");
        let llr = demodulate_soft(&audio[cand.sample_offset..], cand.base_freq_hz, &sm);
        let d = bp_decode(&llr, 60).expect("decode after sync");
        assert_eq!(d.info, info);
    }

    #[test]
    fn a_pure_noise_offset_scores_lower_than_the_true_one() {
        let sm = params(Submode::Normal);
        let sps = sm.samples_per_symbol;
        let (audio, _) = slot_audio(1500.0, 3 * sps, 5);
        let at_signal = sync_score(&audio, 3 * sps, 1500.0, &sm).unwrap();
        let at_noise = sync_score(&audio, 0, 1500.0, &sm).unwrap(); // leading silence
        assert!(
            at_signal > at_noise + 5.0,
            "signal {at_signal} vs noise {at_noise}"
        );
    }
}
