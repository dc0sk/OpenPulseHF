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
    let payload = b"over-the-air adaptive rate-stepping lockstep payload";
    let session = "ota-lockstep";
    let (mut iss, iss_lb) = make_engine();
    let (mut irs, irs_lb) = make_engine();
    irs.set_rx_snr_estimate(Some(rx_snr_db));

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
fn never_desyncs_with_every_other_ack_lost() {
    let levels = run_exchange(20, GOOD_SNR, |i| i % 2 == 0);
    // Still climbs (slower), and crucially never errors on decode.
    assert!(
        *levels.last().unwrap() > SpeedLevel::Sl2,
        "should make progress even at 50% ACK loss: {levels:?}"
    );
}
