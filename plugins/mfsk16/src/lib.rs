//! Constant-envelope non-coherent **16-GFSK** weak-signal sub-floor waveform (REQ-WSIG-01).
//!
//! A robust narrowband rung *below* BPSK31 for deep-fade HF. Measured (ideal + real-sync) to beat coherent
//! BPSK31 by ~4 dB on moderate multipath and to decode where BPSK31 fails entirely on fast fade, at a PAPR
//! **credit** — see `docs/dev/research/robust-narrowband-measurement.md`. It works precisely because it is
//! non-coherent (no carrier phase to track through fades) and constant-envelope.
//!
//! # Waveform (`MFSK16`)
//! - 16 tones, **31.25 Hz** spacing, **31.25 baud** (256 samples/symbol at 8 kHz), 4 bits/symbol.
//! - **500 Hz** occupied; the 16-tone comb is centered on `center_frequency`.
//! - GFSK tone synthesis (constant envelope) reused from the JS8 primitives.
//! - Fixed **one-RS-block** frame: 255 wire bytes → 510 data tones + 3×7 Costas sync blocks = 531 symbols
//!   (≈ 17.0 s). Intended for `FecMode::Rs` (the measurement's decode), soft-capable for HARQ combining.
//!
//! # Acquisition
//! Three Costas sync blocks + a normalized per-symbol tone-fraction correlation searched over
//! timing × frequency, so the plugin self-acquires (a ±25 Hz tuning offset was validated). `estimate_afc_hz`
//! is therefore `None` — the non-coherent plugin opts out of the engine's coherent AFC chain entirely.

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{FrameGeometry, ModulationConfig, ModulationPlugin, PluginInfo};

use js8_plugin::demodulate::goertzel_energy;
use js8_plugin::modulate::{modulate_tones, GfskParams, DEFAULT_BT};

const SPS: usize = 256; // 8000 / 31.25 = 256 exact (at 8 kHz)
const SPACING: f32 = 31.25;
const N_TONES: usize = 16;
const BITS_PER_SYM: usize = 4;

/// FT8 legacy Costas `[4,2,5,6,1,3,0]` ×2 (distinct-difference preserved), spanning tones 0..12 of 16.
const COSTAS16: [u8; 7] = [8, 4, 10, 12, 2, 6, 0];

/// Per-mode frame geometry. `MFSK16` is one 255-byte RS data block; `MFSK16-ACK` is the short
/// non-coherent return channel (13-byte ShortFec-encoded ACK, 1.28 s) that survives at the data floor.
#[derive(Clone, Copy)]
struct Layout {
    frame_bytes: usize,
    sync_starts: &'static [usize],
    onair_tones: usize,
}

/// One RS block: 255 wire bytes → 510 data tones + 3×7 Costas = 531 symbols (≈ 17.0 s).
const DATA_LAYOUT: Layout = Layout {
    frame_bytes: 255,
    sync_starts: &[0, 262, 524],
    onair_tones: 531,
};
/// Short ACK: 13 ShortFec bytes → 26 data tones + 2×7 Costas = 40 symbols (≈ 1.28 s). A short variant of
/// the same waveform. A single copy decodes ~0.90 at the 0 dB data floor (moderate/poor_f1, 400 trials) —
/// the earlier "~0.6" was a 40-trial small-sample artifact — but drops to ~0.6 just 3 dB below. For a
/// fade-margined ARQ return channel, transmit K=3 time-spaced copies and union-decode them with
/// [`openpulse_core::ack::decode_ack_from_llr_copies`] (≥0.99 at −3 dB, no frequency hop needed —
/// energy-summing faded copies does NOT work, the #694 lesson). See `robust_ack.rs` and
/// `docs/dev/research/robust-narrowband-measurement.md`.
const ACK_LAYOUT: Layout = Layout {
    frame_bytes: 13,
    sync_starts: &[0, 20],
    onair_tones: 40,
};

fn layout_for(mode: &str) -> Layout {
    if mode.eq_ignore_ascii_case("MFSK16-ACK") {
        ACK_LAYOUT
    } else {
        DATA_LAYOUT
    }
}

fn gfsk_params(sample_rate: u32) -> GfskParams {
    GfskParams {
        samples_per_symbol: SPS,
        tone_spacing_hz: SPACING,
        sample_rate,
        bt: DEFAULT_BT,
    }
}

/// The 16-tone comb's base (lowest tone), centered on `fc`.
fn base_freq(fc: f32) -> f32 {
    fc - (N_TONES as f32 - 1.0) / 2.0 * SPACING
}

fn sync_mask(lay: &Layout) -> Vec<bool> {
    let mut m = vec![false; lay.onair_tones];
    for &s in lay.sync_starts {
        for k in 0..COSTAS16.len() {
            m[s + k] = true;
        }
    }
    m
}

