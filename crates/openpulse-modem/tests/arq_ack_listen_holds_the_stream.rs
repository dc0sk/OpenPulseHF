//! The ARQ ISS ACK listen holds one capture stream and finds an ACK behind a lead (#1315).
//!
//! `receive_ack_with_short_fec_within` retried a one-shot `receive_ack_with_short_fec`, which opens a
//! stream, reads ONCE and decodes that read whole, with no onset scan. On a callback backend each try
//! saw one poll interval, so an ACK with any lead — every ACK on real audio — was unreachable. ARDOP's
//! adaptive ISS (`bridge.rs`) and the CLI's `transmit_arq` call it. The capture is delivered in chunks
//! via `push_frame`, as `fsk4_ack_scan_reaches_the_whole_window` does: the unpaced loopback returns a
//! whole `fill_samples` capture in one read, and with that the buffer IS the ACK and nothing shows.

use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::{AckFrame, AckType};
use openpulse_modem::engine::ModemEngine;

const CHUNK: usize = 1_000;
const LISTEN_MS: u64 = 1_500;

fn engine() -> (ModemEngine, LoopbackBackend) {
    let lb = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    (e, lb)
}

fn ack_audio() -> (Vec<f32>, AckFrame) {
    let (mut irs, lb) = engine();
    let ack = AckFrame::new(AckType::AckOk, "arq");
    irs.transmit_ack_with_short_fec(&ack, None)
        .expect("transmit ACK");
    (lb.drain_samples(), ack)
}

/// Deliver `capture` one CHUNK per read and listen through the production entry.
fn listen(capture: &[f32]) -> Option<AckFrame> {
    let (mut iss, lb) = engine();
    for c in capture.chunks(CHUNK) {
        lb.push_frame(c);
    }
    iss.receive_ack_with_short_fec_within(None, LISTEN_MS).ok()
}

#[test]
fn an_ack_behind_a_lead_is_found_across_reads() {
    let (ack_audio, ack) = ack_audio();
    let mut cap = vec![0.0; 3_000 + 377];
    cap.extend_from_slice(&ack_audio);
    cap.extend(vec![0.0; 2 * CHUNK]);
    let got = listen(&cap).expect(
        "no ACK recovered from a chunked capture with a lead: the listen is not holding its stream \
         and scanning it (#1315)",
    );
    assert_eq!(got.ack_type, ack.ack_type);
    assert_eq!(got.session_hash, ack.session_hash);
}

/// Control: no ACK in the capture → the listen ends in an error at its window, not a false ACK.
#[test]
fn silence_yields_no_ack() {
    assert!(listen(&vec![0.0; 12 * CHUNK]).is_none());
}
