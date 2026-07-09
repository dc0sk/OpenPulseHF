//! HARQ soft-combining across OTA retransmissions, driven through the daemon's decode entry
//! (`ota_decode_burst`).
//!
//! `receive_with_llr_combining` (the #694 union) is synchronous multi-capture and RS-only, so it
//! never fit the daemon's async, per-MODCOD OTA flow — the diversity gain it measured never reached
//! the air. This wires it in: `ota_decode_and_ack` now retains the soft LLRs of a *failed* burst,
//! keyed by `(session, mode)`, and MAP-combines them with the next burst of the same mode before
//! giving up. On `moderate_f1` near a rung's threshold each burst is a partially-ruined observation of
//! the same bits; summing their calibrated LLRs across independent fade realisations decodes frames no
//! single burst can. Exercised here on SL12 (`OFDM52-16QAM` after the SC-FDMA→OFDM re-seat).
//!
//! The gate: over many fade realisations, a single engine that retains and combines across three
//! sequential bursts must decode more frames than three *independent* engines each decoding one burst
//! standalone. That gap is the diversity the retention adds; a regression that drops the retained LLRs
//! (or clears them too eagerly) collapses it back to the standalone baseline.

use ofdm_plugin::OfdmPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;

const PAYLOAD: &[u8] = b"OTA HARQ combining gate payload, sixty-four bytes AAAAAAAAAAA";
const MODE: &str = "OFDM52-16QAM"; // hpx_hf SL12 = OFDM52-16QAM + SoftConcatenated (post SC-FDMA→OFDM re-seat)
const FEC: FecMode = FecMode::SoftConcatenated;
const SESSION: &str = "harq-sess";
const TRIALS: u32 = 50;
const ATTEMPTS: usize = 3;
const SNR_DB: f32 = 10.0;

fn make() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .expect("register");
    // Lock the receiver-led OTA controller to SL12 so `rx_candidates()` is exactly
    // (OFDM52-16QAM, SoftConcatenated) — the soft rung HARQ combining acts on.
    engine.start_ota_session(SessionProfile::hpx_hf());
    engine.ota_lock_level(SpeedLevel::Sl12);
    (engine, backend)
}

fn tx_samples() -> Vec<f32> {
    let (mut engine, backend) = make();
    engine
        .transmit_with_fec_mode(PAYLOAD, MODE, FEC, None)
        .expect("transmit");
    backend.drain_samples()
}

/// An independent Watterson `moderate_f1` realisation (1 ms delay spread, 1 Hz Doppler).
fn faded(tx: &[f32], seed: u64) -> Vec<f32> {
    let mut cfg = WattersonConfig::moderate_f1(Some(seed));
    cfg.snr_db = SNR_DB;
    WattersonChannel::new(cfg).expect("watterson").apply(tx)
}

fn seed(trial: u32, attempt: usize) -> u64 {
    9100 + (trial as u64) * 10 + attempt as u64
}

/// Baseline: each of the three bursts decoded standalone by a *fresh* engine (no retained state).
/// Succeeds if any single burst decodes on its own.
fn standalone_success(tx: &[f32]) -> f32 {
    let mut ok = 0u32;
    for trial in 0..TRIALS {
        for attempt in 0..ATTEMPTS {
            let (mut rx, _backend) = make();
            let burst = AudioSamples {
                samples: faded(tx, seed(trial, attempt)),
            };
            if rx
                .ota_decode_burst(&burst, SESSION)
                .ok()
                .and_then(|r| r.payload)
                .map(|p| p == PAYLOAD)
                .unwrap_or(false)
            {
                ok += 1;
                break;
            }
        }
    }
    ok as f32 / TRIALS as f32
}

/// Treatment: one engine feeds the three bursts in sequence via `ota_decode_burst`, so a failed
/// burst's LLRs are retained and MAP-combined into the next. Succeeds if any burst yields the frame.
fn combining_success(tx: &[f32]) -> f32 {
    let mut ok = 0u32;
    for trial in 0..TRIALS {
        let (mut rx, _backend) = make();
        let mut got = false;
        for attempt in 0..ATTEMPTS {
            let burst = AudioSamples {
                samples: faded(tx, seed(trial, attempt)),
            };
            if rx
                .ota_decode_burst(&burst, SESSION)
                .ok()
                .and_then(|r| r.payload)
                .map(|p| p == PAYLOAD)
                .unwrap_or(false)
            {
                got = true;
                break;
            }
        }
        if got {
            ok += 1;
        }
    }
    ok as f32 / TRIALS as f32
}

/// Retaining and combining failed-burst LLRs across retransmissions must decode strictly more frames
/// than deciding each burst independently — the diversity gain HARQ combining exists to capture.
#[test]
fn ota_retention_combines_across_retransmissions() {
    let tx = tx_samples();
    let standalone = standalone_success(&tx);
    let combining = combining_success(&tx);
    assert!(
        combining > standalone + 0.08,
        "moderate_f1 @{SNR_DB} dB, {ATTEMPTS} bursts: combining {combining:.2} vs standalone \
         {standalone:.2} — retained-LLR MAP combining across OTA retransmissions must add diversity gain"
    );
}
