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
}

/// Parse numeric baud rate from modes such as "QPSK250".
pub(crate) fn parse_baud_rate(mode: &str) -> Result<f32, ModemError> {
    let digits: String = mode.chars().skip_while(|c| !c.is_ascii_digit()).collect();
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
}