fn data_positions(lay: &Layout) -> Vec<usize> {
    let mask = sync_mask(lay);
    (0..lay.onair_tones).filter(|&p| !mask[p]).collect()
}

/// Bytes (padded/truncated to `lay.frame_bytes`) → LSB-first 4-bit tones (tone = Σ bit_{4j+b}·2^b).
fn bytes_to_data_tones(data: &[u8], lay: &Layout) -> Vec<u8> {
    let mut block = vec![0u8; lay.frame_bytes];
    let n = data.len().min(lay.frame_bytes);
    block[..n].copy_from_slice(&data[..n]);
    let mut bits = vec![0u8; lay.frame_bytes * 8];
    for (i, &byte) in block.iter().enumerate() {
        for k in 0..8 {
            bits[i * 8 + k] = (byte >> k) & 1;
        }
    }
    bits.chunks(BITS_PER_SYM)
        .map(|c| c[0] | (c[1] << 1) | (c[2] << 2) | (c[3] << 3))
        .collect()
}

/// Interleave the Costas sync blocks into the data tones → the on-air symbol sequence.
fn insert_sync(data_tones: &[u8], lay: &Layout) -> Vec<u8> {
    let mask = sync_mask(lay);
    let mut out = vec![0u8; lay.onair_tones];
    for &s in lay.sync_starts {
        out[s..s + COSTAS16.len()].copy_from_slice(&COSTAS16);
    }
    let mut di = 0;
    for (p, &is_sync) in mask.iter().enumerate() {
        if !is_sync {
            out[p] = data_tones[di];
            di += 1;
        }
    }
    out
}

fn sym_energies(win: &[f32], base: f32, fs: f32) -> [f32; N_TONES] {
    let mut e = [0f32; N_TONES];
    for (t, slot) in e.iter_mut().enumerate() {
        *slot = goertzel_energy(win, base + t as f32 * SPACING, fs);
    }
    e
}

/// Normalized Costas correlation over the sync symbols at `(offset, base)`; a perfect lock scores
/// `sync_starts.len()·7`, a noise window ≈ that/16.
fn sync_score(audio: &[f32], offset: usize, base: f32, fs: f32, lay: &Layout) -> Option<f32> {
    let mut score = 0.0f32;
    for &s in lay.sync_starts {
        for k in 0..COSTAS16.len() {
            let start = offset + (s + k) * SPS;
            let win = audio.get(start..start + SPS)?;
            let e = sym_energies(win, base, fs);
            let sum: f32 = e.iter().sum::<f32>() + 1e-9;
            score += e[COSTAS16[k] as usize] / sum;
        }
    }
    Some(score)
}

/// Acquire `(offset, base)` by maximising the normalized Costas score: coarse timing (symbol step) ×
/// frequency (±46.9 Hz @ 15.625), then refine (timing ±1 sym @ sps/8, frequency ±15.6 Hz @ 3.9). Gates at
/// 0.57× the perfect score (JS8's 12/21 fraction). The search absorbs the tuning offset.
fn acquire(audio: &[f32], nominal_base: f32, fs: f32, lay: &Layout) -> Option<(usize, f32)> {
    let span = lay.onair_tones * SPS;
    let max_offset = audio.len().saturating_sub(span);
    let coarse_freqs: Vec<f32> = (-3..=3)
        .map(|i| nominal_base + i as f32 * (SPACING / 2.0))
        .collect();
    let mut best: Option<(f32, usize, f32)> = None;
    let mut off = 0;
    loop {
        for &bf in &coarse_freqs {
            if let Some(sc) = sync_score(audio, off, bf, fs, lay) {
                if best.is_none_or(|(bs, _, _)| sc > bs) {
                    best = Some((sc, off, bf));
                }
            }
        }
        if off >= max_offset {
            break;
        }
        off = (off + SPS).min(max_offset);
    }
    let (_, coff, cbf) = best?;
    let mut refined: Option<(f32, usize, f32)> = None;
    let mut t = coff.saturating_sub(SPS);
    while t <= (coff + SPS).min(max_offset) {
        for i in -4..=4 {
            let bf = cbf + i as f32 * (SPACING / 8.0);
            if let Some(sc) = sync_score(audio, t, bf, fs, lay) {
                if refined.is_none_or(|(bs, _, _)| sc > bs) {
                    refined = Some((sc, t, bf));
                }
            }
        }
        t += SPS / 8;
    }
    let (score, off, bf) = refined?;
    let gate = 0.57 * (lay.sync_starts.len() * COSTAS16.len()) as f32;
    (score >= gate).then_some((off, bf))
}

