//! `openpulse calibrate` — on-device audio/PTT/AFC calibration checks.

use std::f32::consts::PI;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use serde::Serialize;

use crate::radio::build_ptt_controller;

#[derive(clap::Subcommand)]
pub enum CalibrateCommands {
    /// Measure audio input level and headroom to clip.
    Audio,
    /// Measure PTT assert/release latency against the 50 ms target.
    Ptt,
    /// Measure AFC frequency offset using a BPSK250 loopback burst.
    Afc,
    /// Find the TX attenuation that lands the rig's ALC in a moderate band
    /// (requires a real radio + rigctld + the `cpal-backend` feature). Keys the
    /// transmitter, so run into a dummy load / with care.
    Drive {
        /// Modulation mode to tune with (default `OFDM52`, the high-PAPR case).
        #[arg(long, default_value = "OFDM52")]
        mode: String,
        /// Lower bound of the target ALC band (rigctld 0.0–1.0).
        #[arg(long, default_value_t = 0.3)]
        target_alc_lo: f32,
        /// Upper bound of the target ALC band (rigctld 0.0–1.0).
        #[arg(long, default_value_t = 0.5)]
        target_alc_hi: f32,
    },
}

#[derive(Serialize)]
pub struct CalibrationResult {
    test: String,
    result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    headroom_db: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    afc_offset_hz: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

/// Result of the `calibrate drive` ALC-targeting routine.
#[derive(Serialize)]
pub struct DriveResult {
    test: String,
    result: String,
    recommended_tx_attenuation_db: f32,
    final_alc: Option<f32>,
    iterations: u32,
    message: String,
}

/// Outcome of one drive-tuning observation.
#[cfg_attr(not(feature = "cpal-backend"), allow(dead_code))]
#[derive(Debug, PartialEq)]
pub enum DriveStep {
    /// Apply this TX attenuation (dB, ≤ 0) and measure again.
    Adjust(f32),
    /// ALC is within the target band at this attenuation.
    Converged(f32),
    /// Could not reach the band — hit an attenuation rail or the iteration cap.
    GiveUp(f32),
}

/// Pure convergence logic for `calibrate drive`. Given a measured ALC (rigctld
/// 0.0–1.0), it picks the next TX attenuation (dB) to land ALC in `[lo, hi]`.
/// ALC decreases monotonically with attenuation, so this is a guarded step
/// search that halves its step on overshoot — kept separate from the RF loop so
/// it can be unit-tested without hardware.
#[cfg_attr(not(feature = "cpal-backend"), allow(dead_code))]
pub struct DriveTuner {
    lo: f32,
    hi: f32,
    atten: f32,
    step: f32,
    min_atten: f32,
    max_atten: f32,
    last_dir: i8,
    iters: u32,
    max_iters: u32,
}

#[cfg_attr(not(feature = "cpal-backend"), allow(dead_code))]
impl DriveTuner {
    /// Target band `[lo, hi]` (ALC) and a starting attenuation (dB, ≤ 0).
    pub fn new(lo: f32, hi: f32, start_atten: f32) -> Self {
        Self {
            lo,
            hi,
            atten: start_atten.clamp(-40.0, 0.0),
            step: 4.0,
            min_atten: -40.0,
            max_atten: 0.0,
            last_dir: 0,
            iters: 0,
            max_iters: 16,
        }
    }

    /// Current attenuation to transmit at.
    pub fn attenuation(&self) -> f32 {
        self.atten
    }

    /// Iterations performed so far.
    pub fn iterations(&self) -> u32 {
        self.iters
    }

