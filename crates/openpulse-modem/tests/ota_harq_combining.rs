//! HARQ soft-combining across OTA retransmissions, driven through the daemon's decode entry
//! (`ota_decode_burst`).
//!
//! `receive_with_llr_combining` (the #694 union) is synchronous multi-capture and RS-only, so it
//! never fit the daemon's async, per-MODCOD OTA flow — the diversity gain it measured never reached
//! the air. This wires it in: `ota_decode_and_ack` now retains the soft LLRs of a *failed* burst,
//! keyed by `(session, mode)`, and MAP-combines them with the next burst of the same mode before
//! giving up. On `moderate_f1` near a rung's threshold each burst is a partially-ruined observation of
//! the same bits; summing their calibrated LLRs across independent fade realisations decodes frames no
//! single burst can. Exercised here on SL9 (`OFDM52-16QAM` + SoftConcatenated on the fade-aware ladder).
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
/// A *different* message of the same length as `PAYLOAD` — so its LLR vector is the same length
/// and the length filter alone cannot tell the two frames apart.
const STALE_PAYLOAD: &[u8] = b"Abandoned message, same length, different bits BBBBBBBBBBBBBB";
const MODE: &str = "OFDM52-16QAM"; // hpx_hf SL9 = OFDM52-16QAM + SoftConcatenated (fade-aware re-seat)
const FEC: FecMode = FecMode::SoftConcatenated;
const SESSION: &str = "harq-sess";
/// Fade realisations per arm. 50 keeps CI cheap but leaves ±0.06 of binomial noise — enough to hide
/// the pollution these gates measure. Rebuild with `HARQ_TRIALS=1 cargo test …` (a compile-time env
/// var, so it needs a rebuild, and only its presence is read) for the 400-trial measurement the
/// numbers quoted here came from.
const TRIALS: u32 = if option_env!("HARQ_TRIALS").is_some() {
    400
} else {
    50
};
const ATTEMPTS: usize = 3;
const SNR_DB: f32 = 10.0;
/// SNR for the abandoned message's bursts — the worst case for pollution, measured by sweeping it.
/// A burst must *demodulate* before its LLRs are retained, so far below the rung (0 dB) nothing is
/// kept and there is nothing to pollute with; far above, the stale message decodes and clears the
/// buffer itself. In between, every stale burst is retained and none of them clear: that is where a
/// sender gives up, and where the damage was worst (-0.067 at 6 dB before the suffix trial).
const STALE_SNR_DB: f32 = 6.0;

fn make() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .expect("register");
    // Lock the receiver-led OTA controller to SL9 so `rx_candidates()` is exactly
    // (OFDM52-16QAM, SoftConcatenated) — the soft rung HARQ combining acts on.
    engine.start_ota_session(SessionProfile::hpx_hf());
    engine.ota_lock_level(SpeedLevel::Sl9);
    (engine, backend)
}

fn tx_samples_of(payload: &[u8]) -> Vec<f32> {
    let (mut engine, backend) = make();
    engine
        .transmit_with_fec_mode(payload, MODE, FEC, None)
        .expect("transmit");
    backend.drain_samples()
}

fn tx_samples() -> Vec<f32> {
    tx_samples_of(PAYLOAD)
}

/// An independent Watterson `moderate_f1` realisation (1 ms delay spread, 1 Hz Doppler).
fn faded_at(tx: &[f32], seed: u64, snr_db: f32) -> Vec<f32> {
    let mut cfg = WattersonConfig::moderate_f1(Some(seed));
    cfg.snr_db = snr_db;
    WattersonChannel::new(cfg).expect("watterson").apply(tx)
}

fn faded(tx: &[f32], seed: u64) -> Vec<f32> {
    faded_at(tx, seed, SNR_DB)
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

/// Same as `combining_success`, but the engine first sees `PRELUDE` bursts of a *different*
/// message that never decoded — the abandoned-message case. Those LLRs stay retained (same mode,
/// same length), so without a frame-identity guard they MAP-combine into this message's bursts and
/// evict its own good LLRs from the bounded buffer.
fn combining_success_after_stale(tx: &[f32], stale_tx: &[f32], stale_snr_db: f32) -> f32 {
    const PRELUDE: usize = 3;
    let mut ok = 0u32;
    for trial in 0..TRIALS {
        let (mut rx, _backend) = make();
        for attempt in 0..PRELUDE {
            let burst = AudioSamples {
                // A seed space far from `seed()`'s so the abandoned message's fade realisations
                // are independent of the real message's — an overlap would correlate the two.
                samples: faded_at(
                    stale_tx,
                    500_000 + trial as u64 * 10 + attempt as u64,
                    stale_snr_db,
                ),
            };
            let _ = rx.ota_decode_burst(&burst, SESSION);
        }
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

/// An abandoned message's retained LLRs must not degrade the next message's HARQ combining.
///
/// The daemon pins `session_id` to the local callsign, so the per-session guard never fires there;
/// isolation rests entirely on the LLR-vector length, which two same-length messages share. This
/// gate is the reason `ota_retained_llrs` needs a frame-identity test (audit 2026-07-16 #4).
#[test]
fn stale_message_llrs_do_not_pollute_the_next_message() {
    // Equal payload lengths are what make the two messages indistinguishable to the length filter.
    // Unequal ones would make this gate vacuous — the filter would reject the stale LLRs for us.
    assert_eq!(
        PAYLOAD.len(),
        STALE_PAYLOAD.len(),
        "the stale message must be the same length as the real one or this test proves nothing"
    );
    let tx = tx_samples();
    let stale_tx = tx_samples_of(STALE_PAYLOAD);
    let clean = combining_success(&tx);
    let polluted = combining_success_after_stale(&tx, &stale_tx, STALE_SNR_DB);
    println!(
        "clean={clean:.3} polluted={polluted:.3} (delta {:+.3})",
        polluted - clean
    );
    // Both arms decode the same real bursts from the same seeds, so this is a paired comparison:
    // with the suffix trial working the two are *identical*, and the tolerance only absorbs the
    // stale bursts that decode and perturb the retained state. Before the fix this measured -0.055
    // at 50 trials and -0.067 at 400.
    assert!(
        polluted >= clean - 0.04,
        "moderate_f1 @{SNR_DB} dB: combining after an abandoned same-length message {polluted:.3} \
         vs clean {clean:.3} — stale retained LLRs must not dilute the next message's diversity gain"
    );
}

/// Retaining and combining failed-burst LLRs across retransmissions must decode strictly more frames
/// than deciding each burst independently — the diversity gain HARQ combining exists to capture.
///
/// The margin also guards how retained bursts are *aligned* onto the current one. A faded demod
/// recovers a varying symbol count for the same frame, so the equality filter this replaced silently
/// dropped most retained vectors and most of the gain with them: it measures +0.280 here against the
/// +0.400 of truncate/zero-pad alignment. A threshold loose enough to pass both (the original +0.08)
/// cannot see that regression, so it is set between them.
#[test]
fn ota_retention_combines_across_retransmissions() {
    let tx = tx_samples();
    let standalone = standalone_success(&tx);
    let combining = combining_success(&tx);
    println!(
        "standalone={standalone:.3} combining={combining:.3} (delta {:+.3})",
        combining - standalone
    );
    assert!(
        combining > standalone + 0.35,
        "moderate_f1 @{SNR_DB} dB, {ATTEMPTS} bursts: combining {combining:.3} vs standalone \
         {standalone:.3} — retained-LLR MAP combining across OTA retransmissions must add diversity \
         gain, and must align mismatched-length retained bursts rather than discard them"
    );
}
