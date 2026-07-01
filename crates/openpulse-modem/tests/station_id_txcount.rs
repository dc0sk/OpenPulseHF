//! The engine's `frames_transmitted()` counter is the TX-activity signal the daemon's periodic
//! station-ID timer (REQ-REG-10) polls to decide when to key up and identify. It must bump exactly
//! at the single TX seam (every emit) and never on receive — otherwise a pure-receive station would
//! falsely arm auto-ID and key up unnecessarily.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;

fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("BPSK registration");
    e
}

#[test]
fn frames_transmitted_counts_emits_and_not_receives() {
    let mut e = engine();
    assert_eq!(e.frames_transmitted(), 0, "counter starts at zero");

    e.transmit_with_fec(b"DE TEST", "BPSK250", None)
        .expect("transmit");
    let after_tx = e.frames_transmitted();
    assert!(
        after_tx >= 1,
        "a transmit must bump the counter (got {after_tx})"
    );

    let received = e.receive_with_fec("BPSK250", None).expect("receive");
    assert_eq!(received, b"DE TEST");
    assert_eq!(
        e.frames_transmitted(),
        after_tx,
        "receive must not bump the TX counter"
    );

    e.transmit_with_fec(b"DE TEST", "BPSK250", None)
        .expect("transmit again");
    assert!(
        e.frames_transmitted() > after_tx,
        "a second transmit must advance the counter further"
    );
}

#[test]
fn emit_cw_id_transmits_and_counts_as_a_frame() {
    let mut e = engine();
    let before = e.frames_transmitted();
    e.emit_cw_id("W1AW", None).expect("CW ID emit");
    assert_eq!(
        e.frames_transmitted(),
        before + 1,
        "a CW ID emit is one TX frame at the seam"
    );
    // Nothing renderable → no emit, no count.
    e.emit_cw_id("###", None).expect("empty CW ID is a no-op");
    assert_eq!(
        e.frames_transmitted(),
        before + 1,
        "no-op CW ID must not count"
    );
}
