//! 8PSK modulation/demodulation plugin for OpenPulse.

pub mod demodulate;
pub mod modulate;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin, PluginInfo};

/// 8PSK modulation plugin.
pub struct Psk8Plugin {
    info: PluginInfo,
}

impl Default for Psk8Plugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Psk8Plugin {
    /// Create the plugin.
    pub fn new() -> Self {
        Self {
            info: PluginInfo {
                name: "8PSK".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "8-Phase-Shift Keying with Gray-mapped tribits".to_string(),
                author: "OpenPulse Contributors".to_string(),
                supported_modes: vec!["8PSK500".to_string(), "8PSK1000".to_string()],
                trait_version_required: "1.0".to_string(),
            },
        }
    }
}

impl ModulationPlugin for Psk8Plugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        modulate::psk8_modulate(data, config)
    }

    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError> {
        demodulate::psk8_demodulate(samples, config)
    }
}

/// Parse numeric baud rate from the trailing digits of modes such as "8PSK500".
pub(crate) fn parse_baud_rate(mode: &str) -> Result<f32, ModemError> {
    let trailing: String = mode
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    match trailing.as_str() {
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
    use openpulse_core::plugin::ModulationPlugin;

    #[test]
    fn parse_modes() {
        assert!((parse_baud_rate("8PSK500").unwrap() - 500.0).abs() < 1e-6);
        assert!((parse_baud_rate("8PSK1000").unwrap() - 1000.0).abs() < 1e-6);
        assert!(parse_baud_rate("8PSK").is_err());
    }

    #[test]
    fn psk8_500_loopback() {
        let plugin = Psk8Plugin::new();
        let cfg = ModulationConfig {
            mode: "8PSK500".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"8PSK test payload";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let recovered = plugin.demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }

    #[test]
    fn psk8_1000_loopback() {
        let plugin = Psk8Plugin::new();
        let cfg = ModulationConfig {
            mode: "8PSK1000".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"8PSK1000 hi";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let recovered = plugin.demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }
}
