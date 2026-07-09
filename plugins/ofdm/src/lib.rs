//! OFDM modulation plugin for OpenPulse.
//!
//! - `OFDM16`: 16 data SCs, BW ≈ 625 Hz, QPSK, ~889 bps — conservative HF use
//! - `OFDM52`: 52 data SCs, BW ≈ 2031 Hz, QPSK, ~2889 bps — good-channel HF use
//! - `OFDM52-{8PSK,16QAM,32QAM,64QAM}`: the 52-SC band at higher constellation
//!   orders (~4333 / 5778 / 7222 / 8667 bps gross) — the high-throughput /
//!   high-reliability HF path, run FEC-protected (soft).
//!
//! All modes use FFT=256, CP=32, centre at 1500 Hz (SC 48), per-subcarrier
//! constellation mapping (shared `openpulse_dsp::constellation`), and LS+ZF
//! per-subcarrier channel equalization on RX.  QPSK uses iterative PAPR clipping
//! (target 12 dB); the higher-order modes keep their natural PAPR (clipping
//! distortion breaks dense constellations) and rely on TX leveling/backoff.

pub mod channel;
pub mod demodulate;
pub mod modulate;
pub mod params;
pub mod scramble;

use openpulse_core::{
    error::ModemError,
    plugin::{FrameGeometry, ModulationConfig, ModulationPlugin, PluginInfo},
};

use crate::demodulate::{ofdm_demodulate, ofdm_demodulate_soft};
use crate::modulate::{ofdm_modulate, ofdm_modulate_iq};
use crate::params::{params_for_mode, SAMPLE_RATE, SYM_LEN};

/// OFDM plugin supporting OFDM16 and OFDM52 modes.
pub struct OfdmPlugin {
    info: PluginInfo,
}

impl OfdmPlugin {
    pub fn new() -> Self {
        Self {
            info: PluginInfo {
                name: "OFDM".into(),
                version: "0.1.0".into(),
                description: "OFDM multi-carrier HF plugin: OFDM16/52 (QPSK) + OFDM52 higher-order (8PSK/16QAM/32QAM/64QAM)".into(),
                author: "OpenPulse Contributors".into(),
                supported_modes: vec![
                    "OFDM16".into(),
                    "OFDM52".into(),
                    "OFDM52-8PSK".into(),
                    "OFDM52-16QAM".into(),
                    "OFDM52-32QAM".into(),
                    "OFDM52-64QAM".into(),
                ],
                trait_version_required: "1.0".into(),
            },
        }
    }
}

