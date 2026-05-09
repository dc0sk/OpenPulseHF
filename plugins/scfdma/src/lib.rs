//! SC-FDMA modulation plugin for OpenPulse (FF-12).
//!
//! SC-FDMA (DFT-spread OFDM) adds a DFT precoding step before the OFDM IFFT,
//! spreading each symbol across all allocated subcarriers.  The transmitted
//! signal resembles single-carrier: 3–4 dB lower PAPR than OFDM without any
//! iterative clipping, while using identical LS+ZF channel equalization on RX.
//!
//! Two modes match the OFDM bandwidth slots:
//! - `SCFDMA16`: 16 data SCs, BW ≈ 625 Hz, gross ~889 bps
//! - `SCFDMA52`: 52 data SCs, BW ≈ 2031 Hz, gross ~2889 bps

pub mod channel;
pub mod demodulate;
pub mod modulate;
pub mod params;

use openpulse_core::{
    error::ModemError,
    plugin::{ModulationConfig, ModulationPlugin, PluginInfo},
};

use crate::demodulate::scfdma_demodulate;
use crate::modulate::scfdma_modulate;
use crate::params::{params_for_mode, SAMPLE_RATE};

/// SC-FDMA plugin supporting SCFDMA16 and SCFDMA52 modes.
pub struct ScFdmaPlugin {
    info: PluginInfo,
}

impl ScFdmaPlugin {
    pub fn new() -> Self {
        Self {
            info: PluginInfo {
                name: "SC-FDMA".into(),
                version: "0.1.0".into(),
                description: "SC-FDMA (DFT-spread OFDM) HF plugin (FF-12): SCFDMA16 and SCFDMA52"
                    .into(),
                author: "OpenPulse Contributors".into(),
                supported_modes: vec!["SCFDMA16".into(), "SCFDMA52".into()],
                trait_version_required: "1.0".into(),
            },
        }
    }
}

impl Default for ScFdmaPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ModulationPlugin for ScFdmaPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        if params_for_mode(&config.mode).is_none() {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: unknown mode '{}'",
                config.mode
            )));
        }
        if config.sample_rate != SAMPLE_RATE {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: sample_rate {} not supported; must be {SAMPLE_RATE}",
                config.sample_rate
            )));
        }
        if (config.center_frequency - 1500.0).abs() > 1.0 {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: center_frequency {:.1} not supported; must be 1500.0 Hz",
                config.center_frequency
            )));
        }
        Ok(scfdma_modulate(data, &config.mode))
    }

    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError> {
        if params_for_mode(&config.mode).is_none() {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: unknown mode '{}'",
                config.mode
            )));
        }
        if config.sample_rate != SAMPLE_RATE {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: sample_rate {} not supported; must be {SAMPLE_RATE}",
                config.sample_rate
            )));
        }
        if (config.center_frequency - 1500.0).abs() > 1.0 {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: center_frequency {:.1} not supported; must be 1500.0 Hz",
                config.center_frequency
            )));
        }
        Ok(scfdma_demodulate(samples, &config.mode))
    }

    // Per-subcarrier LS/ZF equalization handles channel phase; no global CFO estimator.
    fn estimate_afc_hz(&self, _samples: &[f32], _config: &ModulationConfig) -> Option<f32> {
        None
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::pilot_positions;
    use crate::modulate::measure_papr;
    use crate::params::{SCFDMA16, SCFDMA52};

    fn mod_config(mode: &str) -> ModulationConfig {
        ModulationConfig {
            mode: mode.into(),
            center_frequency: 1500.0,
            sample_rate: 8000,
            ..ModulationConfig::default()
        }
    }

    #[test]
    fn scfdma16_loopback_clean() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"SCFDMA16 loopback test payload, hello!";
        let samples = plugin.modulate(payload, &mod_config("SCFDMA16")).unwrap();
        let rx = plugin
            .demodulate(&samples, &mod_config("SCFDMA16"))
            .unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    #[test]
    fn scfdma52_loopback_clean() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"SCFDMA52 clean loopback test payload, more data here!";
        let samples = plugin.modulate(payload, &mod_config("SCFDMA52")).unwrap();
        let rx = plugin
            .demodulate(&samples, &mod_config("SCFDMA52"))
            .unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    #[test]
    fn scfdma16_loopback_short_payload() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"X";
        let samples = plugin.modulate(payload, &mod_config("SCFDMA16")).unwrap();
        let rx = plugin
            .demodulate(&samples, &mod_config("SCFDMA16"))
            .unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    #[test]
    fn scfdma16_pilot_positions_correct() {
        let pilots = pilot_positions(&SCFDMA16);
        assert_eq!(pilots, vec![42, 47, 52, 57]);
        assert_eq!(pilots.len(), SCFDMA16.n_pilots);
    }

    #[test]
    fn scfdma52_pilot_positions_correct() {
        let pilots = pilot_positions(&SCFDMA52);
        assert_eq!(pilots.len(), SCFDMA52.n_pilots);
        assert_eq!(pilots[0], 20);
        assert_eq!(*pilots.last().unwrap(), 80);
    }

    #[test]
    fn unknown_mode_returns_err() {
        let plugin = ScFdmaPlugin::new();
        let cfg = mod_config("SCFDMA99");
        assert!(plugin.modulate(b"x", &cfg).is_err());
        let samples = vec![0.0f32; 288];
        assert!(plugin.demodulate(&samples, &cfg).is_err());
    }

    #[test]
    fn scfdma52_papr_is_below_12db() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"SC-FDMA PAPR test payload longer text here for more subcarriers fill";
        let samples = plugin.modulate(payload, &mod_config("SCFDMA52")).unwrap();
        let papr = measure_papr(&samples);
        // Localized SC-FDMA with 52 of 256 subcarriers achieves ~8-11 dB PAPR without
        // hard clipping.  OFDM with the same allocation clips to a 6 dB target,
        // introducing OOB spectral regrowth; SC-FDMA avoids that distortion.
        assert!(
            papr < 12.0,
            "SC-FDMA PAPR {papr:.1} dB should be below 12 dB (no clipping applied)"
        );
    }
}
