//! `Js8Plugin`: the JS8 waveform as a [`ModulationPlugin`].
//!
//! `modulate` is the full TX chain — a packed JS8 message (10 bytes: 72-bit payload + 3-bit flags +
//! 5 pad) → [`js8_info_bits`] → LDPC → [`message_to_tones`] → GFSK audio. `demodulate` runs the
//! window decoder ([`decode_window`]) over the captured audio and returns the strongest CRC-valid
//! frame; the discovery service uses [`decode_window`] directly for a full-passband, T/R-scheduled
//! window.
//!
//! One deliberate deviation from the other plugins: a JS8 frame must go on the wire **without** the
//! OpenPulse `Frame` envelope (interop needs byte-exact JS8), so the discovery service calls
//! `modulate` directly rather than through `engine.transmit()`.

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{FrameGeometry, ModulationConfig, ModulationPlugin, PluginInfo};

use crate::decoder::{decode_window, DecodeCfg};
use crate::message::js8_info_bits;
use crate::modulate::{modulate_tones, GfskParams};
use crate::submode::{params_for_mode, SubmodeParams, COSTAS_LEN};
use crate::tones::message_to_tones;

/// The JS8 modulation plugin (NORMAL submode is the MVP; all five are recognized).
pub struct Js8Plugin {
    info: PluginInfo,
}

impl Js8Plugin {
    /// Create the plugin with its static [`PluginInfo`].
    pub fn new() -> Self {
        Self {
            info: PluginInfo {
                name: "JS8".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "JS8-compatible 8-GFSK weak-signal waveform".to_string(),
                author: "OpenPulse".to_string(),
                supported_modes: vec![
                    "JS8-SLOW".to_string(),
                    "JS8-NORMAL".to_string(),
                    "JS8-FAST".to_string(),
                    "JS8-TURBO".to_string(),
                    "JS8-ULTRA".to_string(),
                ],
                trait_version_required: "1.0".to_string(),
            },
        }
    }
}

impl Default for Js8Plugin {
    fn default() -> Self {
        Self::new()
    }
}

/// Split a packed JS8 message into its 72-bit payload (9 bytes) and 3-bit `i3bit` flags. Shorter
/// input is zero-padded; extra bytes are ignored.
fn split_message(data: &[u8]) -> ([u8; 9], u8) {
    let mut msg = [0u8; 10];
    let n = data.len().min(10);
    msg[..n].copy_from_slice(&data[..n]);
    let mut payload9 = [0u8; 9];
    payload9.copy_from_slice(&msg[..9]);
    (payload9, msg[9] >> 5)
}

