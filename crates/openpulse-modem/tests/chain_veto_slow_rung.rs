//! END-TO-END — does the deployed chain refuse a steady tone at a slow rung? (It does not.)
//!
//! The repo's own history says a component-level tone verdict is not the deployed verdict: the AFC
//! settle *rescues* BPSK250's component tone hole, because a locked settle lands on the tone and the
//! template's lines end up `baud/4` = 62.5 Hz away, outside the ±20 Hz grid.
//!
//! That rescue is a coincidence of magnitudes. The parking distance *is* `baud/4`, so at BPSK31 it
//! is 7.8 Hz — inside the same grid. `ddc_correlation_equivalence::p4` measured that geometry with
//! the settle *modelled* as a perfect lock; this runs the actual chain, so the settle is whatever
//! `afc_mini_settle` really produces rather than what the model assumes.
//!
//! **This test deliberately defeats a shipped guard.** `bpsk` refuses to publish a BPSK31 template
//! precisely because its constants are not derived and its grid reaches its own first spectral
//! line. `UnvettedSlowRung` publishes one anyway, which is the configuration the guard exists to
//! prevent — the point is to measure what that configuration does, not to propose it.

use openpulse_audio::loopback::LoopbackBackend;
use openpulse_core::error::ModemError;
use openpulse_core::fec::FecMode;
use openpulse_core::plugin::{
    FrameGeometry, ModulationConfig, ModulationPlugin, PluginInfo, PreambleTemplate,
};
use openpulse_modem::engine::ModemEngine;
use std::f32::consts::PI;
use std::time::Duration;

const FS: f32 = 8_000.0;
const FC: f32 = 1_500.0;

/// `BpskPlugin` that publishes a template for a mode whose constants were never derived.
///
/// Everything is delegated; only `preamble_template` differs, and only in that it hands out the
/// BPSK250-derived threshold and grid for whichever mode is asked — which is exactly the state the
/// correlation cap used to hide and the shipped guard now refuses.
struct UnvettedSlowRung(bpsk_plugin::BpskPlugin);

impl ModulationPlugin for UnvettedSlowRung {
    fn info(&self) -> &PluginInfo {
        self.0.info()
    }
    fn modulate(&self, d: &[u8], c: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        self.0.modulate(d, c)
    }
    fn demodulate(&self, s: &[f32], c: &ModulationConfig) -> Result<Vec<u8>, ModemError> {
        self.0.demodulate(s, c)
    }
    fn demodulate_soft(&self, s: &[f32], c: &ModulationConfig) -> Result<Vec<f32>, ModemError> {
        self.0.demodulate_soft(s, c)
    }
    fn frame_geometry(&self, c: &ModulationConfig) -> Option<FrameGeometry> {
        self.0.frame_geometry(c)
    }
    fn estimate_snr_db(&self, s: &[f32], c: &ModulationConfig) -> Option<f32> {
        self.0.estimate_snr_db(s, c)
    }
    fn supports_soft_demod(&self, m: &str) -> bool {
        self.0.supports_soft_demod(m)
    }
    fn estimate_afc_hz(&self, s: &[f32], c: &ModulationConfig) -> Option<f32> {
        self.0.estimate_afc_hz(s, c)
    }
    fn occupied_bandwidth_hz(&self, m: &str) -> Option<f32> {
        self.0.occupied_bandwidth_hz(m)
    }
    fn modulate_iq(
        &self,
        d: &[u8],
        c: &ModulationConfig,
    ) -> Result<(Vec<f32>, Vec<f32>), ModemError> {
        self.0.modulate_iq(d, c)
    }
    fn preamble_template(&self, config: &ModulationConfig) -> Option<PreambleTemplate> {
        let samples = bpsk_plugin::modulate::bpsk_preamble_template(config).ok()?;
        Some(PreambleTemplate::new(
            config.mode.clone(),
            samples,
            bpsk_plugin::modulate::PREAMBLE_RHO_THRESHOLD,
            bpsk_plugin::modulate::PREAMBLE_RHO_GRID_HZ,
        ))
    }
}

fn tone(f: f32, a: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|k| a * (2.0 * PI * f * k as f32 / FS).cos())
        .collect()
}

/// Feed a steady tone to a receiver listening on `mode`; report what the veto did.
fn run(mode: &str, offset_hz: f32) -> (u64, u64) {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(UnvettedSlowRung(bpsk_plugin::BpskPlugin::new())))
        .expect("register");
    // Work-based budgets per #1066, or the counts measure the CPU.
    e.set_deterministic_scan_positions(Some(3_000));
    e.set_deterministic_max_iterations(Some(600));
    backend.fill_samples(&tone(FC + offset_hz, 0.25, (6.0 * FS) as usize));
    let _ = e.receive_with_fec_mode_timeout(mode, FecMode::Rs, None, Duration::from_millis(60_000));
    (e.rho_accepted_settles(), e.rho_rejected_settles())
}

#[test]
#[ignore = "verification"]
fn q1_does_the_chain_refuse_a_tone_at_the_slow_rungs() {
    println!("\nQ1: steady tone through the FULL chain, veto active on each mode");
    println!("    (BPSK31/63 templates published by a test-local plugin that defeats the shipped");
    println!("     guard on purpose — that configuration is what is being measured)\n");
    println!(
        "{:<10} {:>8} {:>9} {:>10} {:>10} {:>28}",
        "mode", "baud/4", "offset", "accepted", "rejected", "verdict"
    );
    for (mode, baud) in [("BPSK31", 31.25f32), ("BPSK63", 62.5), ("BPSK250", 250.0)] {
        for offset in [0.0f32, 37.0] {
            let (acc, rej) = run(mode, offset);
            let verdict = if acc > 0 {
                "CORROBORATES a steady tone"
            } else if rej > 0 {
                "refuses"
            } else {
                "never reached the veto"
            };
            println!(
                "{:<10} {:>8.2} {:>9.0} {:>10} {:>10} {:>28}",
                mode,
                baud / 4.0,
                offset,
                acc,
                rej,
                verdict
            );
        }
    }
    println!(
        "\n  The settle's rescue needs baud/4 to exceed the grid half-width (20 Hz). Where it"
    );
    println!("  does not, a locked settle hands the correlator a tone within reach of a line and");
    println!("  the chain's own protection becomes the hazard.");
}
