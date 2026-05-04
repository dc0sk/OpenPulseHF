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
//! pulse-shaped with a raised-cosine (Hann) window per symbol to minimise
//! occupied bandwidth.

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
                "Binary Phase-Shift Keying with NRZI encoding and raised-cosine pulse shaping"
                    .to_string(),
            author: "OpenPulse Contributors".to_string(),
            supported_modes: vec![
                "BPSK31".to_string(),
                "BPSK63".to_string(),
                "BPSK100".to_string(),
                "BPSK250".to_string(),
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

    fn estimate_afc_hz(&self, samples: &[f32], config: &ModulationConfig) -> Option<f32> {
        demodulate::afc_estimate_hz(samples, config)
    }
}

// ── Helper: parse baud rate from mode string ──────────────────────────────────

/// Parse the numeric baud rate from a mode string such as `"BPSK31"`.
pub(crate) fn parse_baud_rate(mode: &str) -> Result<f32, ModemError> {
    // Strip any leading non-digit characters ("BPSK") then parse the number.
    let digits: String = mode.chars().skip_while(|c| !c.is_ascii_digit()).collect();
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
}
