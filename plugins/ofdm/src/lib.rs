//! OFDM modulation plugin for OpenPulse (FF-4).
//!
//! Two modes are provided:
//! - `OFDM16`: 16 data SCs, BW ≈ 625 Hz, gross ~889 bps — conservative HF use
//! - `OFDM52`: 52 data SCs, BW ≈ 2031 Hz, gross ~2889 bps — good-channel HF use
//!
//! Both modes use FFT=256, CP=32, QPSK per-subcarrier, centre at 1500 Hz (SC 48),
//! iterative PAPR clipping (target 6 dB), and LS+ZF channel equalization on RX.

pub mod channel;
pub mod demodulate;
pub mod modulate;
pub mod params;

use openpulse_core::{
    error::ModemError,
    plugin::{ModulationConfig, ModulationPlugin, PluginInfo},
};

use crate::demodulate::ofdm_demodulate;
use crate::modulate::ofdm_modulate;
use crate::params::params_for_mode;

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
                description: "OFDM multi-carrier HF plugin (FF-4): OFDM16 and OFDM52".into(),
                author: "OpenPulse Contributors".into(),
                supported_modes: vec!["OFDM16".into(), "OFDM52".into()],
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
        Ok(ofdm_modulate(data, &config.mode))
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
        Ok(ofdm_demodulate(samples, &config.mode))
    }

    // AFC: OFDM channel estimation is per-subcarrier; no global frequency offset.
    fn estimate_afc_hz(&self, _samples: &[f32], _config: &ModulationConfig) -> Option<f32> {
        Some(0.0)
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

    // 1. OFDM16 clean loopback
    #[test]
    fn ofdm16_loopback_clean() {
        let plugin = OfdmPlugin::new();
        let payload = b"OFDM16 loopback test payload, hello";
        let samples = plugin.modulate(payload, &mod_config("OFDM16")).unwrap();
        let rx = plugin.demodulate(&samples, &mod_config("OFDM16")).unwrap();
        assert_eq!(&rx[..payload.len().min(rx.len())], payload);
    }

    // 2. OFDM52 clean loopback
    #[test]
    fn ofdm52_loopback_clean() {
        let plugin = OfdmPlugin::new();
        let payload = b"OFDM52 clean loopback test payload, more data here";
        let samples = plugin.modulate(payload, &mod_config("OFDM52")).unwrap();
        let rx = plugin.demodulate(&samples, &mod_config("OFDM52")).unwrap();
        assert_eq!(&rx[..payload.len().min(rx.len())], payload);
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
            assert!(sc >= OFDM16.first_sc && sc <= OFDM16.last_sc);
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
            assert!(sc >= OFDM52.first_sc && sc <= OFDM52.last_sc);
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
    }
}
