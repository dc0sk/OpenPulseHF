//! Which decode arm the daemon dispatches to decides whether an UNCODED frame can be received.
//!
//! `server::run`'s rx_ticker has two mutually-exclusive arms for a flushed burst: with an OTA
//! session active it calls `ota_decode_burst`, otherwise `decode_burst`. `ota_decode_burst` tries
//! only the current rung's candidates — at most two `(mode, FEC)` pairs (`ota_rate.rs:353`) — each
//! carrying `profile.fec_for(level)`. Under the default `hpx_hf` every rung is coded, so an uncoded
//! frame matches no candidate whatever its mode. The daemon nevertheless transmits uncoded frames on
//! several paths that have nothing to do with the OTA data ladder (station ID, filexfer fragments,
//! handshake CONREQ/CONACK, QSY frames, relay envelopes).
//!
//! Note this is a property of the profile's FEC table, not of profiles in general: `fec_for` is
//! `fec_modes[level].unwrap_or(FecMode::None)` (`profile.rs:110`), and only `hpx_modcod`, `hpx_hf`
//! and `hpx_ofdm_hf` populate that table at all — the rest yield uncoded candidates that still only
//! cover the ladder's own modes at the current rung.
//!
//! This isolates the arm as the single variable: ONE burst, gathered through the daemon's own
//! capture entry, decoded both ways. The `decode_burst` cell is the positive control — it proves
//! the burst is intact, correctly framed and locatable, so a failure in the `ota_decode_burst` cell
//! cannot be blamed on chunking, DCD boundaries, the front-end seam or mode mismatch.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;
use openpulse_modem::EngineEvent;

/// hpx_hf SL5 is `BPSK250` + `Rs`. Locking the OTA session here makes the OTA arm's candidate
/// **mode** identical to the transmitted one, so the only remaining difference is the FEC the
/// candidate carries. Without the lock a fresh session would offer SL2/BPSK31 and the result would
/// be overdetermined by mode mismatch.
const LOCK_LEVEL: SpeedLevel = SpeedLevel::Sl5;
const MODE: &str = "BPSK250";
const PAYLOAD: &[u8] = b"UNCODED CONTROL FRAME";
const SESSION: &str = "arm-dispatch";
/// The daemon's receive tick is 100 ms; at 8 kHz that is 800 samples per `accumulate_capture` call.
const TICK_SAMPLES: usize = 800;

fn engine_with(backend: &LoopbackBackend) -> ModemEngine {
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    engine
}

/// Drive `engine` the way `server::run`'s rx_ticker does: tick-sized chunks into
/// `accumulate_capture`, with trailing silence so the carrier drops and the burst flushes.
/// `accumulate_routed` never flushes at end-of-input, so the trailing quiet is load-bearing.
fn capture_via_daemon_path(engine: &mut ModemEngine, signal: &[f32]) -> Option<AudioSamples> {
    let quiet = vec![0.0f32; TICK_SAMPLES];
    let mut flushed = None;
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
    for _ in 0..4 {
        if let Ok(Some(b)) = engine.accumulate_capture(Some(MODE), quiet.clone()) {
            flushed = Some(b);
        }
    }
    flushed
}

/// The uncoded audio the daemon's non-OTA transmit paths put on the air (`engine.transmit`).
fn uncoded_tx_samples() -> Vec<f32> {
    let backend = LoopbackBackend::new();
    let mut engine = engine_with(&backend);
    engine.transmit(PAYLOAD, MODE, None).expect("transmit");
    backend.drain_samples()
}

