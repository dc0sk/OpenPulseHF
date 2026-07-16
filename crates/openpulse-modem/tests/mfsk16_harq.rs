//! HARQ soft-combining on the MFSK16 sub-floor rung (SL1), driven through the daemon's decode entry.
//!
//! MFSK16 was held out of HARQ combining because every frame is one fixed 255-byte RS block, so
//! nothing could tell an abandoned message's retained LLRs from a retransmission of the live one —
//! worst case, combining could deliver the wrong message. The newest-first suffix trial in
//! `ota_decode_and_ack_inner` contains that hazard by construction (a stale message's bursts are
//! always older, so some suffix excludes them), which is what admits `Rs` to the soft path here.
//!
//! It earns its place on the number: MFSK16 is soft-capable, and `decode_combined_llrs` handles `Rs`
//! by hard-deciding the *combined* vector, so summing calibrated LLRs across independent fades pulls
//! the hard-decision error count under RS(255,223)'s 16-byte-per-block capacity. Measured on
//! `moderate_f1` over 60 realisations per arm, three bursts — roughly **+2.5 dB** of sub-floor
//! sensitivity:
//!
//! ```text
//! SNR    standalone  combining
//! -6.0      0.000      0.267     <- decodes where no single burst ever does
//! -5.0      0.067      0.583
//! -4.0      0.117      0.750
//! -3.0      0.417      0.933
//! -2.0      0.683      0.983
//! ```
//!
//! This is the rung the ChirpFallback path drops to under sustained failure, so the gain lands
//! exactly where the link has nothing else left.

use mfsk16_plugin::Mfsk16Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;

const MODE: &str = "MFSK16";
const SESSION: &str = "mfsk16-harq";
const PAYLOAD: &[u8] = b"MFSK16 HARQ sub-floor payload AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
/// A different message of the same length. Equal lengths are the whole point — every MFSK16 frame is
/// one fixed 255-byte RS block anyway, so nothing downstream can separate the two by size.
const STALE_PAYLOAD: &[u8] = b"Abandoned MFSK16 message, different bits BBBBBBBBBBBBBBBBBBBBB";
const TRIALS: u32 = 60;
const ATTEMPTS: usize = 3;
/// The knee of the sub-floor: standalone 0.117 vs combining 0.750, so the gain is unmistakable.
const SNR_DB: f32 = -4.0;
/// The abandoned message's bursts sit at the same operating point — retained, never decoding.
const STALE_SNR_DB: f32 = -4.0;

fn make() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(Mfsk16Plugin::new()))
        .expect("register");
    e.set_center_frequency(1500.0);
    // Lock the receiver-led controller to SL1 so `rx_candidates()` is exactly (MFSK16, Rs).
    e.start_ota_session(SessionProfile::hpx_hf());
    e.ota_lock_level(SpeedLevel::Sl1);
    (e, backend)
}

fn tx_samples_of(payload: &[u8]) -> Vec<f32> {
    let (mut e, backend) = make();
    e.transmit_with_fec(payload, MODE, None).expect("transmit");
    backend.drain_samples()
}

fn faded(tx: &[f32], seed: u64, snr_db: f32) -> Vec<f32> {
    let mut cfg = WattersonConfig::moderate_f1(Some(seed));
    cfg.snr_db = snr_db;
    WattersonChannel::new(cfg).expect("watterson").apply(tx)
}

fn seed(trial: u32, attempt: usize) -> u64 {
    770_000 + (trial as u64) * 10 + attempt as u64
}

fn decoded(rx: &mut ModemEngine, burst: &AudioSamples) -> Option<Vec<u8>> {
    rx.ota_decode_burst(burst, SESSION)
        .ok()
        .and_then(|r| r.payload)
}

/// Baseline: each burst decoded standalone by a fresh engine, so nothing is retained.
fn standalone_success(tx: &[f32]) -> f32 {
    let mut ok = 0u32;
    for trial in 0..TRIALS {
        for attempt in 0..ATTEMPTS {
            let (mut rx, _b) = make();
            let burst = AudioSamples {
                samples: faded(tx, seed(trial, attempt), SNR_DB),
            };
            if decoded(&mut rx, &burst).as_deref() == Some(PAYLOAD) {
                ok += 1;
                break;
            }
        }
    }
    ok as f32 / TRIALS as f32
}

