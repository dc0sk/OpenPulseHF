//! MODCOD ladder: OTA adaptive stepping that adapts modulation AND FEC together.
//!
//! The `hpx_modcod` profile interleaves FEC rungs between modulation steps
//! (BPSK250+LDPC → BPSK250+RS → QPSK250+LDPC → QPSK250+RS → QPSK500+RS →
//! QPSK500). This proves the OTA engine applies the per-level FEC on both TX
//! (`transmit_with_fec_mode`) and RX (the candidate fallback via `decode_attempt`),
//! and that the receiver-led ladder climbs across modulation+FEC rungs over a real
//! loopback channel.

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
    engine.start_ota_session(SessionProfile::hpx_modcod());
    (engine, backend)
}

fn route(src: &LoopbackBackend, dst: &LoopbackBackend) {
    dst.fill_samples(&src.drain_samples());
}

const GOOD_SNR: f32 = 30.0;

/// Run `frames` MODCOD exchanges; `ack_delivered(i)` gates the reverse ACK.
/// Returns the ISS (TX level, TX mode) after each frame.
fn run_modcod(frames: usize, ack_delivered: impl Fn(usize) -> bool) -> Vec<(SpeedLevel, String)> {
    let payload = b"modcod ladder payload: modulation x FEC adaptation";
    let session = "modcod";
    let (mut iss, iss_lb) = make_engine();
    let (mut irs, irs_lb) = make_engine();
    irs.set_rx_snr_estimate(Some(GOOD_SNR));

    let mut history = Vec::with_capacity(frames);
    for i in 0..frames {
        let mode = iss.ota_tx_mode().expect("session").to_owned();
        let fec = iss.ota_tx_fec();
        iss.transmit_with_fec_mode(payload, &mode, fec, None)
            .unwrap();
        route(&iss_lb, &irs_lb);

        let rx = irs.respond_arq_ota(session, None).unwrap_or_else(|e| {
            panic!("frame {i}: IRS failed to decode {mode} (fec {fec:?}): {e}")
        });
        assert_eq!(
            &rx, payload,
            "payload corrupted at frame {i} ({mode}, {fec:?})"
        );

        if ack_delivered(i) {
            route(&irs_lb, &iss_lb);
            let ack = iss.receive_ack_with_short_fec(None).expect("ACK");
            iss.apply_ota_ack(&ack);
        } else {
            iss_lb.drain_samples();
            irs_lb.drain_samples();
        }
        history.push((
            iss.ota_tx_level().unwrap(),
            iss.ota_tx_mode().unwrap().to_owned(),
        ));
    }
    history
}

#[test]
fn modcod_profile_interleaves_modulation_and_fec() {
    use openpulse_core::fec::FecMode;
    let p = SessionProfile::hpx_modcod();
    // Same modulation, different FEC at adjacent rungs (the MODCOD property).
    assert_eq!(p.mode_for(SpeedLevel::Sl2), Some("BPSK250"));
    assert_eq!(p.fec_for(SpeedLevel::Sl2), FecMode::Ldpc);
    assert_eq!(p.mode_for(SpeedLevel::Sl3), Some("BPSK250"));
    assert_eq!(p.fec_for(SpeedLevel::Sl3), FecMode::Rs);
    assert_eq!(p.fec_for(SpeedLevel::Sl7), FecMode::None);
}

#[test]
fn modcod_ladder_climbs_across_modulation_and_fec_rungs() {
    let history = run_modcod(16, |_| true);
    let (last_level, last_mode) = history.last().unwrap();
    assert!(
        *last_level > SpeedLevel::Sl2,
        "MODCOD ladder should climb above the initial rung: {history:?}"
    );
    // Climbing must traverse a FEC step (BPSK250+LDPC → BPSK250+RS) before the
    // modulation changes — i.e. BPSK250 appears at more than one rung.
    assert_eq!(
        *last_level,
        SpeedLevel::Sl7,
        "should reach the top MODCOD rung: {history:?}"
    );
    assert_eq!(last_mode, "QPSK500", "top rung modulation: {history:?}");
}

#[test]
fn modcod_never_desyncs_under_ack_loss() {
    // Per-level FEC must decode correctly through the 2-mode fallback even when
    // the sender lags a rung behind on a lost ACK.
    let history = run_modcod(20, |i| i % 2 == 0);
    assert!(
        history.last().unwrap().0 > SpeedLevel::Sl2,
        "should still progress under 50% ACK loss: {history:?}"
    );
}
