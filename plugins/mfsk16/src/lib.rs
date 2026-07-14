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
/// One fixed RS block: 255 wire bytes → 510 data tones + 3×7 sync = 531 on-air symbols.
const FRAME_BYTES: usize = 255;
const DATA_TONES: usize = FRAME_BYTES * 8 / BITS_PER_SYM; // 510
const SYNC_STARTS: [usize; 3] = [0, 262, 524];
const ONAIR_TONES: usize = DATA_TONES + 3 * COSTAS16.len(); // 531

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

fn sync_mask() -> [bool; ONAIR_TONES] {
    let mut m = [false; ONAIR_TONES];
    for &s in &SYNC_STARTS {
        for k in 0..COSTAS16.len() {
            m[s + k] = true;
        }
    }
    m
}

fn data_positions() -> Vec<usize> {
    let mask = sync_mask();
    (0..ONAIR_TONES).filter(|&p| !mask[p]).collect()
}

/// Bytes (padded/truncated to one 255-byte block) → LSB-first 4-bit tones (tone = Σ bit_{4j+b}·2^b).
fn bytes_to_data_tones(data: &[u8]) -> Vec<u8> {
    let mut block = [0u8; FRAME_BYTES];
    let n = data.len().min(FRAME_BYTES);
    block[..n].copy_from_slice(&data[..n]);
    let mut tones = Vec::with_capacity(DATA_TONES);
    let mut bits = [0u8; FRAME_BYTES * 8];
    for (i, &byte) in block.iter().enumerate() {
        for k in 0..8 {
            bits[i * 8 + k] = (byte >> k) & 1;
        }
    }
    for c in bits.chunks(BITS_PER_SYM) {
        tones.push(c[0] | (c[1] << 1) | (c[2] << 2) | (c[3] << 3));
    }
    tones
}

/// Interleave the 3 Costas sync blocks into the 510 data tones → the 531-symbol on-air sequence.
fn insert_sync(data_tones: &[u8]) -> Vec<u8> {
    let mask = sync_mask();
    let mut out = vec![0u8; ONAIR_TONES];
    for &s in &SYNC_STARTS {
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

/// Normalized Costas correlation over the 21 sync symbols at `(offset, base)`; perfect lock ≈ 21.
fn sync_score(audio: &[f32], offset: usize, base: f32, fs: f32) -> Option<f32> {
    let mut score = 0.0f32;
    for &s in &SYNC_STARTS {
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
/// 12/21. `nominal_base` is the comb base the receiver believes; the search absorbs the tuning offset.
fn acquire(audio: &[f32], nominal_base: f32, fs: f32) -> Option<(usize, f32)> {
    let span = ONAIR_TONES * SPS;
    let max_offset = audio.len().saturating_sub(span);
    let coarse_freqs: Vec<f32> = (-3..=3)
        .map(|i| nominal_base + i as f32 * (SPACING / 2.0))
        .collect();
    let mut best: Option<(f32, usize, f32)> = None;
    let mut off = 0;
    loop {
        for &bf in &coarse_freqs {
            if let Some(sc) = sync_score(audio, off, bf, fs) {
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
            if let Some(sc) = sync_score(audio, t, bf, fs) {
                if refined.is_none_or(|(bs, _, _)| sc > bs) {
                    refined = Some((sc, t, bf));
                }
            }
        }
        t += SPS / 8;
    }
    let (score, off, bf) = refined?;
    (score >= 12.0).then_some((off, bf))
}

/// Acquire, then return the 510 data-symbol tone-energy arrays (sync blocks skipped). `None` on a failed
/// acquisition gate.
fn acquire_and_energies(audio: &[f32], fc: f32, fs: f32) -> Option<Vec<[f32; N_TONES]>> {
    let (offset, base) = acquire(audio, base_freq(fc), fs)?;
    let positions = data_positions();
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
                supported_modes: vec!["MFSK16".to_string()],
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
        if data.len() > FRAME_BYTES {
            return Err(ModemError::Modulation(format!(
                "MFSK16 frame is one {FRAME_BYTES}-byte RS block; got {} bytes",
                data.len()
            )));
        }
        let onair = insert_sync(&bytes_to_data_tones(data));
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
        let energies = acquire_and_energies(samples, config.center_frequency, fs)
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
        let energies = acquire_and_energies(samples, config.center_frequency, fs)
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
        let n = (config.sample_rate as f32 / SPACING).round() as usize;
        Some(FrameGeometry {
            symbol_period_samples: n,
            preamble_samples: n * COSTAS16.len(),
            min_frame_samples: n * ONAIR_TONES,
            max_frame_samples: n * ONAIR_TONES,
        })
    }

    fn occupied_bandwidth_hz(&self, mode: &str) -> Option<f32> {
        mode.eq_ignore_ascii_case("MFSK16").then_some(500.0)
    }
}

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

    fn block(seed: u8) -> Vec<u8> {
        (0..FRAME_BYTES as u16)
            .map(|i| (i as u8).wrapping_add(seed))
            .collect()
    }

    #[test]
    fn clean_loopback_round_trips() {
        let plugin = Mfsk16Plugin::new();
        let data = block(7);
        let audio = plugin.modulate(&data, &cfg()).expect("modulate");
        assert_eq!(audio.len(), ONAIR_TONES * SPS);
        let out = plugin.demodulate(&audio, &cfg()).expect("demodulate");
        assert_eq!(
            out, data,
            "hard demod must recover the 255-byte block clean"
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
