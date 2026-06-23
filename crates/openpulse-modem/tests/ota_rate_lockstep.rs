//! Over-the-air receiver-led adaptive rate-stepping: end-to-end lockstep through
//! a loopback channel, including injected ACK loss.
//!
//! Wires the real `OtaRateController` into `respond_arq_ota` / `apply_ota_ack`
//! over two `ModemEngine`s bridged by `LoopbackBackend`s. Proves:
//! - the absolute `recommended_level` flows over the real FSK4 + short-FEC ACK;
//! - the rate climbs on a clean channel (SL2 → top of the hpx500 ladder);
//! - a dropped ACK never desyncs — the IRS's 2-mode fallback still decodes, and
//!   the climb simply pauses a frame.

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;
use qpsk_plugin::QpskPlugin;

fn make_engine() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    engine.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    engine.start_ota_session(SessionProfile::hpx500());
    (engine, backend)
}

fn route(src: &LoopbackBackend, dst: &LoopbackBackend) {
    dst.fill_samples(&src.drain_samples());
}

/// Run `frames` ISS→IRS exchanges. `ack_delivered(i)` decides whether the reverse
/// ACK for frame `i` reaches the ISS. `rx_snr_db` is the SNR the IRS uses for its
/// receiver-led decision (fed externally — the LLR proxy is too weak on loopback).
/// Returns the ISS TX level after each frame.
fn run_exchange(
    frames: usize,
    rx_snr_db: f32,
    ack_delivered: impl Fn(usize) -> bool,
) -> Vec<SpeedLevel> {
    run_exchange_cfg(frames, rx_snr_db, ack_delivered, |_iss, _irs| {})
}

/// As [`run_exchange`], but `configure` runs against both engines after the OTA
/// session starts (e.g. to clamp or lock the ladder).
fn run_exchange_cfg(
    frames: usize,
    rx_snr_db: f32,
    ack_delivered: impl Fn(usize) -> bool,
    configure: impl Fn(&mut ModemEngine, &mut ModemEngine),
) -> Vec<SpeedLevel> {
    let payload = b"over-the-air adaptive rate-stepping lockstep payload";
    let session = "ota-lockstep";
    let (mut iss, iss_lb) = make_engine();
    let (mut irs, irs_lb) = make_engine();
    irs.set_rx_snr_estimate(Some(rx_snr_db));
    configure(&mut iss, &mut irs);

    let mut tx_levels = Vec::with_capacity(frames);
    for i in 0..frames {
        // ISS transmits at its current OTA TX mode.
        let tx_mode = iss.ota_tx_mode().expect("OTA session active").to_owned();
        iss.transmit(payload, &tx_mode, None).unwrap();
        route(&iss_lb, &irs_lb);

        // IRS decodes with the candidate fallback and ACKs with a recommendation.
        let rx = irs
            .respond_arq_ota(session, None)
            .expect("IRS must always decode — the candidate set covers the sender's mode");
        assert_eq!(&rx, payload, "payload corrupted at frame {i}");

        // Reverse ACK path — maybe dropped.
        if ack_delivered(i) {
            route(&irs_lb, &iss_lb);
            let ack = iss
                .receive_ack_with_short_fec(None)
                .expect("ISS should receive the ACK");
            iss.apply_ota_ack(&ack);
        } else {
            iss_lb.drain_samples(); // ACK lost: discard whatever the IRS sent back
            irs_lb.drain_samples();
        }
        tx_levels.push(iss.ota_tx_level().unwrap());
    }
    tx_levels
}

// 30 dB is above every hpx500 ceiling (max 18 dB at SL6) → the ladder should
// climb to the top rung.
const GOOD_SNR: f32 = 30.0;

#[test]
fn climbs_on_clean_channel_no_loss() {
    let levels = run_exchange(12, GOOD_SNR, |_| true);
    let initial = SpeedLevel::Sl2;
    let last = *levels.last().unwrap();
    assert!(
        last > initial,
        "rate should climb above the initial level on a clean channel; got {levels:?}"
    );
    // hpx500 tops out at SL6 (QPSK500); with good SNR it should reach it.
    assert_eq!(
        last,
        SpeedLevel::Sl6,
        "should reach the top of the ladder: {levels:?}"
    );
    // Monotonic non-decreasing climb.
    for w in levels.windows(2) {
        assert!(
            w[1] >= w[0],
            "TX level must not drop on a clean channel: {levels:?}"
        );
    }
}

#[test]
fn climbs_using_the_builtin_m2m4_snr_estimator() {
    // Clear the external SNR seam so the engine must derive SNR itself (M2M4 on the
    // captured envelope). Unlike the old mean-|LLR| proxy (≈ −2 dB → no climb), the
    // M2M4 estimate of the real loopback frame reads ~10 dB — a realistic finite
    // value (frame transitions are not a pure constant-modulus tone) — which drives
    // the ladder up to a mid rung without any externally-supplied estimate.
    let levels = run_exchange_cfg(
        12,
        0.0,
        |_| true,
        |_iss, irs| {
            irs.set_rx_snr_estimate(None);
        },
    );
    assert!(
        *levels.last().unwrap() >= SpeedLevel::Sl4,
        "M2M4 estimator should drive a meaningful climb on a clean channel: {levels:?}"
    );
}

