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
                supported_modes: vec![
                    "8PSK500".to_string(),
                    "8PSK1000".to_string(),
                    "8PSK1000-HF".to_string(),
                    "8PSK500-RRC".to_string(),
                    "8PSK1000-RRC".to_string(),
                    // UHF/VHF — 12.5 kHz narrowband (8 kHz audio, 2000 baud, ~2700 Hz BW)
                    "8PSK2000".to_string(),
                    "8PSK2000-RRC".to_string(),
                    // UHF/VHF — 12.5 kHz HD (requires 48 kHz audio, 9600 baud, ~13 kHz BW)
                    "8PSK9600".to_string(),
                    "8PSK9600-RRC".to_string(),
                ],
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

    fn demodulate_soft(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<f32>, ModemError> {
        // 8PSK max-log-MAP soft demapping (~1 dB gain) is not yet implemented.
        // Falls back to hard ±1.0 pseudo-LLRs from the trait default.
        let bytes = self.demodulate(samples, config)?;
        Ok(bytes
            .iter()
            .flat_map(|&b| (0..8u8).map(move |i| if (b >> i) & 1 == 0 { 1.0f32 } else { -1.0f32 }))
            .collect())
    }
}

/// Parse numeric baud rate from the trailing digits of modes such as "8PSK500", "8PSK1000-HF", or "8PSK500-RRC".
pub(crate) fn parse_baud_rate(mode: &str) -> Result<f32, ModemError> {
    let base = mode.trim_end_matches("-HF").trim_end_matches("-RRC");
    let trailing: String = base
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
        "2000" => Ok(2000.0),
        "9600" => Ok(9600.0),
        _ => Err(ModemError::Configuration(format!(
            "unknown baud rate in mode '{mode}'"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_core::plugin::{ModulationPlugin, PulseShape};

    #[test]
    fn parse_modes() {
        assert!((parse_baud_rate("8PSK500").unwrap() - 500.0).abs() < 1e-6);
        assert!((parse_baud_rate("8PSK1000").unwrap() - 1000.0).abs() < 1e-6);
        assert!((parse_baud_rate("8PSK1000-HF").unwrap() - 1000.0).abs() < 1e-6);
        assert!((parse_baud_rate("8PSK2000").unwrap() - 2000.0).abs() < 1e-6);
        assert!((parse_baud_rate("8PSK9600").unwrap() - 9600.0).abs() < 1e-6);
        assert!((parse_baud_rate("8PSK9600-RRC").unwrap() - 9600.0).abs() < 1e-6);
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

    #[test]
    fn psk8_1000_hf_loopback() {
        let plugin = Psk8Plugin::new();
        let cfg = ModulationConfig {
            mode: "8PSK1000-HF".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"8PSK1000-HF round-trip";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let recovered = plugin.demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }

    #[test]
    fn psk8_500_rrc_loopback() {
        let plugin = Psk8Plugin::new();
        let cfg = ModulationConfig {
            mode: "8PSK500-RRC".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"8PSK RRC loopback";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let recovered = plugin.demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }

    #[test]
    fn psk8_1000_rrc_loopback() {
        let plugin = Psk8Plugin::new();
        let cfg = ModulationConfig {
            mode: "8PSK1000-RRC".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"8PSK1000 RRC loopback";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let recovered = plugin.demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }

    /// 8PSK2000 clean loopback at 8 kHz (4 samples/symbol).
    #[test]
    fn psk8_2000_loopback() {
        let plugin = Psk8Plugin::new();
        let cfg = ModulationConfig {
            mode: "8PSK2000".to_string(),
            // Hann ISI at n=4 is too large for 8PSK's 22.5° margins; CosineOverlap
            // zeros at boundaries, eliminating ISI.  fc must be an integer multiple
            // of baud (2000 Hz → 1 cycle/symbol) for perfect I/Q orthogonality at n=4.
            pulse_shape: PulseShape::CosineOverlap,
            center_frequency: 2000.0,
            ..ModulationConfig::default()
        };
        let payload = b"8PSK2000 VHF narrowband";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let recovered = plugin.demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }

    /// 8PSK2000-RRC clean loopback at 8 kHz with Gardner + Costas PLL.
    #[test]
    fn psk8_2000_rrc_loopback() {
        let plugin = Psk8Plugin::new();
        let cfg = ModulationConfig {
            mode: "8PSK2000-RRC".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"8PSK2000-RRC 12.5 kHz PMR";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let recovered = plugin.demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }

    /// 8PSK9600 clean loopback at 48 kHz (5 samples/symbol, ~13 kHz BW).
    #[test]
    fn psk8_9600_loopback_48k() {
        let plugin = Psk8Plugin::new();
        let cfg = ModulationConfig {
            mode: "8PSK9600".to_string(),
            sample_rate: 48000,
            center_frequency: 12000.0,
            ..ModulationConfig::default()
        };
        let payload = b"8PSK9600 12.5 kHz HD";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let recovered = plugin.demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }

    /// 8PSK9600-RRC loopback at 48 kHz with Gardner + Costas PLL.
    #[test]
    fn psk8_9600_rrc_loopback_48k() {
        let plugin = Psk8Plugin::new();
        let cfg = ModulationConfig {
            mode: "8PSK9600-RRC".to_string(),
            sample_rate: 48000,
            center_frequency: 12000.0,
            ..ModulationConfig::default()
        };
        let payload = b"8PSK9600-RRC fills 12.5 kHz";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let recovered = plugin.demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }
}