/// Acquire, then return the data-symbol tone-energy arrays (sync blocks skipped). `None` on a failed gate.
fn acquire_and_energies(
    audio: &[f32],
    fc: f32,
    fs: f32,
    lay: &Layout,
) -> Option<Vec<[f32; N_TONES]>> {
    let (offset, base) = acquire(audio, base_freq(fc), fs, lay)?;
    let positions = data_positions(lay);
    let mut out = Vec::with_capacity(positions.len());
    for &p in &positions {
        let start = offset + p * SPS;
        let win = audio.get(start..start + SPS)?;
        out.push(sym_energies(win, base, fs));
    }
    Some(out)
}

/// Frame-level noise variance: the median of every data symbol's 15 non-winner tone energies, corrected
/// for the exponential distribution (`median ≈ mean·ln2`). Frame-level (noise is stationary; only the
/// signal fades) and median-based (resists tone leakage under Doppler) — the calibration the `llr_reliability`
/// gate requires. Falls back to `1e-9` if empty.
fn frame_noise(energies: &[[f32; N_TONES]]) -> f32 {
    let mut noise: Vec<f32> = Vec::with_capacity(energies.len() * (N_TONES - 1));
    for e in energies {
        let max = e.iter().cloned().fold(0.0f32, f32::max);
        let mut seen_max = false;
        for &v in e {
            if v == max && !seen_max {
                seen_max = true; // exclude exactly one winner
            } else {
                noise.push(v);
            }
        }
    }
    if noise.is_empty() {
        return 1e-9;
    }
    noise.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = noise[noise.len() / 2];
    (median / std::f32::consts::LN_2).max(1e-9)
}

/// Engine-convention max-log 4-bit LLRs for one symbol's tone energies (positive = bit 0, LSB-first).
fn bit_llrs(e: &[f32; N_TONES], inv_noise: f32) -> [f32; BITS_PER_SYM] {
    let mut out = [0f32; BITS_PER_SYM];
    for (b, slot) in out.iter_mut().enumerate() {
        let mask = 1usize << b;
        let mut e0 = f32::NEG_INFINITY;
        let mut e1 = f32::NEG_INFINITY;
        for (t, &energy) in e.iter().enumerate() {
            if t & mask != 0 {
                e1 = e1.max(energy);
            } else {
                e0 = e0.max(energy);
            }
        }
        *slot = (e0 - e1) * inv_noise;
    }
    out
}

/// The 16-GFSK weak-signal sub-floor modulation plugin.
pub struct Mfsk16Plugin {
    info: PluginInfo,
}

impl Default for Mfsk16Plugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Mfsk16Plugin {
    pub fn new() -> Self {
        Self {
            info: PluginInfo {
                name: "MFSK16".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description:
                    "Constant-envelope non-coherent 16-GFSK weak-signal sub-floor rung (REQ-WSIG-01)"
                        .to_string(),
                author: "OpenPulse Contributors".to_string(),
                supported_modes: vec!["MFSK16".to_string(), "MFSK16-ACK".to_string()],
                trait_version_required: "1.0".to_string(),
            },
        }
    }
}

impl ModulationPlugin for Mfsk16Plugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        let lay = layout_for(&config.mode);
        if data.len() > lay.frame_bytes {
            return Err(ModemError::Modulation(format!(
                "{} frame carries {} bytes; got {}",
                config.mode,
                lay.frame_bytes,
                data.len()
            )));
        }
        let onair = insert_sync(&bytes_to_data_tones(data, &lay), &lay);
        Ok(modulate_tones(
            &onair,
            base_freq(config.center_frequency),
            &gfsk_params(config.sample_rate),
        ))
    }

    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError> {
        let fs = config.sample_rate as f32;
        let lay = layout_for(&config.mode);
        let energies = acquire_and_energies(samples, config.center_frequency, fs, &lay)
            .ok_or_else(|| ModemError::Demodulation("MFSK16 acquisition failed".into()))?;
        // Hard decision = argmax tone per symbol → bits (consistent with the soft path's LLR signs).
        let mut bits = Vec::with_capacity(energies.len() * BITS_PER_SYM);
        for e in &energies {
            let tone = e
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(t, _)| t)
                .unwrap_or(0);
            for b in 0..BITS_PER_SYM {
                bits.push(((tone >> b) & 1) as u8);
            }
        }
        Ok(bits
            .chunks(8)
            .map(|c| {
                c.iter()
                    .enumerate()
                    .fold(0u8, |a, (i, &bit)| a | (bit << i))
            })
            .collect())
    }

    fn demodulate_soft(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<f32>, ModemError> {
        let fs = config.sample_rate as f32;
        let lay = layout_for(&config.mode);
        let energies = acquire_and_energies(samples, config.center_frequency, fs, &lay)
            .ok_or_else(|| ModemError::Demodulation("MFSK16 acquisition failed".into()))?;
        let inv_noise = 1.0 / frame_noise(&energies);
        let mut out = Vec::with_capacity(energies.len() * BITS_PER_SYM);
        for e in &energies {
            out.extend_from_slice(&bit_llrs(e, inv_noise));
        }
        Ok(out)
    }

    fn supports_soft_demod(&self) -> bool {
        true
    }

    /// Non-coherent: acquisition searches frequency internally, so the plugin opts out of the engine's
    /// coherent AFC chain (a returned estimate would double-correct).
    fn estimate_afc_hz(&self, _samples: &[f32], _config: &ModulationConfig) -> Option<f32> {
        None
    }

    fn frame_geometry(&self, config: &ModulationConfig) -> Option<FrameGeometry> {
        let lay = layout_for(&config.mode);
        let n = (config.sample_rate as f32 / SPACING).round() as usize;
        Some(FrameGeometry {
            symbol_period_samples: n,
            preamble_samples: n * COSTAS16.len(),
            min_frame_samples: n * lay.onair_tones,
            max_frame_samples: n * lay.onair_tones,
        })
    }

    fn occupied_bandwidth_hz(&self, mode: &str) -> Option<f32> {
        (mode.eq_ignore_ascii_case("MFSK16") || mode.eq_ignore_ascii_case("MFSK16-ACK"))
            .then_some(500.0)
    }
}

