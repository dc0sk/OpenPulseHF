//! OTA receive — including HARQ combining — driven through the daemon's **production** capture
//! entry, not the convenience seam.
//!
//! Captured audio reaches the OTA decoder two different ways. Every existing OTA test hands a
//! perfectly-trimmed `AudioSamples` straight to `ota_decode_burst`. The running daemon never does
//! that: `server::run`'s `rx_ticker` feeds *tick-sized chunks* to `accumulate_capture`, which routes
//! them through the `InputCapture` seam, gathers them under DCD gating, and flushes a burst only when
//! the carrier drops — so a real burst's boundaries are decided by the DCD, and it carries whatever
//! leading/trailing channel audio the squelch let through.
//!
//! That gap is not hypothetical: it is exactly what let the audit's finding #3 (the OTA decode loop
//! re-running the InputCapture front-end) ship unnoticed, and CLAUDE.md's cross-cutting RX checklist
//! calls for a test through the production entry for precisely this reason. These tests close it for
//! the OTA path, and in doing so put the HARQ retention/combining work (#920/#921/#922) on the same
//! footing — that machinery only ever pays off on the daemon's async bursts.

use ofdm_plugin::OfdmPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;

const PAYLOAD: &[u8] = b"OTA burst through the real daemon capture entry AAAAAAAAAAAA";
const MODE: &str = "OFDM52-16QAM"; // hpx_hf SL9 + SoftConcatenated — the soft rung HARQ acts on
const FEC: FecMode = FecMode::SoftConcatenated;
const SESSION: &str = "prod-path";
/// The daemon's default receive tick is 100 ms; at 8 kHz that is 800 samples per `accumulate_capture`
/// call. Feeding one big slice would skip the very chunking this test exists to exercise.
const TICK_SAMPLES: usize = 800;
const TRIALS: u32 = 30;
const ATTEMPTS: usize = 3;
const SNR_DB: f32 = 10.0;

fn make() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine
        .register_plugin(Box::new(OfdmPlugin::new()))
        .expect("register");
    engine.start_ota_session(SessionProfile::hpx_hf());
    engine.ota_lock_level(SpeedLevel::Sl9);
    (engine, backend)
}

fn tx_samples() -> Vec<f32> {
    let (mut engine, backend) = make();
    engine
        .transmit_with_fec_mode(PAYLOAD, MODE, FEC, None)
        .expect("transmit");
    backend.drain_samples()
}

fn faded(tx: &[f32], seed: u64) -> Vec<f32> {
    let mut cfg = WattersonConfig::moderate_f1(Some(seed));
    cfg.snr_db = SNR_DB;
    WattersonChannel::new(cfg).expect("watterson").apply(tx)
}

/// Drive `engine` the way `server::run`'s `rx_ticker` does: silence, the signal in tick-sized
/// chunks, then silence to drop the carrier and flush. Returns the burst `accumulate_capture`
/// handed back — the exact value the daemon passes to `ota_decode_burst`.
fn capture_via_daemon_path(engine: &mut ModemEngine, signal: &[f32]) -> Option<AudioSamples> {
    let quiet = vec![0.0f32; TICK_SAMPLES];
    let mut flushed = None;
    // Lead-in silence: no carrier, nothing should flush.
    for _ in 0..2 {
        if let Ok(Some(b)) = engine.accumulate_capture(Some(MODE), quiet.clone()) {
            flushed = Some(b);
        }
    }
    for chunk in signal.chunks(TICK_SAMPLES) {
        if let Ok(Some(b)) = engine.accumulate_capture(Some(MODE), chunk.to_vec()) {
            flushed = Some(b);
        }
    }
    // Trailing silence: the carrier drops and the accumulated burst is flushed.
    for _ in 0..4 {
        if let Ok(Some(b)) = engine.accumulate_capture(Some(MODE), quiet.clone()) {
            flushed = Some(b);
        }
    }
    flushed
}