#[test]
fn never_desyncs_with_periodic_ack_loss() {
    // Every 3rd ACK is lost. respond_arq_ota's `.expect()` inside run_exchange
    // fires if a desync ever causes a decode failure — so reaching the end proves
    // the lockstep invariant held over the real wire.
    let levels = run_exchange(20, GOOD_SNR, |i| i % 3 != 0);
    assert!(
        *levels.last().unwrap() > SpeedLevel::Sl2,
        "should still climb despite periodic ACK loss: {levels:?}"
    );
}

#[test]
fn max_level_bound_caps_the_climb_over_the_wire() {
    // Cap the ladder at SL4 (BPSK250); the IRS leads, so it must clamp its
    // recommendation, and the ISS must never be driven past SL4.
    let levels = run_exchange_cfg(
        12,
        GOOD_SNR,
        |_| true,
        |iss, irs| {
            iss.ota_set_level_bounds(None, Some(SpeedLevel::Sl4));
            irs.ota_set_level_bounds(None, Some(SpeedLevel::Sl4));
        },
    );
    let last = *levels.last().unwrap();
    assert!(
        last > SpeedLevel::Sl2,
        "should still climb up to the cap: {levels:?}"
    );
    assert!(
        levels.iter().all(|&l| l <= SpeedLevel::Sl4),
        "TX level must never exceed the max bound: {levels:?}"
    );
    assert_eq!(
        last,
        SpeedLevel::Sl4,
        "should climb to and hold at the cap: {levels:?}"
    );
}

#[test]
fn locked_level_holds_over_the_wire() {
    // Lock both ends at SL3; the rate must never move despite good SNR.
    let levels = run_exchange_cfg(
        8,
        GOOD_SNR,
        |_| true,
        |iss, irs| {
            iss.ota_lock_level(SpeedLevel::Sl3);
            irs.ota_lock_level(SpeedLevel::Sl3);
        },
    );
    assert!(
        levels.iter().all(|&l| l == SpeedLevel::Sl3),
        "locked link must hold its level: {levels:?}"
    );
}

#[test]
fn transmit_arq_ota_errors_without_session() {
    // A bare engine with no OTA session must reject transmit_arq_ota.
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    e.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    assert!(e.transmit_arq_ota(b"hello", None, 0).is_err());
}

#[test]
fn transmit_arq_ota_exhausts_retries_without_ack_responder() {
    // OTA session active but no peer answers: each attempt transmits then fails to
    // decode an FSK4 ACK (its loopback holds the data frame, not an ACK) → retries
    // exhaust and the call errors. (The happy path needs a real ACK responder/peer.)
    let (mut iss, _lb) = make_engine();
    assert!(iss
        .transmit_arq_ota(b"over-the-air payload", None, 2)
        .is_err());
}

#[test]
fn poll_ota_rx_idle_window_returns_none() {
    // No samples in the loopback → the idle gate must suppress the ACK so the
    // daemon never keys PTT to answer silence.
    let (mut irs, _lb) = make_engine();
    let res = irs.poll_ota_rx("ota", None).expect("poll must not error");
    assert!(
        res.is_none(),
        "an idle window must not produce an ACK to send"
    );
}

#[test]
fn poll_ota_rx_decodes_and_yields_ack_to_transmit() {
    // The daemon split: poll_ota_rx decodes WITHOUT transmitting, returns the ACK
    // for the caller to key PTT around. Transmitting it separately reproduces the
    // same receiver-led climb as respond_arq_ota.
    let payload = b"poll-ota-rx split path payload";
    let (mut iss, iss_lb) = make_engine();
    let (mut irs, irs_lb) = make_engine();
    irs.set_rx_snr_estimate(Some(GOOD_SNR));

    for i in 0..6 {
        let tx_mode = iss.ota_tx_mode().unwrap().to_owned();
        iss.transmit(payload, &tx_mode, None).unwrap();
        route(&iss_lb, &irs_lb);

        let res = irs
            .poll_ota_rx("ota", None)
            .expect("poll must not error")
            .expect("an energetic window must decode-attempt");
        assert_eq!(
            res.payload.as_deref(),
            Some(&payload[..]),
            "payload corrupted at frame {i}"
        );
        // Caller transmits the ACK the poll built (PTT keyed around this on radio).
        irs.transmit_ack_with_short_fec(&res.ack, None).unwrap();
        route(&irs_lb, &iss_lb);
        let ack = iss.receive_ack_with_short_fec(None).unwrap();
        iss.apply_ota_ack(&ack);
    }
    assert!(
        iss.ota_tx_level().unwrap() > SpeedLevel::Sl2,
        "the split poll path should still climb on a clean channel"
    );
}

#[test]
fn never_desyncs_with_every_other_ack_lost() {
    let levels = run_exchange(20, GOOD_SNR, |i| i % 2 == 0);
    // Still climbs (slower), and crucially never errors on decode.
    assert!(
        *levels.last().unwrap() > SpeedLevel::Sl2,
        "should make progress even at 50% ACK loss: {levels:?}"
    );
}
