//! Reproduction: a QPSK500 frame preceded by leading silence, delivered
//! incrementally through `receive_with_timeout`, must decode.
//!
//! Excluded from the hardware loopback with the note "AFC anchor fires at
//! preamble start, retry misses by 200 samples (engine bug)". This exercises the
//! same engine scan/AFC/onset path in-process (no audio hardware, no carrier
//! offset — matched-card rig conditions).

use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use qpsk_plugin::QpskPlugin;
use std::time::Duration;

#[test]
fn qpsk500_frame_with_leading_silence_decodes() {
    let loopback = LoopbackBackend::new();
    let shared = loopback.clone_shared();
    let mut engine = ModemEngine::new(Box::new(loopback));
    engine.register_plugin(Box::new(QpskPlugin::new())).unwrap();

    let payload = b"qpsk500-acquisition-regression-payload-128b-..............................................................";
    engine.transmit(payload, "QPSK500", None).unwrap();
    let frame = shared.drain_samples();
    assert!(!frame.is_empty(), "transmit must produce samples");

    // ~5 s leading silence (one read), then the frame in a few reads, so the
    // preamble settles while the frame is still only partially buffered.
    shared.push_frame(&vec![0.0f32; 40000]);
    for chunk in frame.chunks(8000) {
        shared.push_frame(chunk);
    }

    let got = engine
        .receive_with_timeout("QPSK500", None, Duration::from_secs(10))
        .expect("QPSK500 frame with leading silence must decode");
    assert_eq!(
        &got[..payload.len()],
        payload,
        "decoded payload must match through incremental acquisition"
    );
}
