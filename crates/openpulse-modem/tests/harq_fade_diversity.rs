//! HARQ over a fading channel: the union of "retry" and "combine" beats either alone.
//!
//! After PRs #685–#693 the SC-FDMA receiver is equalizer-limited nowhere: what still loses frames on
//! Watterson is **deep-fade outage**, where the channel spent the burst in a null. No per-frame receiver
//! technique recovers those (see the P7/IBDFE rejection in `docs/dev/research/scfdma-improvements.md`).
//! Only *diversity* does — independent fade states across retransmissions.
//!
//! Two ways to use them, and neither dominates the other:
//!   * **plain ARQ retry** — decode each attempt on its own; wins when one attempt is simply clean;
//!   * **soft combining** — sum the calibrated LLRs; wins when every attempt is partially ruined and they
//!     carry complementary information.
//!
//! `receive_with_llr_combining` now does both, so its success is a strict superset. These tests pin that.

use openpulse_audio::LoopbackBackend;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_modem::engine::ModemEngine;
use scfdma_plugin::ScFdmaPlugin;

const PAYLOAD: &[u8] = b"HARQ fade-diversity gate payload, sixty-four bytes AAAAAAAAA";
const TRIALS: u32 = 60;
const ATTEMPTS: usize = 3;

fn make() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine
        .register_plugin(Box::new(ScFdmaPlugin::new()))
        .expect("register");
    (engine, backend)
}

fn tx_samples(mode: &str) -> Vec<f32> {
    let (mut engine, backend) = make();
    engine
        .transmit_with_fec(PAYLOAD, mode, None)
        .expect("transmit");
    backend.drain_samples()
}

/// An independent Watterson `moderate_f1` realisation (1 ms delay spread, 1 Hz Doppler).
fn faded(tx: &[f32], snr_db: f32, seed: u64) -> Vec<f32> {
    let mut cfg = WattersonConfig::moderate_f1(Some(seed));
    cfg.snr_db = snr_db;
    WattersonChannel::new(cfg).expect("watterson").apply(tx)
}

fn seed(trial: u32, attempt: usize) -> u64 {
    7000 + (trial as u64) * 10 + attempt as u64
}

/// Plain ARQ retry: succeeds if any single attempt decodes standalone.
fn retry_success(mode: &str, tx: &[f32], snr_db: f32) -> f32 {
    let mut ok = 0u32;
    for trial in 0..TRIALS {
        for attempt in 0..ATTEMPTS {
            let (mut rx, backend) = make();
            backend.push_frame(&faded(tx, snr_db, seed(trial, attempt)));
            if rx
                .receive_with_fec(mode, None)
                .map(|d| d == PAYLOAD)
                .unwrap_or(false)
            {
                ok += 1;
                break;
            }
        }
    }
    ok as f32 / TRIALS as f32
}

/// What the engine does: each attempt standalone, then the MAP-combined LLRs.
fn engine_success(mode: &str, tx: &[f32], snr_db: f32) -> f32 {
    let mut ok = 0u32;
    for trial in 0..TRIALS {
        let (mut rx, backend) = make();
        for attempt in 0..ATTEMPTS {
            backend.push_frame(&faded(tx, snr_db, seed(trial, attempt)));
        }
        if rx
            .receive_with_llr_combining(mode, None, ATTEMPTS)
            .map(|d| d == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    ok as f32 / TRIALS as f32
}

/// On `moderate_f1` the dense rungs fail by outage, so the attempts carry complementary information and
/// the MAP sum decodes frames no single attempt can. Combining must therefore beat plain retry outright.
///
/// Measured before the union landed (60 trials): plain retry 0.43, combining alone 0.48, both 0.67.
#[test]
fn combining_beats_plain_retry_on_a_fading_channel() {
    let mode = "SCFDMA52-16QAM";
    let tx = tx_samples(mode);
    let retry = retry_success(mode, &tx, 28.0);
    let engine = engine_success(mode, &tx, 28.0);
    assert!(
        engine > retry + 0.10,
        "moderate_f1 @28 dB, {ATTEMPTS} attempts: engine {engine:.2} vs plain retry {retry:.2} — soft \
         combining must add real diversity gain over simply retrying"
    );
}

/// The union can never lose a frame that plain retry keeps: every attempt is tried standalone first.
/// This is the invariant that makes combining safe to enable unconditionally — combining *alone* is
/// worse than retry on the low-order rungs, where a single clean attempt is likely and summing it with
/// two ruined ones dilutes it (measured on `moderate_f1`, SCFDMA52, combining *alone* vs retry: 12 dB
/// 0.70 vs 0.75, 20 dB 0.83 vs 0.88, 28 dB 0.88 vs 0.92).
#[test]
fn combining_never_loses_a_frame_plain_retry_would_have_kept() {
    for (mode, snr_db) in [
        ("SCFDMA52", 12.0f32),
        ("SCFDMA52", 20.0),
        ("SCFDMA52", 28.0),
    ] {
        let tx = tx_samples(mode);
        let retry = retry_success(mode, &tx, snr_db);
        let engine = engine_success(mode, &tx, snr_db);
        assert!(
            engine >= retry,
            "{mode} @{snr_db} dB: engine {engine:.2} fell below plain retry {retry:.2} — the standalone \
             decode of each attempt must run before the attempts are combined"
        );
    }
}
