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
use openpulse_core::plugin::{
    FrameGeometry, ModulationConfig, ModulationPlugin, PluginInfo, PreambleTemplate,
};

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
            trait_version_required: "3.0".to_string(),
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

    fn frame_geometry(&self, config: &ModulationConfig) -> Option<FrameGeometry> {
        let baud = parse_baud_rate(&config.mode).ok()?;
        let n = modulate::samples_per_symbol(config.sample_rate as f32, baud).ok()?;
        const BITS_PER_SYMBOL: usize = 1;
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

    /// Absolute additive SNR (dB) for the rate decision — see `demodulate::estimate_snr_db`.
    ///
    /// Without this the engine fell back to the waveform-blind M2M4 moment estimator, which reads a
    /// flat ≈ -6.6 dB on a fade regardless of the true SNR and drove the rate controller to the
    /// bottom rung on frames that decoded (issue #934). `hpx_hf`'s SL2-SL5 are all BPSK.
    fn estimate_snr_db(&self, samples: &[f32], config: &ModulationConfig) -> Option<f32> {
        demodulate::estimate_snr_db(samples, config)
    }

    fn supports_soft_demod(&self, mode: &str) -> bool {
        let _ = mode;
        true
    }

    fn estimate_afc_hz(&self, samples: &[f32], config: &ModulationConfig) -> Option<f32> {
        demodulate::afc_estimate_hz(samples, config)
    }

    /// Published only for modes whose ρ constants have actually been derived.
    ///
    /// `PREAMBLE_RHO_THRESHOLD` is a **BPSK250** number — its doc derives it from BPSK250's own
    /// decode cliff and BPSK250's own recorded idle ceiling. Handing it to BPSK31 would be
    /// borrowing a threshold across templates, which is the practice #1053 was withdrawn for and
    /// which `PreambleTemplate`'s own doc calls unrepresentable by design: ρ is normalised, so its
    /// noise floor is set by template length, and BPSK31's template is eight times longer.
    ///
    /// This guard used to be provided *accidentally* by `MAX_PREAMBLE_CORRELATION_SAMPLES`: the
    /// slow rungs' templates exceeded the cap, so the engine discarded them before the borrowed
    /// constant could be used. Phase 0 of #1062 decimates oversized templates instead of refusing
    /// them, which removes that accident — so the constraint has to be stated where it belongs,
    /// here, rather than depending on a cost limit to enforce a correctness property.
    ///
    /// To add a mode: derive its own threshold from its own noise column (across receive
    /// bandwidths) and its own decode column (on the channel that rung exists for), then list it.
    fn preamble_template(&self, config: &ModulationConfig) -> Option<PreambleTemplate> {
        // The constants below were derived on BPSK250 and are named as such, so the engine rejects
        // them for any other mode rather than silently inheriting. Two of them are mode-specific,
        // not one: besides the threshold, PREAMBLE_RHO_GRID_HZ = 20 is safe only while the grid
        // stays under half the preamble's line spacing (baud/2). At BPSK31 that spacing is 15.6 Hz
        // and at BPSK63 31.25 Hz, so a +/-20 Hz grid can rotate SOME line onto ANY tone frequency
        // and the veto would corroborate a steady tone wherever it sits.
        //
        // To add a mode: derive its own threshold (noise column across receive bandwidths, decode
        // column on the channel that rung exists for) AND its own grid (bounded above by baud/4,
        // below by the settle residual, measured at <= 0.3 Hz), then name it here.
        const DERIVED_FOR: &str = "BPSK250";
        if config.mode != DERIVED_FOR {
            return None;
        }
        // Mechanical form of the bound the comment above states, so activating a mode with an
        // unsafe grid fails here instead of shipping. The preamble's lines sit at odd multiples of
        // baud/4, so adjacent lines are baud/2 apart; a grid reaching half that spacing can rotate
        // SOME line onto ANY frequency. Measured at the deployed geometry (grid centred where a
        // locked settle puts it): BPSK31 rho 0.661 and BPSK63 0.701 against a 0.40 threshold, where
        // BPSK250 sits at 0.042 — the settle's rescue is a coincidence of magnitudes that stops
        // holding once baud/4 falls under the grid.
        //
        // Scoped to this plugin deliberately: baud/4 is a property of THIS preamble's period-4 line
        // structure and would be wrong for a PN successor.
        let baud = crate::parse_baud_rate(&config.mode).ok()?;
        if modulate::PREAMBLE_RHO_GRID_HZ >= baud / 4.0 {
            // Loud where someone would be editing this, safe-fail where it ships. Publishing
            // nothing costs the energy-only settle; publishing this costs a veto that corroborates
            // any steady tone, which is worse than having none.
            debug_assert!(
                false,
                "{} would publish a preamble template whose grid (±{} Hz) reaches its first                  spectral line at baud/4 = {:.1} Hz — a steady tone at any frequency would                  corroborate. Derive a narrower grid for this mode before listing it.",
                config.mode,
                modulate::PREAMBLE_RHO_GRID_HZ,
                baud / 4.0
            );
            return None;
        }
        let samples = modulate::bpsk_preamble_template(config).ok()?;
        Some(
            PreambleTemplate::new(
                DERIVED_FOR,
                samples,
                modulate::PREAMBLE_RHO_THRESHOLD,
                modulate::PREAMBLE_RHO_GRID_HZ,
            )
            // VERIFIES: REQ-RX-03
            .with_delivered_frame_bound(modulate::DELIVERED_FRAME_RHO_BOUND),
        )
    }

    fn occupied_bandwidth_hz(&self, mode: &str) -> Option<f32> {
        // Rectangular main-lobe null-to-null = 2×baud; a safe over-estimate for the RRC path.
        parse_baud_rate(mode).ok().map(|b| 2.0 * b)
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

    /// Crossfade-ISI cancellation must lower the BPSK AWGN bit-error rate: the uncancelled `+β` bias in
    /// the differential dot product costs several dB of flip-bit margin. Deterministic (fixed-seed noise).
    #[test]
    fn crossfade_cancellation_lowers_awgn_ber() {
        // Box-Muller Gaussian from a deterministic LCG — no rng dep, reproducible across runs.
        fn add_noise(samples: &mut [f32], sigma: f32, mut seed: u64) {
            let mut next = || {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((seed >> 11) as f64 / (1u64 << 53) as f64) as f32
            };
            for s in samples.iter_mut() {
                let u1 = next().max(1e-7);
                let u2 = next();
                let g = (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos();
                *s += sigma * g;
            }
        }

        let plugin = BpskPlugin::new();
        let cfg = ModulationConfig {
            mode: "BPSK250".to_string(),
            ..ModulationConfig::default()
        };
        // A long pseudo-random payload so the BER is statistically stable.
        let payload: Vec<u8> = (0..180u32)
            .map(|i| (i.wrapping_mul(37).wrapping_add(11) & 0xff) as u8)
            .collect();
        let clean = plugin.modulate(&payload, &cfg).expect("modulate");

        // A noise level in the crossfade-margin-sensitive regime: high enough that the ISI bias tips
        // bits, low enough that the cancelled path recovers the frame cleanly.
        let mut noisy = clean.clone();
        add_noise(&mut noisy, 0.9, 0x1234_5678);
        let recovered = plugin.demodulate(&noisy, &cfg).expect("demodulate");

        let n = payload.len().min(recovered.len());
        let bit_errors: u32 = (0..n)
            .map(|k| (payload[k] ^ recovered[k]).count_ones())
            .sum();
        let total_bits = (n * 8) as f32;
        let ber = bit_errors as f32 / total_bits;
        // With cancellation this holds comfortably; with the uncancelled +β bias the BER is materially
        // worse at this noise level (measured A/B during development).
        assert!(
            ber < 0.02,
            "BPSK AWGN BER {ber:.4} too high — crossfade ISI cancellation regressed?"
        );
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
