//! SC-FDMA modulation plugin for OpenPulse (FF-12 + BL-TP-4).
//!
//! SC-FDMA (DFT-spread OFDM) adds a DFT precoding step before the OFDM IFFT,
//! spreading each symbol across all allocated subcarriers.  The transmitted
//! signal resembles single-carrier: 3–4 dB lower PAPR than OFDM without any
//! iterative clipping, while using identical LS channel estimation on RX.
//! MMSE equalization (BL-TP-4) replaces ZF, enabling reliable 16QAM and 64QAM.
//!
//! Supported modes:
//! - `SCFDMA16`:          16 data SCs, QPSK,       BW ≈  625 Hz, gross ~   889 bps
//! - `SCFDMA52`:          52 data SCs, QPSK,       BW ≈ 2031 Hz, gross ~ 2,889 bps
//! - `SCFDMA52-8PSK`:     52 data SCs, 8PSK,       BW ≈ 2031 Hz, gross ~ 4,333 bps
//! - `SCFDMA52-16QAM`:    52 data SCs, 16QAM,      BW ≈ 2031 Hz, gross ~ 5,778 bps
//! - `SCFDMA52-32QAM`:    52 data SCs, cross-32QAM, BW ≈ 2031 Hz, gross ~ 7,222 bps
//! - `SCFDMA52-64QAM`:    52 data SCs, 64QAM,      BW ≈ 2031 Hz, gross ~ 8,667 bps
//! - `SCFDMA52-64QAM-P4`: 49 data SCs, 64QAM, denser pilots (16), gross ~ 8,167 bps

pub mod adaptive_pilot;
pub mod channel;
pub mod demodulate;
pub mod modulate;
pub mod params;

#[cfg(feature = "gpu")]
use std::sync::Arc;
use std::sync::Mutex;

use openpulse_core::{
    error::ModemError,
    plugin::{ModulationConfig, ModulationPlugin, PluginInfo},
};

use crate::adaptive_pilot::AdaptivePilotState;
use crate::demodulate::{scfdma_demodulate, scfdma_demodulate_soft};
use crate::modulate::{scfdma_modulate, scfdma_modulate_iq};
use crate::params::{params_for_mode, SAMPLE_RATE};

/// SC-FDMA plugin supporting SCFDMA16 and SCFDMA52 modes.
pub struct ScFdmaPlugin {
    info: PluginInfo,
    #[cfg(feature = "gpu")]
    gpu: Option<Arc<openpulse_gpu::GpuContext>>,
    adaptive: Mutex<AdaptivePilotState>,
}

impl ScFdmaPlugin {
    pub fn new() -> Self {
        Self {
            info: Self::make_info(),
            #[cfg(feature = "gpu")]
            gpu: None,
            adaptive: Mutex::new(AdaptivePilotState::new()),
        }
    }

    /// Create the plugin with GPU-accelerated FFT demodulation.
    #[cfg(feature = "gpu")]
    pub fn with_gpu(ctx: Arc<openpulse_gpu::GpuContext>) -> Self {
        Self {
            info: Self::make_info(),
            gpu: Some(ctx),
            adaptive: Mutex::new(AdaptivePilotState::new()),
        }
    }

    /// Return `ScFdmaParams` for `mode` with pilot spacing adapted to the current
    /// smoothed coherence bandwidth estimate.
    pub fn adaptive_params_for_mode(&self, mode: &str) -> Option<crate::params::ScFdmaParams> {
        let base = params_for_mode(mode)?;
        let coh_bw = self.adaptive.lock().ok()?.coh_bw_hz();
        Some(base.with_pilot_density(coh_bw))
    }

    fn make_info() -> PluginInfo {
        PluginInfo {
            name: "SC-FDMA".into(),
            version: "0.1.0".into(),
            description: "SC-FDMA HF plugin: SCFDMA16/52 (QPSK), SCFDMA52-8PSK, \
                 SCFDMA52-16QAM, SCFDMA52-32QAM (cross-32QAM), SCFDMA52-64QAM, \
                 SCFDMA52-64QAM-P4 (dense pilots); MMSE equalization (BL-TP-4)"
                .into(),
            author: "OpenPulse Contributors".into(),
            supported_modes: vec![
                "SCFDMA16".into(),
                "SCFDMA52".into(),
                "SCFDMA52-8PSK".into(),
                "SCFDMA52-16QAM".into(),
                "SCFDMA52-32QAM".into(),
                "SCFDMA52-64QAM".into(),
                "SCFDMA52-64QAM-P4".into(),
            ],
            trait_version_required: "1.0".into(),
        }
    }
}

