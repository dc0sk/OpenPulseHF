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

#[test]
fn bpsk31_long_frame_with_early_onset_decodes() {
    // Reproduces the dual-card hardware failure: a real analog turn-on ramps the
    // carrier up over ~1-2 symbols before the clean preamble, so the energy gate +
    // refine_onset settle an onset a touch (~1-2 symbols) BEFORE the true preamble
    // — outside the demodulator's one-symbol timing search.  A partial-amplitude
    // carrier lead-in here puts the settled onset early the same way; the forward
    // onset micro-sweep in the receive loop must step forward and still decode.
    let loopback = LoopbackBackend::new();
    let shared = loopback.clone_shared();
    let mut engine = ModemEngine::new(Box::new(loopback));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();

    let payload = b"psk31-early-onset";
    engine.transmit(payload, "BPSK31", None).unwrap();
    let frame = shared.drain_samples();
    assert!(!frame.is_empty());

    // ~5 s silence, then a ~1.5-symbol carrier lead-in at 0.6x amplitude (0.36x
    // power — above refine_onset's 25%-of-peak edge, so the settle latches it ~1.5
    // symbols ahead of the true preamble), then the clean frame.
    shared.push_frame(&vec![0.0f32; 40000]);
    let lead: Vec<f32> = frame[2000..2400].iter().map(|s| s * 0.6).collect();
    shared.push_frame(&lead);
    for chunk in frame.chunks(16000) {
        shared.push_frame(chunk);
    }

    let got = engine
        .receive_with_timeout("BPSK31", None, Duration::from_secs(10))
        .expect("BPSK31 frame with an early settled onset must still decode");
    assert_eq!(&got[..payload.len()], payload);
}
