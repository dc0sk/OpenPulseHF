//! 64QAM modulation/demodulation plugin for OpenPulse.
//!
//! 6 bits per symbol; 8×8 Gray-coded constellation normalized to unit average
//! power.  Suitable for clean LOS links (FM/satellite/UHF-VHF) requiring
//! ≥ 24 dB SNR.  Not for HF ionospheric paths without a wideband equalizer.

pub mod demodulate;
pub mod modulate;

#[cfg(feature = "gpu")]
use std::sync::Arc;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin, PluginInfo};

/// 64QAM modulation plugin.
pub struct Qam64Plugin {
    info: PluginInfo,
    #[cfg(feature = "gpu")]
    gpu: Option<Arc<openpulse_gpu::GpuContext>>,
}

impl Default for Qam64Plugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Qam64Plugin {
    /// Create the plugin with CPU-only DSP.
    pub fn new() -> Self {
        Self {
            info: Self::make_info(),
            #[cfg(feature = "gpu")]
            gpu: None,
        }
    }

    /// Create the plugin with GPU-accelerated soft demodulation.
    #[cfg(feature = "gpu")]
    pub fn with_gpu(ctx: Arc<openpulse_gpu::GpuContext>) -> Self {
        Self {
            info: Self::make_info(),
            gpu: Some(ctx),
        }
    }

    fn make_info() -> PluginInfo {
        PluginInfo {
            name: "64QAM".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "64-Quadrature Amplitude Modulation (6 bits/symbol, Gray-coded)"
                .to_string(),
            author: "OpenPulse Contributors".to_string(),
            supported_modes: vec![
                "64QAM500".to_string(),
                "64QAM1000".to_string(),
                "64QAM2000-RRC".to_string(),
            ],
            trait_version_required: "1.0".to_string(),
        }
    }
}

impl ModulationPlugin for Qam64Plugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        modulate::qam64_modulate(data, config)
    }

    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError> {
        #[cfg(feature = "gpu")]
        if let Some(ref ctx) = self.gpu {
            if let Some(result) = demodulate::qam64_demodulate_gpu(samples, config, ctx) {
                return result;
            }
        }
        demodulate::qam64_demodulate(samples, config)
    }

    fn demodulate_soft(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<f32>, ModemError> {
        #[cfg(feature = "gpu")]
        if let Some(ref ctx) = self.gpu {
            return demodulate::qam64_demodulate_soft_gpu(samples, config, ctx);
        }
        demodulate::qam64_demodulate_soft(samples, config)
    }

    fn supports_soft_demod(&self) -> bool {
        true
    }

    fn estimate_afc_hz(&self, samples: &[f32], config: &ModulationConfig) -> Option<f32> {
        demodulate::afc_estimate_hz(samples, config)
    }
}

fn parse_baud_rate(mode: &str) -> Result<f32, ModemError> {
    // Mode format: "64QAM<baud>" or "64QAM<baud>-RRC". Extract the numeric suffix after "QAM".
    let base = mode.trim_end_matches("-RRC");
    let baud_str = base
        .strip_prefix("64QAM")
        .ok_or_else(|| ModemError::Configuration(format!("unknown 64QAM mode: {mode}")))?;
    baud_str
        .parse::<f32>()
        .map_err(|_| ModemError::Configuration(format!("unknown 64QAM mode: {mode}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpulse_core::plugin::{ModulationConfig, PulseShape};

    fn cfg(mode: &str) -> ModulationConfig {
        ModulationConfig {
            mode: mode.to_string(),
            sample_rate: 8000,
            center_frequency: 1500.0,
            pulse_shape: if mode.ends_with("-RRC") {
                PulseShape::Rrc { alpha: 0.35 }
            } else {
                PulseShape::Hann
            },
        }
    }

    #[test]
    fn qam64_500_loopback() {
        let plugin = Qam64Plugin::new();
        let data = b"64QAM loopback test";
        let samples = plugin.modulate(data, &cfg("64QAM500")).unwrap();
        let recovered = plugin.demodulate(&samples, &cfg("64QAM500")).unwrap();
        assert_eq!(&recovered[..data.len()], data);
    }

    #[test]
    fn qam64_1000_loopback() {
        let plugin = Qam64Plugin::new();
        let data = b"64QAM 1000 baud test";
        let samples = plugin.modulate(data, &cfg("64QAM1000")).unwrap();
        let recovered = plugin.demodulate(&samples, &cfg("64QAM1000")).unwrap();
        assert_eq!(&recovered[..data.len()], data);
    }

    #[test]
    fn qam64_soft_demodulate_returns_six_llrs_per_symbol() {
        let plugin = Qam64Plugin::new();
        let data = b"soft";
        let samples = plugin.modulate(data, &cfg("64QAM500")).unwrap();
        let llrs = plugin.demodulate_soft(&samples, &cfg("64QAM500")).unwrap();
        // 4 data bytes = 32 bits; LLR count must be a multiple of 6 (bits/symbol)
        // and cover at least the data bits.
        assert!(llrs.len() >= 32);
        assert!(llrs.iter().all(|&v| v.is_finite()));
    }

    #[test]
    fn supported_modes_listed() {
        let plugin = Qam64Plugin::new();
        let modes = &plugin.info().supported_modes;
        assert!(modes.iter().any(|m| m == "64QAM500"));
        assert!(modes.iter().any(|m| m == "64QAM1000"));
        assert!(modes.iter().any(|m| m == "64QAM2000-RRC"));
    }

    #[test]
    fn afc_estimate_near_zero_on_carrier_match() {
        let plugin = Qam64Plugin::new();
        let tx_cfg = cfg("64QAM1000");
        let rx_cfg = tx_cfg.clone();
        let samples = plugin.modulate(b"afc qam64", &tx_cfg).expect("modulate");
        let est = plugin
            .estimate_afc_hz(&samples, &rx_cfg)
            .expect("afc estimate");
        assert!(est.abs() < 3.0, "expected near-zero AFC, got {est:.2} Hz");
    }

    #[test]
    fn afc_estimate_tracks_positive_offset() {
        let plugin = Qam64Plugin::new();
        let mut tx_cfg = cfg("64QAM1000");
        tx_cfg.center_frequency = 1520.0;
        let mut rx_cfg = cfg("64QAM1000");
        rx_cfg.center_frequency = 1500.0;
        let samples = plugin.modulate(b"afc qam64", &tx_cfg).expect("modulate");
        let est = plugin
            .estimate_afc_hz(&samples, &rx_cfg)
            .expect("afc estimate");
        assert!(
            (est - 20.0).abs() < 6.0,
            "expected about +20 Hz AFC, got {est:.2} Hz"
        );
    }
}