/// Treatment: one engine across the bursts, so a failed burst's LLRs are retained and combined.
fn combining_success(tx: &[f32]) -> f32 {
    let mut ok = 0u32;
    for trial in 0..TRIALS {
        let (mut rx, _b) = make();
        for attempt in 0..ATTEMPTS {
            let burst = AudioSamples {
                samples: faded(tx, seed(trial, attempt), SNR_DB),
            };
            if decoded(&mut rx, &burst).as_deref() == Some(PAYLOAD) {
                ok += 1;
                break;
            }
        }
    }
    ok as f32 / TRIALS as f32
}

/// As `combining_success`, but preceded by bursts of a different message that never decoded.
/// Returns (success_rate, frames_delivered_that_were_not_the_live_message).
fn combining_after_stale(tx: &[f32], stale_tx: &[f32]) -> (f32, u32) {
    const PRELUDE: usize = 3;
    let (mut ok, mut wrong) = (0u32, 0u32);
    for trial in 0..TRIALS {
        let (mut rx, _b) = make();
        for attempt in 0..PRELUDE {
            let burst = AudioSamples {
                samples: faded(
                    stale_tx,
                    310_000 + trial as u64 * 10 + attempt as u64,
                    STALE_SNR_DB,
                ),
            };
            let _ = decoded(&mut rx, &burst);
        }
        for attempt in 0..ATTEMPTS {
            let burst = AudioSamples {
                samples: faded(tx, seed(trial, attempt), SNR_DB),
            };
            match decoded(&mut rx, &burst) {
                Some(p) if p == PAYLOAD => {
                    ok += 1;
                    break;
                }
                // Anything else handed up is a frame the sender is not transmitting any more.
                Some(_) => wrong += 1,
                None => {}
            }
        }
    }
    (ok as f32 / TRIALS as f32, wrong)
}

/// Combining retained LLRs across retransmissions must decode substantially more sub-floor frames
/// than deciding each burst alone. With MFSK16 held out of the soft path this delta is exactly
/// +0.000 (no combining happens at all), so the gate fails closed if the `Rs` admission regresses.
#[test]
fn mfsk16_harq_combining_adds_subfloor_diversity() {
    let tx = tx_samples_of(PAYLOAD);
    let standalone = standalone_success(&tx);
    let combining = combining_success(&tx);
    println!(
        "MFSK16 @{SNR_DB} dB: standalone={standalone:.3} combining={combining:.3} (delta {:+.3})",
        combining - standalone
    );
    assert!(
        combining > standalone + 0.30,
        "moderate_f1 @{SNR_DB} dB, {ATTEMPTS} bursts: combining {combining:.3} vs standalone \
         {standalone:.3} — MFSK16 must gain sub-floor diversity from retained-LLR MAP combining"
    );
}

/// The hazard that held MFSK16 out of HARQ: an abandoned message's LLRs are indistinguishable from a
/// retransmission's (one fixed 255-byte block either way). They must neither dilute the live
/// message's diversity nor — the worst case the audit named — be delivered as a frame in their own
/// right.
#[test]
fn mfsk16_stale_message_does_not_pollute_or_false_deliver() {
    assert_eq!(
        PAYLOAD.len(),
        STALE_PAYLOAD.len(),
        "the stale message must be the same length as the real one or this gate proves nothing"
    );
    let tx = tx_samples_of(PAYLOAD);
    let stale_tx = tx_samples_of(STALE_PAYLOAD);
    let clean = combining_success(&tx);
    let (polluted, wrong) = combining_after_stale(&tx, &stale_tx);
    println!(
        "MFSK16 @{SNR_DB} dB: clean={clean:.3} polluted={polluted:.3} (delta {:+.3}) wrong={wrong}",
        polluted - clean
    );
    assert_eq!(
        wrong, 0,
        "an abandoned message's retained LLRs were delivered as a frame — RS/CRC must gate every \
         combine, and the suffix trial must never hand up a stale message"
    );
    assert!(
        polluted >= clean - 0.04,
        "combining after an abandoned same-length message {polluted:.3} vs clean {clean:.3} — \
         stale retained LLRs must not dilute the sub-floor rung's diversity gain"
    );
}
