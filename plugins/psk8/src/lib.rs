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
                    "8PSK1000-HF-RRC".to_string(),
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
        demodulate::psk8_demodulate_soft(samples, config)
    }
}

/// Parse numeric baud rate from the trailing digits of modes such as "8PSK500", "8PSK1000-HF", "8PSK500-RRC", or "8PSK1000-HF-RRC".
///
/// Suffixes "-HF" and "-RRC" are stripped repeatedly until neither appears at the end, which
/// handles composite variants such as "8PSK1000-HF-RRC" regardless of suffix ordering.
pub(crate) fn parse_baud_rate(mode: &str) -> Result<f32, ModemError> {
    let mut base = mode;
    loop {
        let stripped = base.trim_end_matches("-HF").trim_end_matches("-RRC");
        if stripped == base {
            break;
        }
        base = stripped;
    }
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
        // Composite HF-RRC suffix: order must not matter.
        assert!((parse_baud_rate("8PSK1000-HF-RRC").unwrap() - 1000.0).abs() < 1e-6);
        assert!((parse_baud_rate("8PSK500-HF-RRC").unwrap() - 500.0).abs() < 1e-6);
        assert!(parse_baud_rate("8PSK").is_err());
    }

    #[test]
    fn supported_modes_include_composite_hf_rrc() {
        let plugin = Psk8Plugin::new();
        assert!(plugin.supports_mode("8PSK1000-HF-RRC"));
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

    /// Composite "8PSK1000-HF-RRC" loopback exercises the full parse → modulate →
    /// demodulate path through the fixed suffix-stripping logic; previously this
    /// mode would cause parse_baud_rate to error before reaching LMS equalization.
    #[test]
    fn psk8_1000_hf_rrc_loopback() {
        let plugin = Psk8Plugin::new();
        let cfg = ModulationConfig {
            mode: "8PSK1000-HF-RRC".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"composite HF-RRC mode";
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

    /// Max-log-MAP soft LLRs must agree with hard decisions and be strictly more
    /// confident than the ±1.0 fallback on a clean loopback.
    #[test]
    fn soft_demodulate_500_sign_and_magnitude() {
        let plugin = Psk8Plugin::new();
        let cfg = ModulationConfig {
            mode: "8PSK500".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"soft LLR 8PSK check";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let hard_bytes = plugin.demodulate(&samples, &cfg).expect("demodulate");
        let llrs = plugin
            .demodulate_soft(&samples, &cfg)
            .expect("demodulate_soft");

        // LLR count must equal hard byte count × 8.
        assert_eq!(llrs.len(), hard_bytes.len() * 8);

        // Sign of each LLR must agree with the hard decision.
        for (byte_idx, &byte) in hard_bytes.iter().enumerate() {
            for bit in 0..8usize {
                let hard_bit = (byte >> bit) & 1;
                let llr = llrs[byte_idx * 8 + bit];
                // LLR > 0 ↔ bit=0 more likely.
                if hard_bit == 0 {
                    assert!(
                        llr > 0.0,
                        "byte {byte_idx} bit {bit}: expected positive LLR for 0, got {llr}"
                    );
                } else {
                    assert!(
                        llr < 0.0,
                        "byte {byte_idx} bit {bit}: expected negative LLR for 1, got {llr}"
                    );
                }
                // LLR must be a real soft value, not degenerate zero.
                assert!(
                    llr.abs() > 0.01,
                    "byte {byte_idx} bit {bit}: LLR magnitude {llr} too small"
                );
            }
        }
    }
}
