//! Scan-path acquisition coverage: multicarrier (OFDM/SC-FDMA) and 64QAM frames
//! preceded by leading silence must acquire + decode through `receive_with_timeout`.
//!
//! BPSK/QPSK/8PSK have dedicated acquisition regressions (`*_acquisition.rs`);
//! this locks the same energy-gate → onset → settle → re-decode path for the
//! multicarrier and dense single-carrier families, which had only `receive()`
//! (no-scan) channel-sim coverage before.

use ofdm_plugin::OfdmPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use qam64_plugin::Qam64Plugin;
use scfdma_plugin::ScFdmaPlugin;
use std::time::Duration;

fn acquires_through_silence(register: impl Fn(&mut ModemEngine), mode: &str) {
    let loopback = LoopbackBackend::new();
    let shared = loopback.clone_shared();
    let mut engine = ModemEngine::new(Box::new(loopback));
    register(&mut engine);

    let payload = b"acquisition-coverage-0123456789-abcdefghij-0123456789-abcdefghij";
    engine.transmit(payload, mode, None).unwrap();
    let frame = shared.drain_samples();
    assert!(!frame.is_empty(), "{mode}: transmit must produce samples");

    shared.push_frame(&vec![0.0f32; 40000]);
    for chunk in frame.chunks(8000) {
        shared.push_frame(chunk);
    }

    let got = engine
        .receive_with_timeout(mode, None, Duration::from_secs(10))
        .unwrap_or_else(|e| panic!("{mode} frame with leading silence must decode: {e}"));
    assert_eq!(&got[..payload.len()], payload, "{mode} payload mismatch");
}

#[test]
fn ofdm52_acquires_through_leading_silence() {
    acquires_through_silence(
        |e| {
            e.register_plugin(Box::new(OfdmPlugin::new())).unwrap();
        },
        "OFDM52",
    );
}

#[test]
fn scfdma52_acquires_through_leading_silence() {
    acquires_through_silence(
        |e| {
            e.register_plugin(Box::new(ScFdmaPlugin::new())).unwrap();
        },
        "SCFDMA52",
    );
}

#[test]
fn qam64_500_acquires_through_leading_silence() {
    acquires_through_silence(
        |e| {
            e.register_plugin(Box::new(Qam64Plugin::new())).unwrap();
        },
        "64QAM500",
    );
}