#[test]
fn one_burst_two_arms() {
    let signal = uncoded_tx_samples();
    assert!(!signal.is_empty(), "the transmit produced no audio");

    // Gather the burst exactly once, through the production capture entry, with the OTA session
    // active — the state the daemon is in whenever `ota_enabled = true`.
    let backend = LoopbackBackend::new();
    let mut engine = engine_with(&backend);
    engine.start_ota_session(SessionProfile::hpx_hf());
    engine.ota_lock_level(LOCK_LEVEL);
    assert!(engine.ota_active(), "the OTA session must be active");

    let burst = capture_via_daemon_path(&mut engine, &signal)
        .expect("the daemon capture entry must flush a burst");

    // Arm 1 — OTA session active: `server.rs` dispatches here.
    let ota = engine
        .ota_decode_burst(&burst, SESSION, Some(MODE))
        .expect("ota_decode_burst must not error");
    let ota_payload = ota.payload.clone();
    assert!(
        ota.ack.is_none(),
        "non-ladder traffic must produce no ACK — an ACK here would key the transmitter for a \
         frame nobody is waiting on, and would credit the rate ladder for a decode it did not make"
    );

    // Arm 2 — no OTA session: the same burst, same mode, same engine. Positive control.
    //
    // `ota_decode_and_ack_inner` does not restore `afc_correction_hz` after its final failed
    // candidate, so arm 1 leaves AFC state behind. That pollution biases AGAINST the control (it
    // can only make the decode harder), so the result would be conservative either way — but clear
    // it so the isolation does not rest on that argument.
    engine.stop_ota_session();
    engine.reset_afc();
    let plain = engine.decode_burst(MODE, &burst);

    eprintln!(
        "MEASURED burst={} samples | ota_decode_burst payload={:?} | decode_burst={:?}",
        burst.samples.len(),
        ota_payload.as_ref().map(|p| String::from_utf8_lossy(p).to_string()),
        plain
            .as_ref()
            .map(|p| String::from_utf8_lossy(p).to_string())
            .map_err(|e| e.to_string()),
    );

    assert_eq!(
        plain.as_deref().ok(),
        Some(PAYLOAD),
        "positive control: the burst must decode through the non-OTA arm"
    );
    assert_eq!(
        ota_payload.as_deref(),
        Some(PAYLOAD),
        "the same burst must also decode with an OTA session active"
    );
}

/// Recovering non-ladder traffic must not touch the rate controller.
///
/// This is the half a decode-or-not assertion is structurally blind to, and the half that was
/// silently wrong on `main` before #1123: an uncoded frame counted as a decode FAILURE, so
/// `on_rx_frame(RxOutcome::Failed, ..)` ran on every heard station ID, filexfer fragment, QSY frame
/// and relay envelope. That path has a **hysteresis-free** demotion — it fast-downshifts on a
/// single failure whenever `level_for_snr(snr) < rx_recommended`, with `snr` measured in the
/// candidate rung's domain on audio that is not that rung — and it resets `rx_consecutive_ok`,
/// which is the only climb path when the SNR estimate is uninformative (#934).
///
/// The `OtaRateDecision` assertion is the load-bearing one: the event is emitted on EVERY
/// controller decision including ones that move nothing (#1081), so its absence proves
/// `on_rx_frame` was never called at all — which is the only way to cover the private
/// `rx_consecutive_ok` streak reset.
#[test]
fn a_control_frame_does_not_touch_the_rate_controller() {
    let signal = uncoded_tx_samples();
    let backend = LoopbackBackend::new();
    let mut engine = engine_with(&backend);
    engine.start_ota_session(SessionProfile::hpx_hf());
    engine.ota_lock_level(LOCK_LEVEL);

    let before_recommended = engine.ota_rx_recommended_level();
    let before_confirmed = engine.ota_rx_confirmed_level();
    let mut events = engine.subscribe();

    let burst = capture_via_daemon_path(&mut engine, &signal).expect("a burst must flush");
    let res = engine
        .ota_decode_burst(&burst, SESSION, Some(MODE))
        .expect("must not error");

    assert_eq!(
        res.payload.as_deref(),
        Some(PAYLOAD),
        "precondition: the control frame must be recovered"
    );
    assert!(res.ack.is_none(), "a control frame must produce no ACK");

    let decisions: Vec<_> = std::iter::from_fn(|| events.try_recv().ok())
        .filter(|e| matches!(e, EngineEvent::OtaRateDecision { .. }))
        .collect();
    assert!(
        decisions.is_empty(),
        "a control frame must drive NO controller decision; got {decisions:?}"
    );
    assert_eq!(
        engine.ota_rx_recommended_level(),
        before_recommended,
        "the recommended level must not move for non-ladder traffic"
    );
    assert_eq!(
        engine.ota_rx_confirmed_level(),
        before_confirmed,
        "the confirmed level must not move for non-ladder traffic"
    );
}

