//! 4FSK modulation plugin for OpenPulse HPX ACK frames.
//!
//! # Waveform
//!
//! | Parameter      | Value                                      |
//! |----------------|--------------------------------------------|
//! | Tones          | 4 (at `fc ± 50 Hz` and `fc ± 150 Hz`)     |
//! | Tone spacing   | 100 Hz                                     |
//! | Default `fc`   | 1 050 Hz → tones 900 / 1 000 / 1 100 / 1 200 Hz |
//! | Symbol rate    | 100 baud                                   |
//! | Bits/symbol    | 2 (MSB-first within each byte)             |
//! | Frame size     | 5 bytes = 20 symbols = 200 ms @ 8 kHz      |
//! | Pulse shaping  | Hann window per symbol                     |
//!
//! # Supported mode
//!
//! | Mode string | Description |
//! |-------------|-------------|
//! | `FSK4-ACK`  | Fixed 5-byte ACK payload encoding          |

pub mod demodulate;
pub mod modulate;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin, PluginInfo};

/// Symbol rate for all FSK4 modes (baud).
pub(crate) const BAUD: f32 = 100.0;

// ── Fsk4Plugin ────────────────────────────────────────────────────────────────

/// 4FSK ACK frame modulation plugin.
pub struct Fsk4Plugin {
    info: PluginInfo,
}

impl Default for Fsk4Plugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Fsk4Plugin {
    /// Create the plugin.
    pub fn new() -> Self {
        Self {
            info: PluginInfo {
                name: "FSK4".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description:
                    "4-tone FSK for HPX ACK frames; decodable at lower SNR than data modulations"
                        .to_string(),
                author: "OpenPulse Contributors".to_string(),
                supported_modes: vec!["FSK4-ACK".to_string()],
                trait_version_required: "1.0".to_string(),
            },
        }
    }
}

impl ModulationPlugin for Fsk4Plugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        modulate::fsk4_modulate(data, config)
    }

    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError> {
        demodulate::fsk4_demodulate(samples, config)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_core::ack::{AckFrame, AckType};

    fn ack_config() -> ModulationConfig {
        ModulationConfig {
            mode: "FSK4-ACK".to_string(),
            sample_rate: 8000,
            center_frequency: 1050.0,
        }
    }

    #[test]
    fn fsk4_loopback_all_bytes() {
        let cfg = ack_config();
        let plugin = Fsk4Plugin::new();
        let original: Vec<u8> = (0..=255).collect();
        let samples = plugin.modulate(&original, &cfg).unwrap();
        let recovered = plugin.demodulate(&samples, &cfg).unwrap();
        assert_eq!(recovered, original);
    }

    #[test]
    fn ack_frame_fsk4_loopback() {
        let cfg = ack_config();
        let plugin = Fsk4Plugin::new();

        for t in [
            AckType::AckOk,
            AckType::AckUp,
            AckType::AckDown,
            AckType::Nack,
            AckType::Break,
            AckType::Req,
            AckType::Qrt,
            AckType::Abort,
        ] {
            let frame = AckFrame::new(t, "session-test");
            let payload = frame.encode();
            let samples = plugin.modulate(&payload, &cfg).unwrap();
            let decoded_bytes = plugin.demodulate(&samples, &cfg).unwrap();
            let decoded_frame = AckFrame::decode(decoded_bytes[..5].try_into().unwrap()).unwrap();
            assert_eq!(decoded_frame, frame, "ACK type {t:?} failed loopback");
        }
    }

    #[test]
    fn fsk4_output_length_is_correct() {
        let cfg = ack_config();
        let plugin = Fsk4Plugin::new();
        let data = [0u8; 5]; // 5 bytes = 20 symbols
        let samples = plugin.modulate(&data, &cfg).unwrap();
        let n = (8000.0f32 / 100.0).round() as usize; // 80 samples/symbol
        assert_eq!(samples.len(), 5 * 4 * n); // 1600 samples = 200 ms
    }
}
