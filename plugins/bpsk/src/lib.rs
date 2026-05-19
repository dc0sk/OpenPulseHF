//! BPSK modulation/demodulation plugin for OpenPulse.
//!
//! # Supported modes
//!
//! | Mode string | Baud rate | Notes |
//! |-------------|-----------|-------|
//! | `BPSK31`    |  31.25    | Narrow-band HF mode (≈ 31 Hz passband) |
//! | `BPSK63`    |  62.5     | Twice the throughput of BPSK31 |
//! | `BPSK100`   | 100       | Convenient for testing |
//! | `BPSK250`   | 250       | Wide-band / VHF |
//!
//! # Wire encoding
//!
//! ```text
//! ┌────────────────┬────────────────────┬──────────┐
//! │  preamble      │  data symbols      │  tail    │
//! │  32 symbols    │  8 × N symbols     │ 8 syms   │
//! └────────────────┴────────────────────┴──────────┘
//! ```
//!
//! Each bit is NRZI-encoded ("1" = phase flip, "0" = keep phase) and
//! pulse-shaped with a 50% overlapping half-Hann crossfade to minimise
//! occupied bandwidth; residual ISI is kept below the decision threshold
//! by the matched half-Hann filter in the demodulator.

pub mod demodulate;
pub mod modulate;

#[cfg(feature = "gpu")]
use std::sync::Arc;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin, PluginInfo};

// ── BpskPlugin ────────────────────────────────────────────────────────────────

/// BPSK modulation plugin.
pub struct BpskPlugin {
    info: PluginInfo,
    #[cfg(feature = "gpu")]
    gpu: Option<Arc<openpulse_gpu::GpuContext>>,
}

impl Default for BpskPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl BpskPlugin {
    /// Create the plugin with CPU-only DSP.
    pub fn new() -> Self {
        Self {
            info: Self::make_info(),
            #[cfg(feature = "gpu")]
            gpu: None,
        }
    }

    /// Create the plugin with GPU-accelerated DSP.
    ///
    /// Heavy modulate/demodulate calls are dispatched to the GPU; all other
    /// operations fall through to the CPU path.
    #[cfg(feature = "gpu")]
    pub fn with_gpu(ctx: Arc<openpulse_gpu::GpuContext>) -> Self {
        Self {
            info: Self::make_info(),
            gpu: Some(ctx),
        }
    }

    fn make_info() -> PluginInfo {
        PluginInfo {
            name: "BPSK".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description:
                "Binary Phase-Shift Keying with NRZI encoding and overlapping half-Hann pulse shaping"
                    .to_string(),
            author: "OpenPulse Contributors".to_string(),
            supported_modes: vec![
                "BPSK31".to_string(),
                "BPSK63".to_string(),
                "BPSK100".to_string(),
                "BPSK250".to_string(),
                "BPSK250-RRC".to_string(),
            ],
            trait_version_required: "1.0".to_string(),
        }
    }
}

impl ModulationPlugin for BpskPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        #[cfg(feature = "gpu")]
        if let Some(ref ctx) = self.gpu {
            return modulate::bpsk_modulate_with_gpu(data, config, ctx);
        }
        modulate::bpsk_modulate(data, config)
    }

    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError> {
        #[cfg(feature = "gpu")]
        if let Some(ref ctx) = self.gpu {
            return demodulate::bpsk_demodulate_with_gpu(samples, config, ctx);
        }
        demodulate::bpsk_demodulate(samples, config)
    }

    fn demodulate_soft(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<f32>, ModemError> {
        demodulate::bpsk_demodulate_soft(samples, config)
    }

    fn estimate_afc_hz(&self, samples: &[f32], config: &ModulationConfig) -> Option<f32> {
        demodulate::afc_estimate_hz(samples, config)
    }

    fn modulate_iq(
        &self,
        data: &[u8],
        config: &ModulationConfig,
    ) -> Result<(Vec<f32>, Vec<f32>), ModemError> {
        modulate::bpsk_modulate_iq(data, config)
    }
}

// ── Helper: parse baud rate from mode string ──────────────────────────────────

/// Parse the numeric baud rate from a mode string such as `"BPSK31"` or `"BPSK250-RRC"`.
pub(crate) fn parse_baud_rate(mode: &str) -> Result<f32, ModemError> {
    // Strip trailing suffixes (-RRC) then parse leading digits after "BPSK".
    let base = mode.trim_end_matches("-RRC");
    let digits: String = base.chars().skip_while(|c| !c.is_ascii_digit()).collect();
    match digits.as_str() {
        "31" => Ok(31.25),
        "63" => Ok(62.5),
        other => other
            .parse::<f32>()
            .map_err(|_| ModemError::Configuration(format!("unknown baud rate in mode '{mode}'"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modes() {
        assert!((parse_baud_rate("BPSK31").unwrap() - 31.25).abs() < 1e-4);
        assert!((parse_baud_rate("BPSK63").unwrap() - 62.5).abs() < 1e-4);
        assert!((parse_baud_rate("BPSK100").unwrap() - 100.0).abs() < 1e-4);
        assert!((parse_baud_rate("BPSK250").unwrap() - 250.0).abs() < 1e-4);
        assert!(parse_baud_rate("BPSK").is_err());
    }

    #[test]
    fn bpsk250_rrc_loopback() {
        let plugin = BpskPlugin::new();
        let cfg = ModulationConfig {
            mode: "BPSK250-RRC".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"BPSK RRC loopback";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let recovered = plugin.demodulate(&samples, &cfg).expect("demodulate");
        assert_eq!(&recovered[..payload.len()], payload);
    }

    /// `demodulate_soft` for BPSK250-RRC must return real matched-filter LLRs, not hard ±1.0.
    ///
    /// Hard ±1.0 fallback produces values that are EXACTLY 1.0f32 or -1.0f32.
    /// Real matched-filter soft LLRs will deviate from exact ±1 due to signal amplitude scaling.
    #[test]
    fn bpsk250_rrc_soft_demod_returns_real_llrs() {
        let plugin = BpskPlugin::new();
        let cfg = ModulationConfig {
            mode: "BPSK250-RRC".to_string(),
            ..ModulationConfig::default()
        };
        let payload = b"soft llr test";
        let samples = plugin.modulate(payload, &cfg).expect("modulate");
        let llrs = plugin
            .demodulate_soft(&samples, &cfg)
            .expect("demodulate_soft");

        assert!(!llrs.is_empty(), "LLRs must not be empty");
        assert!(
            llrs.iter().all(|x| x.is_finite()),
            "demodulate_soft must not return NaN or Inf"
        );
        let all_hard = llrs.iter().all(|&x| x == 1.0f32 || x == -1.0f32);
        assert!(
            !all_hard,
            "demodulate_soft must return real soft LLRs, not hard ±1.0 decisions"
        );
    }
}
