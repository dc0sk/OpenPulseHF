//! 8PSK acquisition through a real carrier offset.
//!
//! 8PSK500/1000 acquired and decoded at *zero* carrier offset but FAILED through a
//! realistic ~25 Hz offset, while BPSK/QPSK/64QAM all succeeded.
//!
//! Root cause (corrected — see memory `8psk-carrier-offset-gap`): the earlier
//! diagnosis blamed AFC-estimate precision, but a swept-AFC experiment showed the
//! demod failed to decode the 25 Hz frame *even when the applied AFC correction was
//! exactly right*.  The real bug was in `carrier_phase_correct`: when the engine
//! signalled an RF offset (`afc_correction_hz` ≥ 0.5) it fit a per-symbol phase drift
//! from the two 8-symbol preamble halves and extrapolated it across the whole frame.
//! Over an 8-symbol baseline that slope is dominated by per-half ISI, not true drift,
//! so it rotated the dense 45° constellation off its decision grid.  Removing that
//! branch (static phase + Costas only) plus replacing the single-pass Costas with a
//! two-pass decision-directed loop (pass 1 *acquires* the residual frequency, pass 2
//! *tracks* it seeded — the structure 64QAM already uses) closes the characterized gap.
//!
//! These tests pin the characterized 25 Hz case (both modes), which the fix decodes
//! reliably.  The fix also recovers many other offsets that previously all failed,
//! but coverage is not yet complete: decode succeeds when the engine's AFC settle
//! error falls inside the tracker's ~±1.5 Hz acquisition range, and that error varies
//! with offset (worse for 8PSK1000 at n=8 samples/symbol).  Closing the remaining
//! offsets needs a more reliable AFC settle — tracked as a narrowed gap in the memory.

use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use psk8_plugin::Psk8Plugin;
use std::time::Duration;

fn decodes_through_offset(mode: &str, offset_hz: f32) -> bool {
    let payload = b"8psk-carrier-offset-0123456789-abcdefghij-0123456789-abcdefghij";

    let tx_lb = LoopbackBackend::new();
    let tx_shared = tx_lb.clone_shared();
    let mut tx = ModemEngine::new(Box::new(tx_lb));
    tx.register_plugin(Box::new(Psk8Plugin::new())).unwrap();
    tx.set_center_frequency(1500.0 + offset_hz);
    tx.transmit(payload, mode, None).unwrap();
    let frame = tx_shared.drain_samples();
    assert!(!frame.is_empty(), "{mode}: transmit must produce samples");

    let rx_lb = LoopbackBackend::new();
    let rx_shared = rx_lb.clone_shared();
    let mut rx = ModemEngine::new(Box::new(rx_lb));
    rx.register_plugin(Box::new(Psk8Plugin::new())).unwrap();
    rx_shared.push_frame(&vec![0.0f32; 40000]);
    for chunk in frame.chunks(8000) {
        rx_shared.push_frame(chunk);
    }
    match rx.receive_with_timeout(mode, None, Duration::from_secs(10)) {
        Ok(got) => got.len() >= payload.len() && &got[..payload.len()] == payload,
        Err(_) => false,
    }
}

#[test]
fn psk8_500_decodes_through_25hz_offset() {
    assert!(
        decodes_through_offset("8PSK500", 25.0),
        "8PSK500 must decode through a 25 Hz carrier offset"
    );
}

#[test]
fn psk8_1000_decodes_through_25hz_offset() {
    assert!(
        decodes_through_offset("8PSK1000", 25.0),
        "8PSK1000 must decode through a 25 Hz carrier offset"
    );
}
