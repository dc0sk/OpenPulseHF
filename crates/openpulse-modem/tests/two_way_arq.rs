//! Bidirectional reliable-ARQ integration: ISS data forward + IRS `respond_arq`
//! ACK return, including NACK-on-failure followed by a successful retransmit.

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::AckType;
use openpulse_modem::engine::ModemEngine;

fn make_engine() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    (engine, backend)
}

/// Move samples from `src` loopback to `dst` loopback (clean passthrough).
fn route(src: &LoopbackBackend, dst: &LoopbackBackend) {
    dst.fill_samples(&src.drain_samples());
}

/// Clean round-trip: ISS sends data, IRS decodes and auto-ACKs, ISS receives the
/// ACK. Without an adaptive session the IRS replies `AckOk` (not `Nack`).
#[test]
fn two_way_arq_clean_roundtrip() {
    let payload = b"reliable two-way arq payload";
    let session = "arq-test-clean";
    let (mut iss, iss_lb) = make_engine();
    let (mut irs, irs_lb) = make_engine();

    iss.transmit(payload, "BPSK250", None).unwrap();
    route(&iss_lb, &irs_lb);

    let rx = irs
        .respond_arq("BPSK250", session, None)
        .expect("IRS should decode the clean frame");
    assert_eq!(&rx, payload);

    route(&irs_lb, &iss_lb);
    let ack = iss
        .receive_ack_with_short_fec(None)
        .expect("ISS should receive the ACK");
    assert_eq!(ack.ack_type, AckType::AckOk);
}

/// A corrupted forward path makes the IRS fail to decode and reply `Nack`; the
/// ISS then retransmits cleanly and the second attempt succeeds with an ACK.
#[test]
fn two_way_arq_nack_then_retransmit_succeeds() {
    let payload = b"arq retransmit payload";
    let session = "arq-test-retx";
    let (mut iss, iss_lb) = make_engine();
    let (mut irs, irs_lb) = make_engine();

    // Attempt 1: discard the real TX samples and hand the IRS only silence.
    iss.transmit(payload, "BPSK250", None).unwrap();
    iss_lb.drain_samples();
    irs_lb.fill_samples(&vec![0.0_f32; 8000]);
    let failed = irs.respond_arq("BPSK250", session, None);
    assert!(failed.is_err(), "IRS must fail to decode silence");

    route(&irs_lb, &iss_lb);
    let ack1 = iss
        .receive_ack_with_short_fec(None)
        .expect("ISS should receive the NACK");
    assert_eq!(ack1.ack_type, AckType::Nack, "decode failure must NACK");

    // Attempt 2 (retransmit): clean forward path succeeds.
    iss.transmit(payload, "BPSK250", None).unwrap();
    route(&iss_lb, &irs_lb);
    let rx = irs
        .respond_arq("BPSK250", session, None)
        .expect("retransmit should decode");
    assert_eq!(&rx, payload);

    route(&irs_lb, &iss_lb);
    let ack2 = iss
        .receive_ack_with_short_fec(None)
        .expect("ISS should receive the ACK");
    assert_ne!(
        ack2.ack_type,
        AckType::Nack,
        "clean retransmit must not NACK"
    );
}