impl ModulationPlugin for Js8Plugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        let params: SubmodeParams = params_for_mode(&config.mode)
            .ok_or_else(|| ModemError::Modulation(format!("unknown JS8 mode {}", config.mode)))?;
        let (payload9, i3bit) = split_message(data);
        let info = js8_info_bits(&payload9, i3bit);
        let tones = message_to_tones(&info, params.costas);
        Ok(modulate_tones(
            &tones,
            config.center_frequency,
            &GfskParams::from_submode(&params),
        ))
    }

    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError> {
        let params = params_for_mode(&config.mode)
            .ok_or_else(|| ModemError::Demodulation(format!("unknown JS8 mode {}", config.mode)))?;
        // Search a narrow band around the configured audio frequency; the slot is assumed aligned to
        // the buffer start (the discovery service uses `decode_window` directly for a full passband /
        // T-R-scheduled window). Return the strongest CRC-valid decode as its packed 10-byte frame.
        let base = config.center_frequency;
        let cfg = DecodeCfg {
            base_min: (base - 20.0).max(1.0),
            base_max: base + 20.0,
            base_step: params.tone_spacing_hz / 2.0,
            max_offset: 0,
            offset_step: 1,
            ..DecodeCfg::default()
        };
        let best = decode_window(samples, &params, &cfg)
            .into_iter()
            .max_by(|a, b| a.sync_score.total_cmp(&b.sync_score))
            .ok_or_else(|| ModemError::Demodulation("no JS8 frame decoded".to_string()))?;
        let mut frame = best.payload.to_vec();
        frame.push(best.i3bit << 5);
        Ok(frame)
    }

    fn supports_mode(&self, mode: &str) -> bool {
        params_for_mode(mode).is_some()
    }

    fn frame_geometry(&self, config: &ModulationConfig) -> Option<FrameGeometry> {
        let p = params_for_mode(&config.mode)?;
        let period = p.samples_per_period();
        Some(FrameGeometry {
            symbol_period_samples: p.samples_per_symbol,
            preamble_samples: COSTAS_LEN * p.samples_per_symbol,
            min_frame_samples: period,
            max_frame_samples: period,
        })
    }

    fn occupied_bandwidth_hz(&self, mode: &str) -> Option<f32> {
        params_for_mode(mode).map(|p| p.bandwidth_hz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::js8_info_bits;
    use crate::submode::{params, Submode, NUM_SYMBOLS, NUM_TONES};
    use crate::tones::message_to_tones;
    use openpulse_core::plugin::{PulseShape, PLUGIN_TRAIT_VERSION};

    fn cfg(mode: &str) -> ModulationConfig {
        ModulationConfig {
            center_frequency: 1500.0,
            sample_rate: 8000,
            mode: mode.to_string(),
            pulse_shape: PulseShape::default(),
            afc_correction_hz: 0.0,
        }
    }

    /// Dominant tone in a symbol window (Goertzel over the 8 candidate frequencies).
    fn detect_tone(win: &[f32], base: f32, spacing: f32, fs: f32) -> u8 {
        (0..NUM_TONES)
            .max_by(|&a, &b| {
                let g = |t: usize| {
                    let f = base + t as f32 * spacing;
                    let w = std::f32::consts::TAU * f / fs;
                    let coeff = 2.0 * w.cos();
                    let (mut s1, mut s2) = (0.0f32, 0.0f32);
                    for &v in win {
                        let s0 = v + coeff * s1 - s2;
                        s2 = s1;
                        s1 = s0;
                    }
                    s1 * s1 + s2 * s2 - coeff * s1 * s2
                };
                g(a).total_cmp(&g(b))
            })
            .unwrap() as u8
    }

    #[test]
    fn info_and_geometry() {
        let p = Js8Plugin::new();
        assert_eq!(p.info().name, "JS8");
        // Required trait major version matches the framework's.
        assert_eq!(
            p.info().trait_version_required.split('.').next(),
            PLUGIN_TRAIT_VERSION.split('.').next()
        );
        assert!(p.supports_mode("JS8-NORMAL"));
        assert!(p.supports_mode("js8-ultra"));
        assert!(!p.supports_mode("BPSK250"));
        assert_eq!(p.occupied_bandwidth_hz("JS8-NORMAL"), Some(50.0));
        let g = p.frame_geometry(&cfg("JS8-NORMAL")).unwrap();
        assert_eq!(g.min_frame_samples, 101_120);
        assert_eq!(g.max_frame_samples, 101_120);
    }

    #[test]
    fn modulate_unknown_mode_errors() {
        assert!(Js8Plugin::new()
            .modulate(&[0u8; 10], &cfg("JS8-HYPER"))
            .is_err());
    }

    #[test]
    fn demodulate_fails_on_silence() {
        assert!(Js8Plugin::new()
            .demodulate(&[0.0; 200_000], &cfg("JS8-NORMAL"))
            .is_err());
    }

    #[test]
    fn plugin_modulate_demodulate_round_trip() {
        // The plugin round-trips through the ModulationPlugin trait: modulate → demodulate recovers
        // the packed frame (payload + flags; the 5 pad bits are not carried).
        let plugin = Js8Plugin::new();
        let c = cfg("JS8-NORMAL");
        let msg: Vec<u8> = (0..10u8)
            .map(|i| i.wrapping_mul(53).wrapping_add(9))
            .collect();
        let audio = plugin.modulate(&msg, &c).unwrap();
        let frame = plugin.demodulate(&audio, &c).unwrap();
        assert_eq!(frame.len(), 10);
        let (payload, i3bit) = split_message(&msg);
        assert_eq!(&frame[..9], &payload);
        assert_eq!(frame[9] >> 5, i3bit);
    }

    #[test]
    fn modulate_produces_the_correct_on_air_tone_sequence() {
        // The full TX chain: a packed message → audio whose 79 symbols Goertzel-decode back to exactly
        // message_to_tones(js8_info_bits(...)). This validates modulate end to end (short of a bit-exact
        // WAV compare against genjs8, which needs gfortran).
        let plugin = Js8Plugin::new();
        let c = cfg("JS8-NORMAL");
        let msg: Vec<u8> = (0..10u8)
            .map(|i| i.wrapping_mul(37).wrapping_add(5))
            .collect();
        let audio = plugin.modulate(&msg, &c).unwrap();

        let sm = params(Submode::Normal);
        assert_eq!(audio.len(), sm.samples_per_period());

        let (payload9, i3bit) = split_message(&msg);
        let expected = message_to_tones(&js8_info_bits(&payload9, i3bit), sm.costas);

        let sps = sm.samples_per_symbol;
        for s in 0..NUM_SYMBOLS {
            let win = &audio[s * sps..(s + 1) * sps];
            let got = detect_tone(
                win,
                c.center_frequency,
                sm.tone_spacing_hz,
                c.sample_rate as f32,
            );
            assert_eq!(got, expected[s], "symbol {s}");
        }
    }
}
