//! MFSK16 sub-floor ARQ rung (REQ-WSIG-01, PR-1 core): the mode-aware OTA ACK path.
//!
//! The sub-floor rung (SL1 = MFSK16) can't ACK over FSK4 (it dies far above the MFSK16 floor), so the IRS
//! sends a K=3 union MFSK16-ACK when recommending SL1. The ISS cannot know which waveform the IRS chose
//! (the "drop to SL1" recommendation rides a waveform the ISS isn't yet expecting), so it **union-listens**
//! for both — the fix for the SL1-boundary desync. These tests prove both waveforms round-trip through the
//! one `receive_ota_ack_within` seam on the `hpx_hf` profile.

use fsk4_plugin::Fsk4Plugin;
use mfsk16_plugin::Mfsk16Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;

fn hf_engine() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine
        .register_plugin(Box::new(Mfsk16Plugin::new()))
        .unwrap();
    engine.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    engine.start_ota_session(SessionProfile::hpx_hf()); // hpx_hf has SL1 = MFSK16
    (engine, backend)
}

fn route(src: &LoopbackBackend, dst: &LoopbackBackend) {
    dst.fill_samples(&src.drain_samples());
}

/// An ACK recommending SL1 → the IRS transmits the K=3 union MFSK16-ACK; the ISS recovers it by
/// union-listening (FSK4 fails, the K=3 slot-union decodes).
#[test]
fn k3_mfsk16_ack_round_trips_through_union_listen() {
    let (mut iss, iss_bk) = hf_engine();
    let (mut irs, irs_bk) = hf_engine();

    let ack = AckFrame::new(AckType::AckOk, "subfloor").with_recommended_level(SpeedLevel::Sl1);
    irs.transmit_ota_ack(&ack, None).expect("transmit K3 ACK");
    route(&irs_bk, &iss_bk);

    let got = iss
        .receive_ota_ack_within(None, 9000)
        .expect("union-listen recovers the K=3 MFSK16-ACK");
    assert_eq!(got.recommended_level, Some(SpeedLevel::Sl1));
    assert_eq!(got.ack_type, AckType::AckOk);
}

/// The same `receive_ota_ack_within` seam also accepts a plain FSK4 ACK (an ACK recommending a normal
/// rung) — proving the union-listen is a superset, so an SL1 boundary crossing can't desync the ACK path.
#[test]
fn union_listen_also_accepts_the_fsk4_ack() {
    let (mut iss, iss_bk) = hf_engine();
    let (mut irs, irs_bk) = hf_engine();

    let ack = AckFrame::new(AckType::AckDown, "subfloor").with_recommended_level(SpeedLevel::Sl2);
    irs.transmit_ota_ack(&ack, None).expect("transmit FSK4 ACK");
    route(&irs_bk, &iss_bk);

    let got = iss
        .receive_ota_ack_within(None, 9000)
        .expect("union-listen recovers the FSK4 ACK");
    assert_eq!(got.recommended_level, Some(SpeedLevel::Sl2));
    assert_eq!(got.ack_type, AckType::AckDown);
}

/// A profile without an MFSK16 rung keeps the fast FSK4-only path (no sub-floor turnaround cost).
#[test]
fn non_subfloor_profile_uses_the_fast_fsk4_path() {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    engine.start_ota_session(SessionProfile::hpx500()); // no MFSK16 rung
    assert!(!engine.ota_profile_has_mfsk16());
    assert_eq!(engine.ota_ack_timeout_ms(), 4000);
}

/// Payload-capacity gate: a body over one MFSK16 RS block can't ride the SL1 sub-floor frame, so the OTA TX
/// bumps off MFSK16 to the next rung (which carries multi-block) instead of hard-erroring and dropping.
#[test]
fn oversized_body_bumps_off_the_mfsk16_subfloor_rung() {
    let (mut e, _bk) = hf_engine();
    e.ota_lock_level(SpeedLevel::Sl1);
    assert_eq!(e.ota_tx_level(), Some(SpeedLevel::Sl1));

    // Within one MFSK16 frame → stays on the sub-floor rung.
    let (small, _) = e
        .ota_tx_for_payload(ModemEngine::MFSK16_OTA_MAX_PAYLOAD)
        .expect("tx for small");
    assert_eq!(small, "MFSK16");

    // Over one RS block → bumped to SL2 (BPSK31 carries multi-block RS).
    let (large, _) = e
        .ota_tx_for_payload(ModemEngine::MFSK16_OTA_MAX_PAYLOAD + 1)
        .expect("tx for large");
    assert_eq!(large, "BPSK31");

    // The cap is exact: MFSK16 holds one RS block of MAX bytes; one more overflows the fixed frame.
    let max = ModemEngine::MFSK16_OTA_MAX_PAYLOAD;
    assert!(
        e.transmit_with_fec_mode(&vec![0u8; max], "MFSK16", FecMode::Rs, None)
            .is_ok(),
        "MFSK16 must carry MFSK16_OTA_MAX_PAYLOAD ({max}) bytes in one RS block"
    );
    assert!(
        e.transmit_with_fec_mode(&vec![0u8; max + 1], "MFSK16", FecMode::Rs, None)
            .is_err(),
        "one byte over the cap must overflow the single MFSK16 RS block"
    );
}
