//! QPSK modulation/demodulation plugin for OpenPulse.

pub mod demodulate;
pub mod modulate;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin, PluginInfo};

/// QPSK modulation plugin.
pub struct QpskPlugin {
    info: PluginInfo,
}

impl Default for QpskPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl QpskPlugin {
    /// Create the plugin.
    pub fn new() -> Self {
        Self {
            info: PluginInfo {
                name: "QPSK".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Quadrature Phase-Shift Keying with Gray-mapped dibits".to_string(),
                author: "OpenPulse Contributors".to_string(),
                supported_modes: vec![
                    "QPSK125".to_string(),
                    "QPSK250".to_string(),
                    "QPSK500".to_string(),
                    "QPSK1000".to_string(),
                    "QPSK1000-HF".to_string(),
                    "QPSK500-RRC".to_string(),
                    "QPSK1000-RRC".to_string(),
                ],
                trait_version_required: "1.0".to_string(),
            },
        }
    }
}

impl ModulationPlugin for QpskPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        modulate::qpsk_modulate(data, config)
    }

    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError> {
        demodulate::qpsk_demodulate(samples, config)
    }

    fn modulate_iq(
        &self,
        data: &[u8],
        config: &ModulationConfig,
    ) -> Result<(Vec<f32>, Vec<f32>), ModemError> {
        modulate::qpsk_modulate_iq(data, config)
    }
}

/// Parse numeric baud rate from modes such as "QPSK250", "QPSK1000-HF", or "QPSK500-RRC".
pub(crate) fn parse_baud_rate(mode: &str) -> Result<f32, ModemError> {
    let base = mode.trim_end_matches("-HF").trim_end_matches("-RRC");
    let digits: String = base.chars().skip_while(|c| !c.is_ascii_digit()).collect();
    match digits.as_str() {
        "125" => Ok(125.0),
        "250" => Ok(250.0),
        "500" => Ok(500.0),
        "1000" => Ok(1000.0),
        _ => Err(ModemError::Configuration(format!(
            "unknown baud rate in mode '{mode}'"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modes() {
        assert!((parse_baud_rate("QPSK125").unwrap() - 125.0).abs() < 1e-6);
        assert!((parse_baud_rate("QPSK250").unwrap() - 250.0).abs() < 1e-6);
        assert!((parse_baud_rate("QPSK500").unwrap() - 500.0).abs() < 1e-6);
        assert!((parse_baud_rate("QPSK1000").unwrap() - 1000.0).abs() < 1e-6);
        assert!((parse_baud_rate("QPSK1000-HF").unwrap() - 1000.0).abs() < 1e-6);
        assert!(parse_baud_rate("QPSK").is_err());
    }

    #[test]
    fn qpsk1000_loopback() {
        use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
        let plugin = QpskPlugin::new();
        let cfg = ModulationConfig {
            mode: "QPSK1000".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"QPSK1000 test";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let recovered = plugin.demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }

    #[test]
    fn qpsk1000_hf_loopback() {
        use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
        let plugin = QpskPlugin::new();
        let cfg = ModulationConfig {
            mode: "QPSK1000-HF".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"QPSK1000-HF round-trip";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let recovered = plugin.demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }

    #[test]
    fn qpsk1000_hf_bandwidth_under_2700hz() {
        use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
        use std::f32::consts::PI;
        let plugin = QpskPlugin::new();
        let fc = 1500.0f32;
        let cfg = ModulationConfig {
            mode: "QPSK1000-HF".to_string(),
            center_frequency: fc,
            sample_rate: 8000,
            ..ModulationConfig::default()
        };
        let payload: Vec<u8> = (0..128u8).collect();
        let samples = plugin.modulate(&payload, &cfg).expect("modulate");
        let fs = 8000.0f32;
        let n = samples.len() as f32;

        let power_at = |freq: f32| -> f32 {
            let re: f32 = samples
                .iter()
                .enumerate()
                .map(|(k, &s)| s * (2.0 * PI * freq * k as f32 / fs).cos())
                .sum::<f32>()
                / n;
            let im: f32 = samples
                .iter()
                .enumerate()
                .map(|(k, &s)| s * (2.0 * PI * freq * k as f32 / fs).sin())
                .sum::<f32>()
                / n;
            re * re + im * im
        };

        let p_inband = power_at(fc);
        let p_edge = power_at(fc + 1350.0);
        // Edge (at the 2700 Hz HF boundary) must be at least 10 dB below in-band.
        assert!(
            p_edge < p_inband / 10.0,
            "edge power {p_edge:.6} should be < 1/10 of in-band {p_inband:.6}"
        );
    }

    #[test]
    fn qpsk500_rrc_loopback() {
        use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
        let plugin = QpskPlugin::new();
        let cfg = ModulationConfig {
            mode: "QPSK500-RRC".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"QPSK RRC loopback";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let recovered = plugin.demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }
}
