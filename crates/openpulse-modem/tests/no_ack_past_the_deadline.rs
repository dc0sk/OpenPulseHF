//! A decode that finishes past the ACK deadline answers no ACK and leaves the ladder alone, and its
//! remaining attempts are skipped (on-air task 18, run a2a: an ACK keyed 10.1 s after the frame, past
//! the sender's 9 s window, landed on the sender's retransmission; the receiver had also moved its
//! recommendation, which the sender never heard; and one failing burst then held a Pi 4 for 3 min).

use std::time::{Duration, Instant};

use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_modem::ModemEngine;

fn engine(bk: &LoopbackBackend) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(bk.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .unwrap();
    e.start_ota_session(SessionProfile::robust());
    e
}

/// One SL2 frame from a sender at the start of the ladder, as the receiver's one burst.
fn sl2_burst() -> openpulse_modem::pipeline::AudioSamples {
    let bk = LoopbackBackend::new();
    let mut tx = engine(&bk);
    let mode = tx.ota_tx_mode().unwrap().to_owned();
    let fec = tx.ota_tx_fec();
    tx.transmit_with_fec_mode(b"sl2 frame", &mode, fec, None)
        .unwrap();
    let mut audio = vec![0.0f32; 2_400];
    audio.extend(bk.drain_samples());
    let rx_bk = LoopbackBackend::new();
    let mut acc = engine(&rx_bk);
    for chunk in audio
        .chunks(800)
        .map(|c| c.to_vec())
        .chain(std::iter::repeat_n(vec![0.0; 800], 4))
    {
        if let Some(b) = acc.accumulate_capture(Some("BPSK250"), chunk).unwrap() {
            return b;
        }
    }
    panic!("the frame never flushed as a burst");
}

#[test]
fn a_decode_past_the_deadline_answers_nothing_and_moves_no_rung() {
    let burst = sl2_burst();

    // In time: decoded, answered, and the controller has decided.
    let mut rx = engine(&LoopbackBackend::new());
    let start = rx.ota_rx_recommended_level();
    rx.set_ota_ack_deadline(Some(Instant::now() + Duration::from_secs(60)));
    let res = rx
        .ota_decode_burst(&burst, "DC0SK", Some("BPSK250"))
        .unwrap();
    assert!(res.payload.is_some(), "precondition: the frame decodes");
    assert!(res.ack.is_some(), "a decode in time is answered");
    assert!(!rx.last_ota_decode_late());
    assert_ne!(
        rx.ota_rx_recommended_level(),
        start,
        "precondition: a clean frame in time moves the recommendation, so 'it stays' below means something"
    );

    // Past the deadline: nothing is attempted, nothing answered, the ladder stays where it was.
    let mut rx = engine(&LoopbackBackend::new());
    let before = rx.ota_rx_recommended_level();
    rx.set_ota_ack_deadline(Some(Instant::now()));
    // No fallback mode: the uncoded fallback (non-ladder, not bounded by the deadline) would run
    // attempts of its own; this counts the ladder decode only.
    let res = rx.ota_decode_burst(&burst, "DC0SK", None).unwrap();
    assert!(
        res.ack.is_none(),
        "an ACK past the sender's window cannot be heard and lands on its retransmission"
    );
    assert!(rx.last_ota_decode_late());
    assert_eq!(
        rx.ota_rx_recommended_level(),
        before,
        "a decision the sender never hears must not move the receiver's ladder"
    );
    assert!(
        rx.decode_attempts_past_deadline() > 0,
        "attempts past the deadline are skipped, not run"
    );
    assert_eq!(
        rx.decode_attempts(),
        0,
        "no ladder attempt ran past the deadline"
    );

    // The deadline applies to one decode only.
    let res = rx
        .ota_decode_burst(&burst, "DC0SK", Some("BPSK250"))
        .unwrap();
    assert!(
        res.ack.is_some(),
        "the next decode has no deadline unless one is set"
    );
}

/// Non-ladder traffic (station ID, file fragments, handshake) answers no ACK, so the deadline must not
/// cut its decode (review 2026-11-02, S1: on the twin, four uncoded file fragments were lost to it).
#[test]
fn a_non_ladder_frame_still_decodes_past_the_deadline() {
    let bk = LoopbackBackend::new();
    let mut tx = engine(&bk);
    tx.transmit(b"DE DC0SK", "BPSK250", None).unwrap();
    let mut audio = vec![0.0f32; 2_400];
    audio.extend(bk.drain_samples());
    let rx_bk = LoopbackBackend::new();
    let mut rx = engine(&rx_bk);
    let mut burst = None;
    for chunk in audio
        .chunks(800)
        .map(|c| c.to_vec())
        .chain(std::iter::repeat_n(vec![0.0; 800], 4))
    {
        if let Some(b) = rx.accumulate_capture(Some("BPSK250"), chunk).unwrap() {
            burst = Some(b);
            break;
        }
    }
    let burst = burst.expect("the ID flushed as a burst");
    rx.set_ota_ack_deadline(Some(Instant::now()));
    let res = rx
        .ota_decode_burst(&burst, "DC0SK", Some("BPSK250"))
        .unwrap();
    assert_eq!(
        res.payload.as_deref(),
        Some(&b"DE DC0SK"[..]),
        "an uncoded non-ladder frame must decode whatever the ACK deadline"
    );
    assert!(res.ack.is_none(), "non-ladder traffic is never ACKed");
}