    /// Fold in a measured ALC and decide the next move.
    pub fn observe(&mut self, alc: f32) -> DriveStep {
        self.iters += 1;
        if alc >= self.lo && alc <= self.hi {
            return DriveStep::Converged(self.atten);
        }
        if self.iters >= self.max_iters {
            return DriveStep::GiveUp(self.atten);
        }
        // ALC too high ⇒ more attenuation (more negative); too low ⇒ less.
        let dir: i8 = if alc > self.hi { -1 } else { 1 };
        // Overshoot (direction reversal) ⇒ halve the step for a finer approach.
        if self.last_dir != 0 && dir != self.last_dir {
            self.step = (self.step * 0.5).max(0.5);
        }
        self.last_dir = dir;
        let next = (self.atten + dir as f32 * self.step).clamp(self.min_atten, self.max_atten);
        if (next - self.atten).abs() < 1e-3 {
            // Hit a rail and still out of band: more drive / external pad needed.
            return DriveStep::GiveUp(self.atten);
        }
        self.atten = next;
        DriveStep::Adjust(self.atten)
    }
}

const SAMPLE_RATE: f32 = 8_000.0;
const TONE_FREQ_HZ: f32 = 1_500.0;
const TONE_LEVEL_DBFS: f32 = -18.0;
const PTT_TARGET_MS: f32 = 50.0;

fn sine_tone(freq_hz: f32, level_dbfs: f32, duration_s: f32) -> Vec<f32> {
    let n = (duration_s * SAMPLE_RATE) as usize;
    let amplitude = 10f32.powf(level_dbfs / 20.0);
    (0..n)
        .map(|i| amplitude * (2.0 * PI * freq_hz * i as f32 / SAMPLE_RATE).sin())
        .collect()
}

fn rms_dbfs(samples: &[f32]) -> Option<f32> {
    if samples.is_empty() {
        return None;
    }
    let rms = (samples.iter().map(|&s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    if rms <= 0.0 {
        return None;
    }
    Some(20.0 * rms.log10())
}

/// Measure audio level and headroom.
pub fn run_audio() -> Result<CalibrationResult> {
    let loopback = LoopbackBackend::new();
    let samples = sine_tone(TONE_FREQ_HZ, TONE_LEVEL_DBFS, 0.5);
    loopback.fill_samples(&samples);
    let received = loopback.drain_samples();
    match rms_dbfs(&received) {
        Some(db) => Ok(CalibrationResult {
            test: "audio".into(),
            result: "pass".into(),
            headroom_db: Some(-db),
            latency_ms: None,
            afc_offset_hz: None,
            message: None,
        }),
        None => Ok(CalibrationResult {
            test: "audio".into(),
            result: "fail".into(),
            headroom_db: None,
            latency_ms: None,
            afc_offset_hz: None,
            message: Some("no samples captured".into()),
        }),
    }
}

/// Measure PTT assert/release round-trip latency against the 50 ms target.
pub fn run_ptt(ptt_backend: &str, rig: &str, rig_file: &str) -> Result<CalibrationResult> {
    let mut ptt = build_ptt_controller(ptt_backend, rig, rig_file)?;
    let t0 = Instant::now();
    let assert_result = ptt.assert_ptt();
    let release_result = ptt.release_ptt();
    let elapsed_ms = t0.elapsed().as_secs_f32() * 1_000.0;
    let assert_ok = assert_result.is_ok();
    let release_ok = release_result.is_ok();
    let pass = assert_ok && release_ok && elapsed_ms < PTT_TARGET_MS;
    let message = if !pass {
        let mut parts = Vec::new();
        if let Err(e) = assert_result {
            parts.push(format!("assert error: {e}"));
        }
        if let Err(e) = release_result {
            parts.push(format!("release error: {e}"));
        }
        if elapsed_ms >= PTT_TARGET_MS {
            parts.push(format!(
                "latency {elapsed_ms:.1} ms vs {PTT_TARGET_MS} ms target"
            ));
        }
        Some(parts.join("; "))
    } else {
        None
    };
    Ok(CalibrationResult {
        test: "ptt".into(),
        result: if pass { "pass" } else { "fail" }.into(),
        headroom_db: None,
        latency_ms: Some(elapsed_ms),
        afc_offset_hz: None,
        message,
    })
}

/// Measure AFC frequency offset using a BPSK250 loopback burst.
pub fn run_afc() -> Result<CalibrationResult> {
    let loopback = LoopbackBackend::new();
    let shared = loopback.clone_shared();
    let mut engine = ModemEngine::new(Box::new(loopback));
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .map_err(anyhow::Error::new)?;
    engine.disable_csma();

    if let Err(e) = engine.transmit(b"AFC_CAL_TEST", "BPSK250", None) {
        return Ok(CalibrationResult {
            test: "afc".into(),
            result: "fail".into(),
            headroom_db: None,
            latency_ms: None,
            afc_offset_hz: None,
            message: Some(format!("TX failed: {e}")),
        });
    }
    let tx = shared.drain_samples();
    if tx.is_empty() {
        return Ok(CalibrationResult {
            test: "afc".into(),
            result: "fail".into(),
            headroom_db: None,
            latency_ms: None,
            afc_offset_hz: None,
            message: Some("no TX samples from modulator".into()),
        });
    }
    shared.fill_samples(&tx);
    if let Err(e) = engine.receive("BPSK250", None) {
        return Ok(CalibrationResult {
            test: "afc".into(),
            result: "fail".into(),
            headroom_db: None,
            latency_ms: None,
            afc_offset_hz: None,
            message: Some(format!("RX failed: {e}")),
        });
    }
    let afc_offset_hz = engine.last_afc_offset_hz();
    let pass = afc_offset_hz.is_some();
    Ok(CalibrationResult {
        test: "afc".into(),
        result: if pass { "pass" } else { "fail" }.into(),
        headroom_db: None,
        latency_ms: None,
        afc_offset_hz,
        message: if !pass {
            Some("demodulation produced no AFC estimate".into())
        } else {
            None
        },
    })
}

/// Guided ALC drive tuning — keys the radio (needs `cpal-backend` + rigctld).
#[cfg(not(feature = "cpal-backend"))]
pub fn run_drive(_rig: &str, _mode: &str, _lo: f32, _hi: f32) -> Result<DriveResult> {
    anyhow::bail!(
        "calibrate drive needs real audio to the radio: rebuild with --features cpal \
         and run with a rigctld-controlled rig (--rig <addr>)"
    )
}

/// Guided ALC drive tuning: step TX attenuation until the rig's ALC (read via
/// rigctld) sits in `[lo, hi]`. Keys the transmitter — use into a dummy load.
#[cfg(feature = "cpal-backend")]
pub fn run_drive(rig: &str, mode: &str, lo: f32, hi: f32) -> Result<DriveResult> {
    use std::thread::sleep;
    use std::time::Duration;

    use anyhow::Context;
    use ofdm_plugin::OfdmPlugin;
    use openpulse_audio::CpalBackend;
    use openpulse_radio::{PttController, RigctldController};

    let mut rigc = RigctldController::connect(rig).with_context(|| {
        format!("calibrate drive needs rigctld for ALC/PTT at {rig} (start rigctld for your rig)")
    })?;
    let mut engine = ModemEngine::new(Box::new(CpalBackend::new()));
    engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .map_err(anyhow::Error::new)?;
    engine.disable_csma();
    let payload: Vec<u8> = (0..255u16)
        .map(|i| (i.wrapping_mul(37).wrapping_add(11)) as u8)
        .collect();

    let mut tuner = DriveTuner::new(lo, hi, 0.0);
    loop {
        engine.set_tx_attenuation_db(tuner.attenuation());
        rigc.assert_ptt().context("PTT assert (rigctld)")?;
        // Drive a short burst, then sample ALC (peak over the burst).
        for _ in 0..3 {
            let _ = engine.transmit(&payload, mode, None);
        }
        sleep(Duration::from_millis(150));
        let mut alc = 0.0f32;
        let mut got = false;
        for _ in 0..4 {
            if let Ok(a) = rigc.get_alc() {
                alc = alc.max(a);
                got = true;
            }
            sleep(Duration::from_millis(80));
        }
        let _ = rigc.release_ptt();
        if !got {
            anyhow::bail!("no ALC reading from rigctld during TX (rig may not expose ALC)");
        }
        match tuner.observe(alc) {
            DriveStep::Adjust(_) => {
                sleep(Duration::from_millis(250));
                continue;
            }
            DriveStep::Converged(a) => {
                return Ok(DriveResult {
                    test: "drive".into(),
                    result: "pass".into(),
                    recommended_tx_attenuation_db: a,
                    final_alc: Some(alc),
                    iterations: tuner.iterations(),
                    message: format!(
                        "ALC {alc:.2} within [{lo:.2}, {hi:.2}] at {a:.1} dB TX attenuation"
                    ),
                });
            }
            DriveStep::GiveUp(a) => {
                return Ok(DriveResult {
                    test: "drive".into(),
                    result: "fail".into(),
                    recommended_tx_attenuation_db: a,
                    final_alc: Some(alc),
                    iterations: tuner.iterations(),
                    message: format!(
                        "could not reach ALC band (last {alc:.2} at {a:.1} dB): adjust the \
                         rig's data/USB drive or add external attenuation"
                    ),
                });
            }
        }
    }
}

/// Dispatch to the selected calibration sub-test and optionally write JSON output.
pub fn run(
    command: &CalibrateCommands,
    ptt: &str,
    rig: &str,
    rig_file: &str,
    output: Option<&PathBuf>,
) -> Result<()> {
    let json = match command {
        CalibrateCommands::Audio => serde_json::to_string_pretty(&run_audio()?)?,
        CalibrateCommands::Ptt => serde_json::to_string_pretty(&run_ptt(ptt, rig, rig_file)?)?,
        CalibrateCommands::Afc => serde_json::to_string_pretty(&run_afc()?)?,
        CalibrateCommands::Drive {
            mode,
            target_alc_lo,
            target_alc_hi,
        } => serde_json::to_string_pretty(&run_drive(rig, mode, *target_alc_lo, *target_alc_hi)?)?,
    };
    println!("{json}");
    if let Some(path) = output {
        std::fs::write(path, &json)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulated rig: ALC falls monotonically with attenuation; ~0.8 at 0 dB.
    fn sim_alc(atten: f32) -> f32 {
        (0.8 * 10f32.powf(atten / 20.0)).clamp(0.0, 1.0)
    }

    fn run_tuner(mut t: DriveTuner, alc_fn: impl Fn(f32) -> f32) -> DriveStep {
        loop {
            let step = t.observe(alc_fn(t.attenuation()));
            if !matches!(step, DriveStep::Adjust(_)) {
                return step;
            }
        }
    }

    #[test]
    fn converges_into_band_from_overdrive() {
        let t = DriveTuner::new(0.3, 0.5, 0.0);
        match run_tuner(t, sim_alc) {
            DriveStep::Converged(a) => {
                let alc = sim_alc(a);
                assert!(
                    (0.3..=0.5).contains(&alc),
                    "ALC {alc} at {a} dB not in band"
                );
            }
            other => panic!("expected convergence, got {other:?}"),
        }
    }

    #[test]
    fn gives_up_when_band_unreachable_too_low() {
        // No attenuation (0 dB) already only reaches ALC 0.8 < 0.9 — can't go higher.
        let t = DriveTuner::new(0.9, 0.95, -6.0);
        assert!(matches!(run_tuner(t, sim_alc), DriveStep::GiveUp(_)));
    }

    #[test]
    fn converges_from_cold_start() {
        // Start heavily attenuated (ALC too low) — must step toward 0 into the band.
        let t = DriveTuner::new(0.3, 0.5, -30.0);
        match run_tuner(t, sim_alc) {
            DriveStep::Converged(a) => assert!((0.3..=0.5).contains(&sim_alc(a))),
            other => panic!("expected convergence, got {other:?}"),
        }
    }

    #[test]
    fn immediate_convergence_when_already_in_band() {
        let mut t = DriveTuner::new(0.3, 0.5, -8.0);
        assert!(matches!(t.observe(0.4), DriveStep::Converged(a) if (a + 8.0).abs() < 1e-3));
    }
}