impl Default for OfdmPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ModulationPlugin for OfdmPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        if params_for_mode(&config.mode).is_none() {
            return Err(ModemError::Configuration(format!(
                "OFDM plugin: unknown mode '{}'",
                config.mode
            )));
        }
        if config.sample_rate != SAMPLE_RATE {
            return Err(ModemError::Configuration(format!(
                "OFDM plugin: sample_rate {} not supported; must be {SAMPLE_RATE}",
                config.sample_rate
            )));
        }
        if (config.center_frequency - 1500.0).abs() > 1.0 {
            return Err(ModemError::Configuration(format!(
                "OFDM plugin: center_frequency {:.1} not supported; must be 1500.0 Hz",
                config.center_frequency
            )));
        }
        Ok(ofdm_modulate(data, &config.mode))
    }

    fn modulate_iq(
        &self,
        data: &[u8],
        config: &ModulationConfig,
    ) -> Result<(Vec<f32>, Vec<f32>), ModemError> {
        if params_for_mode(&config.mode).is_none() {
            return Err(ModemError::Configuration(format!(
                "OFDM plugin: unknown mode '{}'",
                config.mode
            )));
        }
        let interleaved = ofdm_modulate_iq(data, &config.mode);
        let i_ch = interleaved.iter().step_by(2).copied().collect();
        let q_ch = interleaved.iter().skip(1).step_by(2).copied().collect();
        Ok((i_ch, q_ch))
    }

    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError> {
        if params_for_mode(&config.mode).is_none() {
            return Err(ModemError::Configuration(format!(
                "OFDM plugin: unknown mode '{}'",
                config.mode
            )));
        }
        if config.sample_rate != SAMPLE_RATE {
            return Err(ModemError::Configuration(format!(
                "OFDM plugin: sample_rate {} not supported; must be {SAMPLE_RATE}",
                config.sample_rate
            )));
        }
        if (config.center_frequency - 1500.0).abs() > 1.0 {
            return Err(ModemError::Configuration(format!(
                "OFDM plugin: center_frequency {:.1} not supported; must be 1500.0 Hz",
                config.center_frequency
            )));
        }
        ofdm_demodulate(samples, &config.mode)
    }

    fn demodulate_soft(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<f32>, ModemError> {
        if params_for_mode(&config.mode).is_none() {
            return Err(ModemError::Configuration(format!(
                "OFDM plugin: unknown mode '{}'",
                config.mode
            )));
        }
        ofdm_demodulate_soft(samples, &config.mode)
    }

    fn frame_geometry(&self, config: &ModulationConfig) -> Option<FrameGeometry> {
        let p = params_for_mode(&config.mode)?;
        // Schmidl-Cox preamble = one full OFDM symbol (body + CP).
        let preamble = SYM_LEN;
        // Largest frame: 2-byte length prefix + 255-byte RS block + margin.
        let max_data_syms = (260usize * 8).div_ceil(p.bits_per_symbol());
        Some(FrameGeometry {
            symbol_period_samples: SYM_LEN,
            preamble_samples: preamble,
            min_frame_samples: preamble + SYM_LEN,
            max_frame_samples: (preamble + max_data_syms * SYM_LEN) * 11 / 10,
        })
    }

    fn supports_soft_demod(&self) -> bool {
        true
    }

    fn estimate_afc_hz(&self, samples: &[f32], config: &ModulationConfig) -> Option<f32> {
        let p = params_for_mode(&config.mode)?;
        crate::channel::estimate_cfo_hz(samples, &p)
    }

    fn estimate_snr_db(&self, samples: &[f32], config: &ModulationConfig) -> Option<f32> {
        demodulate::estimate_snr_db(samples, &config.mode)
    }

    fn occupied_bandwidth_hz(&self, mode: &str) -> Option<f32> {
        params_for_mode(mode).map(|p| p.occupied_bw_hz())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::pilot_positions;
    use crate::modulate::{measure_papr, ofdm_modulate};
    use crate::params::{OFDM16, OFDM52, TARGET_PAPR_DB};

    fn mod_config(mode: &str) -> ModulationConfig {
        ModulationConfig {
            mode: mode.into(),
            center_frequency: 1500.0,
            sample_rate: 8000,
            ..ModulationConfig::default()
        }
    }

    #[test]
    fn occupied_bandwidth_tracks_subcarrier_span() {
        let p = OfdmPlugin::new();
        // OFDM16 spans 20 SCs × 31.25 Hz = 625 Hz; OFDM52 spans 65 × 31.25 = 2031.25 Hz.
        assert!((p.occupied_bandwidth_hz("OFDM16").unwrap() - 625.0).abs() < 1.0);
        assert!((p.occupied_bandwidth_hz("OFDM52").unwrap() - 2031.25).abs() < 1.0);
        assert_eq!(p.occupied_bandwidth_hz("not-a-mode"), None);
    }

    // The engine previously parsed OFDM52's subcarrier count as "52 baud";
    // the plugin now owns its geometry (block-symbol period).
    #[test]
    fn frame_geometry_uses_block_symbol_period() {
        use crate::params::SYM_LEN;
        let plugin = OfdmPlugin::new();
        let g = plugin
            .frame_geometry(&mod_config("OFDM52"))
            .expect("geometry");
        assert_eq!(g.symbol_period_samples, SYM_LEN);
        assert_eq!(g.preamble_samples, SYM_LEN);
        assert!(g.min_frame_samples > g.preamble_samples);
        assert!(g.max_frame_samples > g.min_frame_samples);
    }

    // Acquisition must lock the frame START, not its trailing edge.  The
    // classic Schmidl-Cox P²/R₂² metric (normalised by the second half-window
    // only) explodes where the first half holds the signal tail and the
    // second holds near-silence — on a quiet sound card M reaches 10³⁺ there,
    // beating the true preamble's M ≈ 1 and decoding garbage from the frame
    // end.  Reproduced from a hardware capture (rpi51→rpi52, 2026-06-12).
    #[test]
    fn ofdm16_acquires_frame_start_not_trailing_edge() {
        let plugin = OfdmPlugin::new();
        let payload: Vec<u8> = (0..64u8).collect();
        let frame = plugin.modulate(&payload, &mod_config("OFDM16")).unwrap();
        // Leading offset + frame + long quiet tail with a faint noise floor
        // (a pure-zero tail is masked by the r > 1e-9 guard; real cards give
        // small nonzero noise).
        let mut state = 0x1234u32;
        let mut noise = |amp: f32| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            ((state >> 16) as f32 / 32768.0 - 1.0) * amp
        };
        let mut rx = Vec::new();
        for _ in 0..200 {
            rx.push(noise(1e-4));
        }
        rx.extend_from_slice(&frame);
        for _ in 0..4000 {
            rx.push(noise(1e-4));
        }
        let decoded = plugin.demodulate(&rx, &mod_config("OFDM16")).unwrap();
        assert_eq!(decoded, payload, "must decode from the frame start");
    }

    // 1. OFDM16 clean loopback
    #[test]
    fn ofdm16_loopback_clean() {
        let plugin = OfdmPlugin::new();
        let payload = b"OFDM16 loopback test payload, hello";
        let samples = plugin.modulate(payload, &mod_config("OFDM16")).unwrap();
        let rx = plugin.demodulate(&samples, &mod_config("OFDM16")).unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    #[test]
    fn constellation_recovers_clean_equalized_symbols() {
        use crate::demodulate::ofdm_constellation;
        let plugin = OfdmPlugin::new();
        let payload = b"OFDM52 constellation extraction test payload, plenty of symbols here";
        let audio = plugin.modulate(payload, &mod_config("OFDM52")).unwrap();
        let pts = ofdm_constellation(&audio, "OFDM52").expect("constellation on a clean signal");
        assert!(!pts.is_empty(), "recovers equalized subcarrier symbols");
        let rms = (pts.iter().map(|&(i, q)| i * i + q * q).sum::<f32>() / pts.len() as f32).sqrt();
        assert!((rms - 1.0).abs() < 0.4, "normalized to RMS≈1, got {rms}");
        assert!(pts.iter().all(|&(i, q)| i.is_finite() && q.is_finite()));
        assert!(
            ofdm_constellation(&audio, "FSK4-ACK").is_none(),
            "unknown mode yields no constellation"
        );
    }

    // 2. OFDM52 clean loopback
    #[test]
    fn ofdm52_loopback_clean() {
        let plugin = OfdmPlugin::new();
        let payload = b"OFDM52 clean loopback test payload, more data here";
        let samples = plugin.modulate(payload, &mod_config("OFDM52")).unwrap();
        let rx = plugin.demodulate(&samples, &mod_config("OFDM52")).unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    // 2b. OFDM52-16QAM (4 bits/SC) clean loopback — the first higher-order rung.
    #[test]
    fn ofdm52_16qam_loopback_clean() {
        let plugin = OfdmPlugin::new();
        let payload: Vec<u8> = (0..96u8).map(|v| v.wrapping_mul(37)).collect();
        let samples = plugin
            .modulate(&payload, &mod_config("OFDM52-16QAM"))
            .unwrap();
        let rx = plugin
            .demodulate(&samples, &mod_config("OFDM52-16QAM"))
            .unwrap();
        assert_eq!(rx, payload, "OFDM52-16QAM clean loopback must round-trip");
    }

    // 2c. OFDM52-16QAM soft LLRs must hard-decide back to the payload on a clean channel.
    #[test]
    fn ofdm52_16qam_soft_llrs_clean() {
        let plugin = OfdmPlugin::new();
        let payload: Vec<u8> = (0..64u8).collect();
        let samples = plugin
            .modulate(&payload, &mod_config("OFDM52-16QAM"))
            .unwrap();
        let llrs = plugin
            .demodulate_soft(&samples, &mod_config("OFDM52-16QAM"))
            .unwrap();
        // Sign convention: positive LLR = bit 0. Pack LSB-first like the modulator.
        let bytes: Vec<u8> = llrs
            .chunks(8)
            .map(|c| {
                c.iter()
                    .enumerate()
                    .fold(0u8, |a, (i, &l)| a | (((l < 0.0) as u8) << i))
            })
            .collect();
        assert!(bytes.len() >= payload.len());
        assert_eq!(
            &bytes[..payload.len()],
            &payload[..],
            "OFDM52-16QAM soft LLRs must decode the payload on a clean channel"
        );
    }

    // 2d. The remaining higher-order rungs round-trip cleanly.
    #[test]
    fn ofdm52_higher_order_round_trips() {
        let plugin = OfdmPlugin::new();
        for mode in ["OFDM52-8PSK", "OFDM52-32QAM", "OFDM52-64QAM"] {
            let payload: Vec<u8> = (0..120u8).map(|v| v.wrapping_mul(53)).collect();
            let samples = plugin.modulate(&payload, &mod_config(mode)).unwrap();
            let rx = plugin.demodulate(&samples, &mod_config(mode)).unwrap();
            assert_eq!(rx, payload, "{mode} clean loopback must round-trip");
        }
    }

    // 2e. OFDM52-16QAM through Watterson Good-F1: per-SC ZF equalization + CP must
    // tame the (mild) frequency-selective fading so uncoded BER stays well below the
    // 0.5 random floor (soft FEC then closes the residual at the engine level).
    #[test]
    fn ofdm52_16qam_watterson_good_f1_equalizes() {
        use openpulse_channel::watterson::WattersonChannel;
        use openpulse_channel::{ChannelModel, WattersonConfig};

        let plugin = OfdmPlugin::new();
        let payload: Vec<u8> = (0..96u8).map(|v| v ^ 0x3C).collect();
        let tx = plugin
            .modulate(&payload, &mod_config("OFDM52-16QAM"))
            .unwrap();

        let ber = |got: &[u8]| -> f32 {
            let e: u32 = payload
                .iter()
                .zip(got.iter())
                .map(|(a, b)| (a ^ b).count_ones())
                .sum();
            let missing = payload.len().saturating_sub(got.len()) as u32 * 8;
            (e + missing) as f32 / (payload.len() as f32 * 8.0)
        };

        let mut best = 1.0f32;
        for seed in [0xF1u64, 0xF2, 0xF3, 0xF4] {
            let mut ch = WattersonChannel::new(WattersonConfig::good_f1(Some(seed))).unwrap();
            let rx = ch.apply(&tx);
            if let Ok(got) = plugin.demodulate(&rx, &mod_config("OFDM52-16QAM")) {
                best = best.min(ber(&got));
            }
        }
        // Measured: best uncoded BER = 0.0000 — the per-SC equalizer fully tames
        // mild Good-F1 fading.  Assert a tight bound to catch equalizer regressions.
        assert!(
            best < 0.02,
            "Good-F1 best uncoded BER = {best:.4} (expected < 0.02)"
        );
    }

    // 3. Short payload (1 byte) — length prefix must survive round-trip
    #[test]
    fn ofdm16_loopback_short_payload() {
        let plugin = OfdmPlugin::new();
        let payload = b"X";
        let samples = plugin.modulate(payload, &mod_config("OFDM16")).unwrap();
        let rx = plugin.demodulate(&samples, &mod_config("OFDM16")).unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    // 4. OFDM52 max single-symbol payload (bytes_per_symbol = 52*2/8 = 13 B; minus 2 for prefix = 11 B)
    #[test]
    fn ofdm52_loopback_max_single_symbol() {
        let plugin = OfdmPlugin::new();
        // 52 data SCs × 2 bits = 104 bits = 13 bytes; minus 2-byte prefix = 11 bytes payload.
        let payload = b"11bytepayl!";
        assert_eq!(payload.len(), 11);
        let samples = plugin.modulate(payload, &mod_config("OFDM52")).unwrap();
        let rx = plugin.demodulate(&samples, &mod_config("OFDM52")).unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    // 5. OFDM16 pilot positions
    #[test]
    fn ofdm16_pilot_positions() {
        let pilots = pilot_positions(&OFDM16);
        assert_eq!(pilots.len(), OFDM16.n_pilots, "pilot count mismatch");
        // All pilots must fall within [first_sc, last_sc].
        for &sc in &pilots {
            assert!((OFDM16.first_sc..=OFDM16.last_sc).contains(&sc));
        }
        // Pilots: 38+4=42, 47, 52, 57
        assert_eq!(pilots, vec![42, 47, 52, 57]);
    }

    // 6. OFDM52 pilot positions
    #[test]
    fn ofdm52_pilot_positions() {
        let pilots = pilot_positions(&OFDM52);
        assert_eq!(pilots.len(), OFDM52.n_pilots, "pilot count mismatch");
        for &sc in &pilots {
            assert!((OFDM52.first_sc..=OFDM52.last_sc).contains(&sc));
        }
        // first pilot: 16+4=20; last: 80
        assert_eq!(pilots[0], 20);
        assert_eq!(*pilots.last().unwrap(), 80);
    }

    // 7. OFDM16 PAPR after clipping
    #[test]
    fn ofdm16_papr_after_clip() {
        // Generate without internal clip to measure raw PAPR, then clip.
        // We test the output of ofdm_modulate (which includes clipping).
        let samples = ofdm_modulate(b"OFDM16 papr test payload", "OFDM16");
        let papr = measure_papr(&samples);
        assert!(
            papr <= TARGET_PAPR_DB + 0.5,
            "OFDM16 PAPR {papr:.1} dB exceeds target {TARGET_PAPR_DB} dB"
        );
    }

    // 8. OFDM52 PAPR after clipping
    #[test]
    fn ofdm52_papr_after_clip() {
        let samples = ofdm_modulate(
            b"OFDM52 papr test payload longer text here for more subcarriers",
            "OFDM52",
        );
        let papr = measure_papr(&samples);
        assert!(
            papr <= TARGET_PAPR_DB + 0.5,
            "OFDM52 PAPR {papr:.1} dB exceeds target {TARGET_PAPR_DB} dB"
        );
    }

    // Extra: unknown mode returns Err
    #[test]
    fn unknown_mode_returns_err() {
        let plugin = OfdmPlugin::new();
        let cfg = mod_config("OFDM99");
        assert!(plugin.modulate(b"x", &cfg).is_err());
        let samples = vec![0.0f32; 288];
        assert!(plugin.demodulate(&samples, &cfg).is_err());
        assert!(plugin.demodulate_soft(&samples, &cfg).is_err());
    }

    // Soft demod: LLR count must be at least 8× payload bytes (1 byte = 8 bits = 8 LLRs).
    #[test]
    fn ofdm16_soft_demod_llr_count() {
        let plugin = OfdmPlugin::new();
        let payload = b"soft test";
        let samples = plugin.modulate(payload, &mod_config("OFDM16")).unwrap();
        let llrs = plugin
            .demodulate_soft(&samples, &mod_config("OFDM16"))
            .unwrap();
        assert!(
            llrs.len() >= payload.len() * 8,
            "too few LLRs: {}",
            llrs.len()
        );
        assert!(llrs.iter().all(|v| v.is_finite()), "non-finite LLR");
    }

    // Soft demod: LLR sign convention — signs must agree with hard decisions.
    #[test]
    fn ofdm52_soft_demod_sign_convention() {
        let plugin = OfdmPlugin::new();
        // Mixed payload to avoid worst-case coherent PAPR clipping (all-zero maximizes peak).
        let payload: Vec<u8> = (0u8..=63u8).collect();
        let samples = plugin.modulate(&payload, &mod_config("OFDM52")).unwrap();
        let llrs = plugin
            .demodulate_soft(&samples, &mod_config("OFDM52"))
            .unwrap();
        let bits_hard = plugin.demodulate(&samples, &mod_config("OFDM52")).unwrap();

        // For each payload bit: LLR > 0 must agree with bit == 0.
        let mut matches = 0usize;
        let mut total = 0usize;
        for (byte_idx, &b) in bits_hard.iter().take(payload.len()).enumerate() {
            for bit in 0..8usize {
                let llr_idx = byte_idx * 8 + bit;
                if llr_idx >= llrs.len() {
                    break;
                }
                let bit_val = (b >> bit) & 1;
                let llr_positive = llrs[llr_idx] > 0.0;
                if (bit_val == 0) == llr_positive {
                    matches += 1;
                }
                total += 1;
            }
        }
        assert!(
            matches > total * 9 / 10,
            "LLR signs disagree with hard decisions: {matches}/{total} agree"
        );
    }

    // supports_soft_demod must be true for OFDM plugin.
    #[test]
    fn ofdm_supports_soft_demod() {
        let plugin = OfdmPlugin::new();
        assert!(plugin.supports_soft_demod());
    }

    // Timing acquisition: frame arriving at a non-symbol-aligned sample offset,
    // preceded by leading capture noise, must still decode.  This is the
    // hardware-loopback condition the synchronous channel-sim harness never
    // exercises (it routes frame-aligned samples).
    #[test]
    fn ofdm52_acquires_with_leading_offset_and_noise() {
        let plugin = OfdmPlugin::new();
        let payload: Vec<u8> = (0u8..64).collect();
        let frame = plugin.modulate(&payload, &mod_config("OFDM52")).unwrap();

        // Deterministic low-level pre-roll noise at an offset that is NOT a
        // multiple of the OFDM symbol period (SYM_LEN = 288).
        let mut buf = Vec::new();
        let mut seed = 0x1234_5678u32;
        for _ in 0..137 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            buf.push(((seed >> 8) as f32 / u32::MAX as f32 - 0.5) * 0.01);
        }
        buf.extend_from_slice(&frame);
        buf.extend(std::iter::repeat_n(0.0, 300));

        let rx = plugin.demodulate(&buf, &mod_config("OFDM52")).unwrap();
        assert_eq!(rx.as_slice(), payload.as_slice());
    }

    #[test]
    fn ofdm16_acquires_with_leading_offset() {
        let plugin = OfdmPlugin::new();
        let payload = b"timing acquisition";
        let frame = plugin.modulate(payload, &mod_config("OFDM16")).unwrap();
        let mut buf = vec![0.0f32; 211];
        buf.extend_from_slice(&frame);
        let rx = plugin.demodulate(&buf, &mod_config("OFDM16")).unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    // AFC estimator: on-carrier signal returns near-zero estimate.
    #[test]
    fn afc_estimate_near_zero_ofdm16() {
        let plugin = OfdmPlugin::new();
        let cfg = mod_config("OFDM16");
        let payload: Vec<u8> = (0..32u8).collect();
        let samples = plugin.modulate(&payload, &cfg).unwrap();
        let est = plugin
            .estimate_afc_hz(&samples, &cfg)
            .expect("afc estimate");
        assert!(est.abs() < 5.0, "expected near-zero AFC, got {est:.2} Hz");
    }

    // AFC estimator: on-carrier OFDM52 signal returns near-zero estimate.
    #[test]
    fn afc_estimate_near_zero_ofdm52() {
        let plugin = OfdmPlugin::new();
        let cfg = mod_config("OFDM52");
        let payload: Vec<u8> = (0..64u8).collect();
        let samples = plugin.modulate(&payload, &cfg).unwrap();
        let est = plugin
            .estimate_afc_hz(&samples, &cfg)
            .expect("afc estimate");
        assert!(est.abs() < 5.0, "expected near-zero AFC, got {est:.2} Hz");
    }

    // AFC estimator: synthetic signal with known inter-symbol pilot phase drift.
    //
    // Constructs pilot-only OFDM symbols where consecutive pilots are phase-rotated
    // by the amount expected for cfo_hz, then verifies the estimator recovers it.
    #[test]
    fn afc_synthetic_pilot_phase_drift_ofdm16() {
        use crate::channel::{estimate_cfo_hz, pilot_positions};
        use crate::params::{CP, FFT_SIZE, PILOT_AMPLITUDE, SAMPLE_RATE, SYM_LEN};
        use num_complex::Complex32;
        use rustfft::FftPlanner;
        use std::f32::consts::PI;

        let cfo_hz = 8.0_f32;
        let delta_phi = 2.0 * PI * cfo_hz * SYM_LEN as f32 / SAMPLE_RATE as f32;
        let p = OFDM16;
        let pilots = pilot_positions(&p);
        let n_syms = 4usize;

        let mut planner = FftPlanner::<f32>::new();
        let ifft = planner.plan_fft_inverse(FFT_SIZE);
        let scale = 1.0 / FFT_SIZE as f32;

        let mut samples = Vec::with_capacity(n_syms * SYM_LEN);
        for sym_idx in 0..n_syms {
            let phase = sym_idx as f32 * delta_phi;
            let (sin_p, cos_p) = phase.sin_cos();
            let mut freq = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
            for &k in &pilots {
                // Hermitian symmetry: mirror at FFT_SIZE-k with conjugated phase.
                freq[k] = Complex32::new(cos_p, sin_p) * PILOT_AMPLITUDE;
                freq[FFT_SIZE - k] = Complex32::new(cos_p, -sin_p) * PILOT_AMPLITUDE;
            }
            ifft.process(&mut freq);
            let time: Vec<f32> = freq.iter().map(|c| c.re * scale).collect();
            let cp_start = FFT_SIZE - CP;
            samples.extend_from_slice(&time[cp_start..]);
            samples.extend_from_slice(&time);
        }

        let est = estimate_cfo_hz(&samples, &p).expect("synthetic cfo estimate");
        assert!(
            (est - cfo_hz).abs() < 2.0,
            "expected ~{cfo_hz:.1} Hz CFO, got {est:.2} Hz"
        );
    }
}