/// `fallback_mode = None` must behave exactly as before #1123 — no uncoded attempt at all.
///
/// This pins the documented darkness of the two sibling entries (`poll_ota_rx`,
/// `respond_arq_ota`), which pass `None`. Without this gate that behaviour is implicit, and a
/// later change could switch them on — or off — with nothing noticing.
#[test]
fn fallback_mode_none_changes_nothing() {
    let signal = uncoded_tx_samples();
    let backend = LoopbackBackend::new();
    let mut engine = engine_with(&backend);
    engine.start_ota_session(SessionProfile::hpx_hf());
    engine.ota_lock_level(LOCK_LEVEL);

    let burst = capture_via_daemon_path(&mut engine, &signal).expect("a burst must flush");
    let res = engine
        .ota_decode_burst(&burst, SESSION, None)
        .expect("must not error");

    assert!(
        res.payload.is_none(),
        "with no fallback mode the uncoded frame must not be recovered (pre-#1123 behaviour)"
    );
    assert!(
        res.ack.is_some(),
        "a failed decode is ladder evidence and must still carry its Nack"
    );
}

/// A ladder frame keeps first claim on the burst, even where the fallback could also decode it.
///
/// `hpx500` populates no FEC table, so `fec_for` returns `FecMode::None` for every rung
/// (`profile.rs`) and its candidates are themselves uncoded. With the active mode equal to a rung
/// mode, a ladder frame and a control frame are literally indistinguishable on the wire — so this
/// is the ONLY fixture where the candidates-before-fallback ordering is observable. Under `hpx_hf`
/// (every rung coded) reordering would be undetectable by construction.
#[test]
fn a_ladder_frame_still_classifies_as_ladder_when_the_fallback_could_also_decode_it() {
    let backend = LoopbackBackend::new();
    let mut tx = engine_with(&backend);
    tx.transmit(PAYLOAD, "BPSK31", None).expect("transmit");
    let signal = backend.drain_samples();

    let rx_backend = LoopbackBackend::new();
    let mut engine = engine_with(&rx_backend);
    // hpx500's SL2 rung is BPSK31 + no FEC; the fallback mode is that same mode.
    engine.start_ota_session(SessionProfile::hpx500());
    let mut events = engine.subscribe();

    let quiet = vec![0.0f32; TICK_SAMPLES];
    let mut burst = None;
    for chunk in signal.chunks(TICK_SAMPLES) {
        if let Ok(Some(b)) = engine.accumulate_capture(Some("BPSK31"), chunk.to_vec()) {
            burst = Some(b);
        }
    }
    for _ in 0..4 {
        if let Ok(Some(b)) = engine.accumulate_capture(Some("BPSK31"), quiet.clone()) {
            burst = Some(b);
        }
    }
    let burst = burst.expect("a burst must flush");

    let res = engine
        .ota_decode_burst(&burst, SESSION, Some("BPSK31"))
        .expect("must not error");
    assert_eq!(
        res.payload.as_deref(),
        Some(PAYLOAD),
        "precondition: the frame must decode at all"
    );
    assert!(
        res.ack.is_some(),
        "a frame a RUNG CANDIDATE can decode is ladder traffic and must carry an ACK — the \
         fallback must not claim it first"
    );
    let decisions: Vec<_> = std::iter::from_fn(|| events.try_recv().ok())
        .filter(|e| matches!(e, EngineEvent::OtaRateDecision { .. }))
        .collect();
    assert!(
        !decisions.is_empty(),
        "a ladder decode must drive a controller decision"
    );
}