#[cfg(test)]
mod robust_ack;

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ModulationConfig {
        ModulationConfig {
            mode: "MFSK16".into(),
            center_frequency: 1500.0,
            sample_rate: 8000,
            ..Default::default()
        }
    }

    fn ack_cfg() -> ModulationConfig {
        ModulationConfig {
            mode: "MFSK16-ACK".into(),
            ..cfg()
        }
    }

    fn block(seed: u8) -> Vec<u8> {
        (0..DATA_LAYOUT.frame_bytes as u16)
            .map(|i| (i as u8).wrapping_add(seed))
            .collect()
    }

    #[test]
    fn clean_loopback_round_trips() {
        let plugin = Mfsk16Plugin::new();
        let data = block(7);
        let audio = plugin.modulate(&data, &cfg()).expect("modulate");
        assert_eq!(audio.len(), DATA_LAYOUT.onair_tones * SPS);
        let out = plugin.demodulate(&audio, &cfg()).expect("demodulate");
        assert_eq!(
            out, data,
            "hard demod must recover the 255-byte block clean"
        );
    }

    #[test]
    fn ack_frame_round_trips() {
        let plugin = Mfsk16Plugin::new();
        let data: Vec<u8> = (0..ACK_LAYOUT.frame_bytes as u8).collect(); // 13 ShortFec bytes
        let audio = plugin.modulate(&data, &ack_cfg()).expect("modulate ack");
        assert_eq!(audio.len(), ACK_LAYOUT.onair_tones * SPS); // 40 symbols ≈ 1.28 s
        let out = plugin
            .demodulate(&audio, &ack_cfg())
            .expect("demodulate ack");
        assert_eq!(
            out, data,
            "MFSK16-ACK must round-trip the 13-byte ACK block"
        );
    }

    #[test]
    fn soft_hard_agree_clean() {
        // Hard-slicing the soft LLRs must reproduce the hard demod (llr-convention invariant).
        let plugin = Mfsk16Plugin::new();
        let data = block(3);
        let audio = plugin.modulate(&data, &cfg()).unwrap();
        let hard = plugin.demodulate(&audio, &cfg()).unwrap();
        let llrs = plugin.demodulate_soft(&audio, &cfg()).unwrap();
        let sliced: Vec<u8> = llrs
            .chunks(8)
            .map(|c| {
                c.iter()
                    .enumerate()
                    .fold(0u8, |a, (i, &l)| a | ((l <= 0.0) as u8) << i)
            })
            .collect();
        assert_eq!(
            sliced, hard,
            "hard-sliced soft LLRs must equal the hard demod"
        );
    }

    #[test]
    fn acquires_with_a_lead_and_tuning_offset() {
        let plugin = Mfsk16Plugin::new();
        let data = block(1);
        // Modulate with the comb tuned +18 Hz off the receiver's nominal center, prepend a lead.
        let tx = plugin
            .modulate(
                &data,
                &ModulationConfig {
                    center_frequency: 1518.0,
                    ..cfg()
                },
            )
            .unwrap();
        let mut sig = vec![0.0f32; 300];
        sig.extend(tx);
        // Receiver believes fc = 1500; acquisition must find the +18 Hz offset and the lead.
        let out = plugin.demodulate(&sig, &cfg()).expect("acquire+demodulate");
        assert_eq!(out, data);
    }

    #[test]
    fn rejects_oversized_frame() {
        assert!(Mfsk16Plugin::new()
            .modulate(&vec![0u8; 256], &cfg())
            .is_err());
    }
}
