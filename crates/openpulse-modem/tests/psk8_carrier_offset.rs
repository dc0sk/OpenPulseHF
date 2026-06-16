//! Characterization (ignored): 8PSK acquisition through a real carrier offset.
//!
//! 8PSK500/1000 acquire and decode at *zero* carrier offset (the matched-card rig,
//! fixed by the AFC deadband in the QPSK500/8PSK acquisition work) but FAIL through
//! a realistic ~25 Hz offset, while BPSK/QPSK/64QAM all succeed. Run with
//! `--ignored` to reproduce.
//!
//! Diagnosis (see also memory `8psk-carrier-offset-gap`): the engine's data-aided
//! AFC settle lands ~0.9 Hz short for 8PSK (a non-cyclic-preamble ISI bias that
//! cannot be iterated away — the estimator reads ~0.9 Hz low, so it converges with
//! that residual). After the engine downconverts, the demod's per-symbol angle
//! error sits at ~8-9° vs the ~7° baseline that decodes cleanly at zero offset —
//! just over the edge for the dense 45° grid. Downstream tracking patches don't
//! close it: a 2-pass Costas PLL gets within ~1-2° but the decision-directed loop
//! cycle-slips on the fast-rotating grid; an 8th-power (M=8) blind CFO estimate is
//! far too noisy (it produced a spurious −1.3 Hz on a *zero-offset* signal and made
//! it worse). The real fix is upstream: reduce the AFC bias (cyclic/guard preamble
//! or a debiased estimator) so the residual is ~0 like 64QAM's. That is a DSP
//! redesign, not a bounded fix; 8PSK on-air is deferred and 8PSK is not in the
//! hardware loopback matrix (BPSK+QPSK only).

use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use psk8_plugin::Psk8Plugin;
use std::time::Duration;

fn decodes_through_offset(mode: &str, offset_hz: f32) {
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
    let got = rx
        .receive_with_timeout(mode, None, Duration::from_secs(10))
        .unwrap_or_else(|e| panic!("{mode} must decode through a {offset_hz} Hz offset: {e}"));
    assert_eq!(&got[..payload.len()], payload, "{mode} payload mismatch");
}

#[test]
#[ignore = "known gap: 8PSK AFC precision insufficient for a real carrier offset; needs a debiased-preamble redesign (see module docs / memory)"]
fn psk8_500_decodes_through_25hz_offset() {
    decodes_through_offset("8PSK500", 25.0);
}

#[test]
#[ignore = "known gap: 8PSK AFC precision insufficient for a real carrier offset; needs a debiased-preamble redesign (see module docs / memory)"]
fn psk8_1000_decodes_through_25hz_offset() {
    decodes_through_offset("8PSK1000", 25.0);
}
