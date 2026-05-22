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

/// Dispatch to the selected calibration sub-test and optionally write JSON output.
pub fn run(
    command: &CalibrateCommands,
    ptt: &str,
    rig: &str,
    rig_file: &str,
    output: Option<&PathBuf>,
) -> Result<()> {
    let result = match command {
        CalibrateCommands::Audio => run_audio()?,
        CalibrateCommands::Ptt => run_ptt(ptt, rig, rig_file)?,
        CalibrateCommands::Afc => run_afc()?,
    };
    let json = serde_json::to_string_pretty(&result)?;
    println!("{json}");
    if let Some(path) = output {
        std::fs::write(path, &json)?;
    }
    Ok(())
}
