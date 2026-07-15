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
use openpulse_channel::{awgn::AwgnChannel, AwgnConfig, ChannelModel};
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
        .receive_ota_ack_within(None, 9000, None)
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
        .receive_ota_ack_within(None, 9000, None)
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

/// Payload-capacity guard: a body over one MFSK16 RS block can't ride the SL1 sub-floor frame, so the
/// daemon skips the send (the sub-floor rung is for short traffic; bumping to a faster rung is futile in a
/// real fade). The engine reports the fit; a non-sub-floor rung always fits.
#[test]
fn oversized_body_does_not_fit_the_mfsk16_subfloor_rung() {
    let max = ModemEngine::MFSK16_OTA_MAX_PAYLOAD;

    // At the MFSK16 sub-floor rung: within one RS block fits, one byte over does not.
    let (mut e, _bk) = hf_engine();
    e.ota_lock_level(SpeedLevel::Sl1);
    assert_eq!(e.ota_tx_level(), Some(SpeedLevel::Sl1));
    assert!(e.ota_payload_fits_tx_rung(max));
    assert!(!e.ota_payload_fits_tx_rung(max + 1));

    // The cap is exact: MFSK16 holds one RS block of MAX bytes; one more overflows the fixed frame.
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

    // A non-sub-floor rung (BPSK31 at SL2) carries multi-block RS → any body fits.
    let (mut e2, _bk2) = hf_engine();
    e2.ota_lock_level(SpeedLevel::Sl2);
    assert!(e2.ota_payload_fits_tx_rung(max + 1000));
}

/// Audit DSP#1 regression gate: the shipped K=3 ACK receiver must decode across turnaround phases at the
/// sub-floor's operating SNR. The original RMS-`energy_onset` aligner triggered on noise at ≤7 dB SNR and
/// decoded only ~28% of turnaround phases at 0 dB (measured 15/45); the Costas-anchored aligner recovers
/// all phases. Build one clean K=3 ACK, then for a sweep of leads (turnaround phases) + 0 dB AWGN, decode
/// through the production `receive_ota_ack_within` path (held-open capture stream).
#[test]
fn k3_ack_decodes_across_turnaround_phases_at_operating_snr() {
    let ack = AckFrame::new(AckType::AckOk, "phase").with_recommended_level(SpeedLevel::Sl1);
    let (mut tx, tx_bk) = hf_engine();
    tx.transmit_ack_mfsk16_k3(&ack, None)
        .expect("modulate K3 ACK");
    let clean = tx_bk.drain_samples();

    // Phases spanning the region the old RMS onset failed on (finder: 0/3 for p ∈ [4064..13208]).
    let leads = [0usize, 1500, 4064, 8000, 13208];
    let mut ok = 0;
    for (i, &lead) in leads.iter().enumerate() {
        let mut sig = vec![0.0f32; lead];
        sig.extend_from_slice(&clean);
        let faded = AwgnChannel::new(AwgnConfig::new(0.0, Some(100 + i as u64)))
            .expect("awgn")
            .apply(&sig);
        let (mut rx, rx_bk) = hf_engine();
        rx_bk.fill_samples(&faded);
        if rx
            .receive_ota_ack_within(None, 800, None)
            .map(|a| a.recommended_level == Some(SpeedLevel::Sl1) && a.ack_type == AckType::AckOk)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    assert!(
        ok >= leads.len() - 1,
        "K=3 ACK must decode across turnaround phases at 0 dB AWGN (got {ok}/{}); the RMS-onset bug \
         decoded ~28% of phases",
        leads.len()
    );
}

/// Audit DSP#3 fix: an ACK carrying a co-channel session's hash must NOT be adopted (else the ISS adopts a
/// foreign rate and marks the message delivered though the peer never got it). A matching hash IS adopted.
#[test]
fn co_channel_ack_with_wrong_session_hash_is_rejected() {
    let expected = AckFrame::hash_session_id("OURPEER");

    // A co-channel pair's ACK (built with a different session id) → rejected → the window times out.
    let (mut iss, iss_bk) = hf_engine();
    let (mut irs, irs_bk) = hf_engine();
    let foreign =
        AckFrame::new(AckType::AckOk, "OTHER-PAIR").with_recommended_level(SpeedLevel::Sl1);
    irs.transmit_ota_ack(&foreign, None)
        .expect("tx foreign ACK");
    route(&irs_bk, &iss_bk);
    assert!(
        iss.receive_ota_ack_within(None, 300, Some(expected))
            .is_err(),
        "a co-channel ACK with a mismatched session hash must not be adopted"
    );

    // Our peer's ACK (matching session id) → adopted.
    let (mut iss2, iss2_bk) = hf_engine();
    let (mut irs2, irs2_bk) = hf_engine();
    let mine = AckFrame::new(AckType::AckOk, "OURPEER").with_recommended_level(SpeedLevel::Sl1);
    irs2.transmit_ota_ack(&mine, None).expect("tx our ACK");
    route(&irs2_bk, &iss2_bk);
    let got = iss2
        .receive_ota_ack_within(None, 800, Some(expected))
        .expect("our peer's ACK (matching hash) must be adopted");
    assert_eq!(got.recommended_level, Some(SpeedLevel::Sl1));
}