/// A burst gathered by the daemon's own capture entry must decode. If the chunking, the DCD-decided
/// boundaries, or the front-end seam mangle it, this fails where the direct-`AudioSamples` tests
/// cannot.
#[test]
fn a_burst_gathered_by_accumulate_capture_decodes() {
    let tx = tx_samples();
    let (mut rx, _b) = make();
    let burst = capture_via_daemon_path(&mut rx, &tx).expect(
        "accumulate_capture must gather and flush a burst when the carrier rises and drops — \
         no flush means the daemon would never call ota_decode_burst at all",
    );
    // The DCD decides the boundaries, so the burst is not the transmitted slice; it must still carry
    // the whole frame.
    assert!(
        burst.samples.len() >= tx.len(),
        "flushed burst ({} samples) is shorter than the transmitted frame ({}) — the DCD trimmed \
         signal away",
        burst.samples.len(),
        tx.len()
    );
    let out = rx
        .ota_decode_burst(&burst, SESSION)
        .expect("decode call")
        .payload;
    assert_eq!(
        out.as_deref(),
        Some(PAYLOAD),
        "a clean burst captured through the daemon's production entry must decode"
    );
}

/// Run `TRIALS` trials of `ATTEMPTS` bursts through the production entry.
///
/// `retain`: false = a fresh engine per burst (standalone baseline, nothing retained); true = one
/// engine across the trial's bursts, exactly as the daemon holds one engine across rx ticks, so
/// failed bursts' LLRs are retained and combined.
///
/// Returns (success_rate, no_flush_count). A burst the DCD never flushed is a genuine failed
/// attempt — the daemon heard nothing — so it counts as failure rather than being skipped, but it is
/// reported so this can never quietly become a test of nothing.
fn run(tx: &[f32], retain: bool) -> (f32, u32) {
    let (mut ok, mut no_flush) = (0u32, 0u32);
    for trial in 0..TRIALS {
        let mut held = retain.then(make);
        for attempt in 0..ATTEMPTS {
            let mut fresh = (!retain).then(make);
            let rx = match (&mut held, &mut fresh) {
                (Some((e, _)), _) | (_, Some((e, _))) => e,
                _ => unreachable!("exactly one of held/fresh is Some"),
            };
            let signal = faded(tx, 4_100 + trial as u64 * 10 + attempt as u64);
            let Some(burst) = capture_via_daemon_path(rx, &signal) else {
                no_flush += 1;
                continue;
            };
            if rx
                .ota_decode_burst(&burst, SESSION)
                .ok()
                .and_then(|r| r.payload)
                .as_deref()
                == Some(PAYLOAD)
            {
                ok += 1;
                break;
            }
        }
    }
    (ok as f32 / TRIALS as f32, no_flush)
}

/// The HARQ diversity gain must survive the production capture entry.
///
/// `ota_harq_combining.rs` proves the gain exists when bursts are handed straight to
/// `ota_decode_burst`. That is not what the daemon does, and the retained LLRs are demodulated from
/// whatever `accumulate_capture` flushes — DCD-decided boundaries, front-end seam applied. If the
/// chunked path produced bursts framed inconsistently from tick to tick, retention would silently
/// stop combining and the gain would never reach the air.
#[test]
fn harq_combining_survives_the_production_capture_entry() {
    let tx = tx_samples();
    let (standalone, sa_no_flush) = run(&tx, false);
    let (combining, co_no_flush) = run(&tx, true);
    println!(
        "production path @{SNR_DB} dB: standalone={standalone:.3} combining={combining:.3} \
         (delta {:+.3}) no_flush={sa_no_flush}/{co_no_flush}",
        combining - standalone
    );
    assert!(
        combining > standalone + 0.15,
        "moderate_f1 @{SNR_DB} dB, {ATTEMPTS} bursts through accumulate_capture: combining \
         {combining:.3} vs standalone {standalone:.3} — the HARQ diversity gain must reach the \
         daemon's real receive path, not just the direct ota_decode_burst seam"
    );
}
