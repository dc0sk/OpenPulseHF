//! `ModemEngine::estimate_air_secs` — the pure airtime estimate that airtime-bounded TX burst
//! planning relies on. It must be positive, grow with payload size, track the mode's symbol rate, and
//! return `None` for an unknown mode — all without touching the wire sequence or the audio backend.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::engine::ModemEngine;

fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("BPSK registration");
    e
}

#[test]
fn positive_and_monotonic_in_payload_size() {
    let e = engine();
    let small = e.estimate_air_secs(32, "BPSK250").expect("known mode");
    let large = e.estimate_air_secs(256, "BPSK250").expect("known mode");
    assert!(small > 0.0, "airtime must be positive");
    assert!(
        large > small,
        "a bigger payload takes longer: {large} vs {small}"
    );
}

#[test]
fn a_slower_mode_takes_longer_than_a_faster_one() {
    let e = engine();
    let slow = e.estimate_air_secs(128, "BPSK31").expect("known mode");
    let fast = e.estimate_air_secs(128, "BPSK250").expect("known mode");
    assert!(
        slow > fast,
        "BPSK31 (31 baud) must be slower than BPSK250: {slow} vs {fast}"
    );
}

#[test]
fn unknown_mode_is_none() {
    assert!(engine().estimate_air_secs(128, "NOPE-9000").is_none());
}

#[test]
fn estimating_emits_no_audio_and_does_not_disturb_a_later_transmit() {
    // The estimate must be side-effect-free: it modulates into a throwaway buffer, never the backend.
    // If it had emitted audio, a receive before any real transmit would decode a spurious frame.
    let mut e = engine();
    let _ = e.estimate_air_secs(128, "BPSK250");
    let _ = e.estimate_air_secs(64, "BPSK250");
    e.transmit(b"hi", "BPSK250", None).expect("transmit");
    let got = e.receive("BPSK250", None).expect("receive");
    assert_eq!(got, b"hi");
}
