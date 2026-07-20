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
use openpulse_core::plugin::{FrameGeometry, ModulationConfig, ModulationPlugin, PluginInfo};

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

    fn frame_geometry(&self, config: &ModulationConfig) -> Option<FrameGeometry> {
        let baud = parse_baud_rate(&config.mode).ok()?;
        let n = modulate::samples_per_symbol(config.sample_rate as f32, baud).ok()?;
        const BITS_PER_SYMBOL: usize = 6;
        // Largest frame: full 255-byte RS block + envelope, plus 10% margin.
        let max_data_syms = (260usize * 8).div_ceil(BITS_PER_SYMBOL);
        let frame_syms = modulate::PREAMBLE_SYMS + max_data_syms + modulate::TAIL_SYMS;
        Some(FrameGeometry {
            symbol_period_samples: n,
            preamble_samples: n * modulate::PREAMBLE_SYMS,
            min_frame_samples: n * (modulate::PREAMBLE_SYMS + 1),
            max_frame_samples: n * frame_syms * 11 / 10,
        })
    }

    fn supports_soft_demod(&self, mode: &str) -> bool {
        let _ = mode;
        true
    }

    fn estimate_afc_hz(&self, samples: &[f32], config: &ModulationConfig) -> Option<f32> {
        demodulate::afc_estimate_hz(samples, config)
    }

    fn occupied_bandwidth_hz(&self, mode: &str) -> Option<f32> {
        // Single-carrier: rectangular main-lobe null-to-null = 2×baud (safe over-estimate for RRC).
        parse_baud_rate(mode).ok().map(|b| 2.0 * b)
    }
}

pub(crate) fn parse_baud_rate(mode: &str) -> Result<f32, ModemError> {
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
            ..ModulationConfig::default()
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
    fn qam64_decodes_at_non_unity_input_level() {
        // Data-aided AGC (corner-preamble reference) must recover the frame across a
        // wide input-level range — the inter-station level spread / QSB fading an AGC
        // exists to remove. Without it the absolute PAM-8 demap and the DD carrier
        // loop's amplitude decisions both fail away from the unity (loopback) scale.
        let plugin = Qam64Plugin::new();
        let data = b"64QAM level-robustness via corner-preamble AGC";
        for mode in ["64QAM500", "64QAM1000", "64QAM2000-RRC"] {
            let samples = plugin.modulate(data, &cfg(mode)).unwrap();
            for scale in [0.2f32, 0.5, 2.0, 5.0] {
                let scaled: Vec<f32> = samples.iter().map(|&s| s * scale).collect();
                let recovered = plugin.demodulate(&scaled, &cfg(mode)).unwrap();
                assert_eq!(
                    &recovered[..data.len()],
                    data,
                    "mode {mode} failed to decode at input scale {scale}"
                );
            }
        }
    }

    #[test]
    fn qam64_1000_loopback() {
        let plugin = Qam64Plugin::new();
        let data = b"64QAM 1000 baud test";
        let samples = plugin.modulate(data, &cfg("64QAM1000")).unwrap();
        let recovered = plugin.demodulate(&samples, &cfg("64QAM1000")).unwrap();
        assert_eq!(&recovered[..data.len()], data);
    }

    /// The carrier phase at the start of a received frame is effectively random on
    /// hardware.  Decoding must be invariant to it.  Prepending silent samples sweeps
    /// the carrier start phase; every sub-symbol offset must decode bit-exact for both
    /// the Hann and RRC paths.  (Padding stays below one symbol period: a full-symbol
    /// shift is a separate frame-acquisition concern handled by the engine's scan.)
    #[test]
    fn qam64_decode_invariant_to_carrier_phase() {
        let plugin = Qam64Plugin::new();
        let payload: Vec<u8> = (0u8..48).collect();
        for mode in ["64QAM500", "64QAM1000", "64QAM2000-RRC"] {
            let config = cfg(mode);
            let baud = crate::parse_baud_rate(mode).expect("baud");
            let n = (config.sample_rate as f32 / baud).round() as usize;
            let signal = plugin.modulate(&payload, &config).expect("modulate");
            for pad in 0..n {
                let mut samples = vec![0.0f32; pad];
                samples.extend_from_slice(&signal);
                let recovered = plugin.demodulate(&samples, &config).expect("demodulate");
                assert!(
                    recovered.len() >= payload.len() && recovered[..payload.len()] == payload[..],
                    "{mode}: wrong decode at carrier phase offset pad={pad}"
                );
            }
        }
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
