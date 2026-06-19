//! Pilot-framed waveform plugin for OpenPulse.
//!
//! A modem waveform whose frames carry **known in-band pilot symbols** at a fixed
//! cadence, so the receiver recovers the carrier with a
//! [`openpulse_dsp::pilot_tracker::PilotTracker`] driven only by known symbols.
//! That data-aided loop is immune to the decision-directed cycle slips that limit
//! the existing preamble-only single-Costas modes on dense constellations and
//! through carrier offset — the convergent lesson from the qo100 / liquid-dsp /
//! gnuradio references.
//!
//! Layers:
//! - [`frame`] — the symbol-level pilot-framed QPSK codec ([`PilotFrame`]).
//! - [`modulate`] / [`demodulate`] — the passband audio chain (rectangular-pulse
//!   QPSK upconvert; downconvert + integrate-and-dump + preamble onset).
//! - [`PilotPlugin`] — the [`ModulationPlugin`] implementation.
//!
//! The POC mode is `PILOT-QPSK500` (rectangular pulse). Engine/profile
//! integration and dense rungs follow (see `docs/dev/reference-mining-plan.md`).

pub mod demodulate;
pub mod frame;
pub mod modulate;

pub use frame::PilotFrame;

use openpulse_core::error::ModemError;
use openpulse_core::plugin::{FrameGeometry, ModulationConfig, ModulationPlugin, PluginInfo};

/// Pilot-framed QPSK modulation plugin.
pub struct PilotPlugin {
    info: PluginInfo,
}

impl PilotPlugin {
    /// Create the plugin with its registered modes.
    pub fn new() -> Self {
        Self {
            info: PluginInfo {
                name: "PILOT".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "Pilot-framed QPSK (in-band pilot-aided carrier tracking)".to_string(),
                author: "OpenPulse".to_string(),
                supported_modes: vec![
                    "PILOT-QPSK500".to_string(),
                    "PILOT-8PSK500".to_string(),
                    "PILOT-16QAM500".to_string(),
                    "PILOT-32APSK500".to_string(),
                ],
                trait_version_required: "1.0".to_string(),
            },
        }
    }
}

impl Default for PilotPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ModulationPlugin for PilotPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        modulate::pilot_modulate(data, config)
    }

    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError> {
        demodulate::pilot_demodulate(samples, config)
    }

    fn demodulate_soft(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<f32>, ModemError> {
        demodulate::pilot_demodulate_soft(samples, config)
    }

    fn supports_soft_demod(&self) -> bool {
        true
    }

    fn estimate_afc_hz(&self, samples: &[f32], config: &ModulationConfig) -> Option<f32> {
        demodulate::pilot_estimate_afc_hz(samples, config)
    }

    fn frame_geometry(&self, config: &ModulationConfig) -> Option<FrameGeometry> {
        let sps = modulate::samples_per_symbol(config).ok()?;
        // Symbols per byte scales with the constellation order: denser modes pack
        // a frame into FEWER, shorter symbols. Sizing `min_frame_samples` from
        // QPSK's 4 sym/byte over-estimates the floor for dense modes, so the
        // engine would wait for a slice longer than the actual (shorter) 32APSK/
        // 16QAM frame and never decode it. Use the mode's real bits/symbol.
        let bits = if config.mode.eq_ignore_ascii_case("PILOT-32APSK500") {
            5
        } else {
            modulate::bits_per_sc_for_mode(&config.mode).ok()?
        };
        let preamble_syms = PilotFrame::new().preamble_len();
        let max_data_syms = (255usize * 8).div_ceil(bits); // 255-byte RS block
        let min_data_syms = (42usize * 8).div_ceil(bits); // minimal HPX frame
        let max_syms = preamble_syms + max_data_syms + max_data_syms / 15 + 8;
        let min_syms = preamble_syms + min_data_syms;
        Some(FrameGeometry {
            symbol_period_samples: sps,
            preamble_samples: preamble_syms * sps,
            min_frame_samples: min_syms * sps,
            max_frame_samples: max_syms * sps,
        })
    }
}