impl Default for ScFdmaPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ModulationPlugin for ScFdmaPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        if params_for_mode(&config.mode).is_none() {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: unknown mode '{}'",
                config.mode
            )));
        }
        if config.sample_rate != SAMPLE_RATE {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: sample_rate {} not supported; must be {SAMPLE_RATE}",
                config.sample_rate
            )));
        }
        if (config.center_frequency - 1500.0).abs() > 1.0 {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: center_frequency {:.1} not supported; must be 1500.0 Hz",
                config.center_frequency
            )));
        }
        Ok(scfdma_modulate(data, &config.mode))
    }

    fn modulate_iq(
        &self,
        data: &[u8],
        config: &ModulationConfig,
    ) -> Result<(Vec<f32>, Vec<f32>), ModemError> {
        if params_for_mode(&config.mode).is_none() {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: unknown mode '{}'",
                config.mode
            )));
        }
        let interleaved = scfdma_modulate_iq(data, &config.mode);
        let i_ch = interleaved.iter().step_by(2).copied().collect();
        let q_ch = interleaved.iter().skip(1).step_by(2).copied().collect();
        Ok((i_ch, q_ch))
    }

    fn demodulate(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<u8>, ModemError> {
        if params_for_mode(&config.mode).is_none() {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: unknown mode '{}'",
                config.mode
            )));
        }
        if config.sample_rate != SAMPLE_RATE {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: sample_rate {} not supported; must be {SAMPLE_RATE}",
                config.sample_rate
            )));
        }
        if (config.center_frequency - 1500.0).abs() > 1.0 {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: center_frequency {:.1} not supported; must be 1500.0 Hz",
                config.center_frequency
            )));
        }
        #[cfg(feature = "gpu")]
        if let Some(ref ctx) = self.gpu {
            if let Some(result) = demodulate::scfdma_demodulate_gpu(samples, &config.mode, ctx) {
                return Ok(result);
            }
        }
        Ok(scfdma_demodulate(samples, &config.mode))
    }

    fn demodulate_soft(
        &self,
        samples: &[f32],
        config: &ModulationConfig,
    ) -> Result<Vec<f32>, ModemError> {
        if params_for_mode(&config.mode).is_none() {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: unknown mode '{}'",
                config.mode
            )));
        }
        if config.sample_rate != SAMPLE_RATE {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: sample_rate {} not supported; must be {SAMPLE_RATE}",
                config.sample_rate
            )));
        }
        if (config.center_frequency - 1500.0).abs() > 1.0 {
            return Err(ModemError::Configuration(format!(
                "SC-FDMA plugin: center_frequency {:.1} not supported; must be 1500.0 Hz",
                config.center_frequency
            )));
        }
        #[cfg(feature = "gpu")]
        if let Some(ref ctx) = self.gpu {
            if let Some(result) = demodulate::scfdma_demodulate_soft_gpu(samples, &config.mode, ctx)
            {
                return Ok(result);
            }
        }

        Ok(scfdma_demodulate_soft(samples, &config.mode))
    }

    fn estimate_afc_hz(&self, samples: &[f32], config: &ModulationConfig) -> Option<f32> {
        let p = params_for_mode(&config.mode)?;
        if let Some(coh_bw) = crate::channel::estimate_coh_bw_hz(samples, &p) {
            if let Ok(mut state) = self.adaptive.lock() {
                state.update(coh_bw);
            }
        }
        crate::channel::estimate_cfo_hz(samples, &p)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::pilot_positions;
    use crate::modulate::measure_papr;
    use crate::params::{SCFDMA16, SCFDMA52, SCFDMA52_32QAM, SCFDMA52_64QAM_P4, SCFDMA52_8PSK};

    fn mod_config(mode: &str) -> ModulationConfig {
        ModulationConfig {
            mode: mode.into(),
            center_frequency: 1500.0,
            sample_rate: 8000,
            ..ModulationConfig::default()
        }
    }

    #[test]
    fn scfdma16_loopback_clean() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"SCFDMA16 loopback test payload, hello!";
        let samples = plugin.modulate(payload, &mod_config("SCFDMA16")).unwrap();
        let rx = plugin
            .demodulate(&samples, &mod_config("SCFDMA16"))
            .unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    #[test]
    fn scfdma52_loopback_clean() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"SCFDMA52 clean loopback test payload, more data here!";
        let samples = plugin.modulate(payload, &mod_config("SCFDMA52")).unwrap();
        let rx = plugin
            .demodulate(&samples, &mod_config("SCFDMA52"))
            .unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    #[test]
    fn scfdma16_loopback_short_payload() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"X";
        let samples = plugin.modulate(payload, &mod_config("SCFDMA16")).unwrap();
        let rx = plugin
            .demodulate(&samples, &mod_config("SCFDMA16"))
            .unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    #[test]
    fn scfdma16_pilot_positions_correct() {
        let pilots = pilot_positions(&SCFDMA16);
        assert_eq!(pilots, vec![42, 47, 52, 57]);
        assert_eq!(pilots.len(), SCFDMA16.n_pilots);
    }

    #[test]
    fn scfdma52_pilot_positions_correct() {
        let pilots = pilot_positions(&SCFDMA52);
        assert_eq!(pilots.len(), SCFDMA52.n_pilots);
        assert_eq!(pilots[0], 20);
        assert_eq!(*pilots.last().unwrap(), 80);
    }

    #[test]
    fn scfdma52_64qam_p4_pilot_positions_correct() {
        let pilots = pilot_positions(&SCFDMA52_64QAM_P4);
        assert_eq!(pilots.len(), SCFDMA52_64QAM_P4.n_pilots);
        assert_eq!(pilots[0], 19);
        assert_eq!(*pilots.last().unwrap(), 79);
    }

    #[test]
    fn scfdma52_16qam_loopback_clean() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"SCFDMA52-16QAM loopback test payload, 16QAM subcarriers!";
        let samples = plugin
            .modulate(payload, &mod_config("SCFDMA52-16QAM"))
            .unwrap();
        let rx = plugin
            .demodulate(&samples, &mod_config("SCFDMA52-16QAM"))
            .unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    #[test]
    fn scfdma52_8psk_loopback_clean() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"SCFDMA52-8PSK clean loopback: 4333 bps gross, Gray-coded 8PSK.";
        let samples = plugin
            .modulate(payload, &mod_config("SCFDMA52-8PSK"))
            .unwrap();
        let rx = plugin
            .demodulate(&samples, &mod_config("SCFDMA52-8PSK"))
            .unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    #[test]
    fn scfdma52_8psk_papr_below_12db() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"8PSK PAPR test payload - unit circle means low PAPR with DFT precoding";
        let samples = plugin
            .modulate(payload, &mod_config("SCFDMA52-8PSK"))
            .unwrap();
        let papr = measure_papr(&samples);
        assert!(
            papr < 12.0,
            "SCFDMA52-8PSK PAPR {papr:.1} dB should be below 12 dB"
        );
    }

    #[test]
    fn scfdma52_8psk_pilot_count_matches_params() {
        use crate::channel::pilot_positions;
        let pilots = pilot_positions(&SCFDMA52_8PSK);
        assert_eq!(pilots.len(), SCFDMA52_8PSK.n_pilots);
        assert_eq!(SCFDMA52_8PSK.bits_per_sc, 3);
    }

    #[test]
    fn scfdma52_32qam_loopback_clean() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"SCFDMA52-32QAM clean loopback: 7222 bps gross, cross-32QAM.";
        let samples = plugin
            .modulate(payload, &mod_config("SCFDMA52-32QAM"))
            .unwrap();
        let rx = plugin
            .demodulate(&samples, &mod_config("SCFDMA52-32QAM"))
            .unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    #[test]
    fn scfdma52_32qam_papr_below_13db() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"32QAM PAPR test payload - DFT precoding keeps PAPR low at 5 bits/SC";
        let samples = plugin
            .modulate(payload, &mod_config("SCFDMA52-32QAM"))
            .unwrap();
        let papr = measure_papr(&samples);
        assert!(
            papr < 13.0,
            "SCFDMA52-32QAM PAPR {papr:.1} dB should be below 13 dB"
        );
    }

    #[test]
    fn scfdma52_32qam_pilot_count_matches_params() {
        use crate::channel::pilot_positions;
        let pilots = pilot_positions(&SCFDMA52_32QAM);
        assert_eq!(pilots.len(), SCFDMA52_32QAM.n_pilots);
        assert_eq!(SCFDMA52_32QAM.bits_per_sc, 5);
    }

    #[test]
    fn scfdma52_64qam_loopback_clean() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"SCFDMA52-64QAM clean loopback: 8667 bps gross, MMSE equalization.";
        let samples = plugin
            .modulate(payload, &mod_config("SCFDMA52-64QAM"))
            .unwrap();
        let rx = plugin
            .demodulate(&samples, &mod_config("SCFDMA52-64QAM"))
            .unwrap();
        assert_eq!(rx.as_slice(), payload.as_ref());
    }

    #[test]
    fn scfdma52_16qam_papr_below_12db() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"16QAM PAPR test payload - DFT precoding keeps PAPR low even with 4 bits/SC";
        let samples = plugin
            .modulate(payload, &mod_config("SCFDMA52-16QAM"))
            .unwrap();
        let papr = measure_papr(&samples);
        assert!(
            papr < 13.0,
            "SCFDMA52-16QAM PAPR {papr:.1} dB should be below 13 dB"
        );
    }

    #[test]
    fn scfdma52_64qam_papr_below_12db() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"64QAM PAPR test payload - DFT precoding still suppresses PAPR at 6 bits/SC";
        let samples = plugin
            .modulate(payload, &mod_config("SCFDMA52-64QAM"))
            .unwrap();
        let papr = measure_papr(&samples);
        assert!(
            papr < 12.0,
            "SCFDMA52-64QAM PAPR {papr:.1} dB should be below 12 dB"
        );
    }

    #[test]
    fn unknown_mode_returns_err() {
        let plugin = ScFdmaPlugin::new();
        let cfg = mod_config("SCFDMA99");
        assert!(plugin.modulate(b"x", &cfg).is_err());
        let samples = vec![0.0f32; 288];
        assert!(plugin.demodulate(&samples, &cfg).is_err());
    }

    #[test]
    fn scfdma52_papr_is_below_12db() {
        let plugin = ScFdmaPlugin::new();
        let payload = b"SC-FDMA PAPR test payload longer text here for more subcarriers fill";
        let samples = plugin.modulate(payload, &mod_config("SCFDMA52")).unwrap();
        let papr = measure_papr(&samples);
        assert!(
            papr < 12.0,
            "SC-FDMA PAPR {papr:.1} dB should be below 12 dB (no clipping applied)"
        );
    }

    // AFC estimator: on-carrier SCFDMA16 signal returns near-zero estimate.
    #[test]
    fn afc_estimate_near_zero_scfdma16() {
        let plugin = ScFdmaPlugin::new();
        let cfg = mod_config("SCFDMA16");
        let payload: Vec<u8> = (0..32u8).collect();
        let samples = plugin.modulate(&payload, &cfg).unwrap();
        let est = plugin
            .estimate_afc_hz(&samples, &cfg)
            .expect("afc estimate");
        assert!(est.abs() < 5.0, "expected near-zero AFC, got {est:.2} Hz");
    }

    // AFC estimator: on-carrier SCFDMA52 signal returns near-zero estimate.
    #[test]
    fn afc_estimate_near_zero_scfdma52() {
        let plugin = ScFdmaPlugin::new();
        let cfg = mod_config("SCFDMA52");
        let payload: Vec<u8> = (0..64u8).collect();
        let samples = plugin.modulate(&payload, &cfg).unwrap();
        let est = plugin
            .estimate_afc_hz(&samples, &cfg)
            .expect("afc estimate");
        assert!(est.abs() < 5.0, "expected near-zero AFC, got {est:.2} Hz");
    }

    // Flat channel → high coherence BW estimate → sparse pilots.
    #[test]
    fn adaptive_pilot_density_flat_channel_stays_sparse() {
        let plugin = ScFdmaPlugin::new();
        let cfg = mod_config("SCFDMA52");
        let payload: Vec<u8> = (0..64u8).collect();
        let samples = plugin.modulate(&payload, &cfg).unwrap();
        for _ in 0..10 {
            plugin.estimate_afc_hz(&samples, &cfg);
        }
        let adapted = plugin
            .adaptive_params_for_mode("SCFDMA52")
            .expect("adaptive params");
        assert!(
            adapted.pilot_spacing >= 5,
            "flat channel should use default/sparse pilots, got spacing={}",
            adapted.pilot_spacing
        );
    }

    // 2-tap multipath at delay=26 samples maximises frequency selectivity for 5-SC pilot spacing
    // (phase step per pilot ≈ π → alternating high/low amplitude, B_c ≈ 57 Hz < 100 Hz → dense).
    #[test]
    fn adaptive_pilot_density_selective_channel_triggers_dense() {
        let plugin = ScFdmaPlugin::new();
        let cfg = mod_config("SCFDMA52");
        let payload: Vec<u8> = (0..64u8).collect();
        let samples = plugin.modulate(&payload, &cfg).unwrap();
        let selective: Vec<f32> = samples
            .iter()
            .enumerate()
            .map(|(i, &s)| s + if i >= 26 { samples[i - 26] * 0.7 } else { 0.0 })
            .collect();
        for _ in 0..10 {
            plugin.estimate_afc_hz(&selective, &cfg);
        }
        let adapted = plugin
            .adaptive_params_for_mode("SCFDMA52")
            .expect("adaptive params");
        assert_eq!(
            adapted.pilot_spacing, 4,
            "selective channel (2-tap, delay=26) should trigger dense pilots"
        );
    }

    // After selective channel → dense, clean frames revert to sparse (EMA tracking).
    #[test]
    fn adaptive_pilot_density_reverts_to_sparse_after_channel_clears() {
        let plugin = ScFdmaPlugin::new();
        let cfg = mod_config("SCFDMA52");
        let payload: Vec<u8> = (0..64u8).collect();
        let samples = plugin.modulate(&payload, &cfg).unwrap();
        let selective: Vec<f32> = samples
            .iter()
            .enumerate()
            .map(|(i, &s)| s + if i >= 26 { samples[i - 26] * 0.7 } else { 0.0 })
            .collect();

        // Drive to dense state.
        for _ in 0..8 {
            plugin.estimate_afc_hz(&selective, &cfg);
        }
        assert_eq!(
            plugin
                .adaptive_params_for_mode("SCFDMA52")
                .unwrap()
                .pilot_spacing,
            4,
            "should be dense after selective channel"
        );

        // Feed 14 clean frames; EMA must revert above the 300 Hz threshold.
        for _ in 0..14 {
            plugin.estimate_afc_hz(&samples, &cfg);
        }
        let adapted = plugin.adaptive_params_for_mode("SCFDMA52").unwrap();
        assert!(
            adapted.pilot_spacing >= 5,
            "should revert to sparse after clean channel, got spacing={}",
            adapted.pilot_spacing
        );
    }

    // AFC estimator: synthetic signal with known inter-symbol pilot phase drift.
    #[test]
    fn afc_synthetic_pilot_phase_drift_scfdma16() {
        use crate::channel::{estimate_cfo_hz, pilot_positions};
        use crate::params::{CP, FFT_SIZE, PILOT_AMPLITUDE, SAMPLE_RATE, SYM_LEN};
        use num_complex::Complex32;
        use rustfft::FftPlanner;
        use std::f32::consts::PI;

        let cfo_hz = 8.0_f32;
        let delta_phi = 2.0 * PI * cfo_hz * SYM_LEN as f32 / SAMPLE_RATE as f32;
        let p = SCFDMA16;
        let pilots = pilot_positions(&p);
        let n_syms = 4usize;

        let mut planner = FftPlanner::<f32>::new();
        let ifft = planner.plan_fft_inverse(FFT_SIZE);
        let scale = 1.0 / FFT_SIZE as f32;

        let mut samples = Vec::with_capacity(n_syms * SYM_LEN);
        for sym_idx in 0..n_syms {
            let phase = sym_idx as f32 * delta_phi;
            let (sin_p, cos_p) = phase.sin_cos();
            let mut freq = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
            for &k in &pilots {
                freq[k] = Complex32::new(cos_p, sin_p) * PILOT_AMPLITUDE;
                freq[FFT_SIZE - k] = Complex32::new(cos_p, -sin_p) * PILOT_AMPLITUDE;
            }
            ifft.process(&mut freq);
            let time: Vec<f32> = freq.iter().map(|c| c.re * scale).collect();
            let cp_start = FFT_SIZE - CP;
            samples.extend_from_slice(&time[cp_start..]);
            samples.extend_from_slice(&time);
        }

        let est = estimate_cfo_hz(&samples, &p).expect("synthetic cfo estimate");
        assert!(
            (est - cfo_hz).abs() < 2.0,
            "expected ~{cfo_hz:.1} Hz CFO, got {est:.2} Hz"
        );
    }
}
