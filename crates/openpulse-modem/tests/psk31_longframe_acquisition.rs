//! Regression: a long BPSK31 frame preceded by leading silence, delivered
//! incrementally, must decode.
//!
//! The preamble is scanned (and AFC settles) while the frame is still only
//! partially buffered.  The engine must re-decode from the settled onset as the
//! rest of the frame arrives, rather than advancing the scan past it — otherwise
//! the long (~12 s) BPSK31 frame after the IRS startup wait never decodes (the
//! hardware-loopback failure this fixes; the demodulator itself is correct, see
//! the diagnosis in the project memory).
use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use std::time::Duration;

#[test]
fn bpsk31_long_frame_with_leading_silence_decodes() {
    let loopback = LoopbackBackend::new();
    let shared = loopback.clone_shared();
    let mut engine = ModemEngine::new(Box::new(loopback));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();

    // Framed BPSK31 signal (no carrier offset — matches the matched-card rig).
    let payload = b"psk31-long-frame";
    engine.transmit(payload, "BPSK31", None).unwrap();
    let frame = shared.drain_samples();
    assert!(!frame.is_empty(), "transmit must produce samples");

    // ~5 s leading silence (one read), then the frame in a few large reads, so the
    // preamble settles while the frame is still only partially buffered.
    shared.push_frame(&vec![0.0f32; 40000]);
    for chunk in frame.chunks(16000) {
        shared.push_frame(chunk);
    }

    let got = engine
        .receive_with_timeout("BPSK31", None, Duration::from_secs(10))
        .expect("BPSK31 long frame with leading silence must decode");
    assert_eq!(
        &got[..payload.len()],
        payload,
        "decoded payload must match through incremental long-frame acquisition"
    );
}
