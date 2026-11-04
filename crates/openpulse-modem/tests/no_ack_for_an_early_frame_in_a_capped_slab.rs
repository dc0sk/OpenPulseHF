//! A frame decoded early in a CAPPED slab (#1255) is not answered when it ended past the sender's ACK
//! window, although the slab itself ended inside it.
//!
//! On air (task 21, run a2g) every cap flush that decoded a frame keyed an ACK, 13 times. The deadline
//! is set from the slab's end, and a capped slab ends where the length cap fell, up to 38 s after its
//! frame. The ACK carries no sequence number, so it can land in the sender's window for a later frame
//! and mark that one delivered (review 2026-11-04 of `dcd-raised-floor.md`).

use std::time::{Duration, Instant};

use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::ModemEngine;

fn engine(bk: &LoopbackBackend) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(bk.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .unwrap();
    e.start_ota_session(SessionProfile::robust());
    e.ota_lock_level(SpeedLevel::Sl6);
    e
}

fn noise(n: usize, amp: f32, st: &mut u64) -> Vec<f32> {
    (0..n)
        .map(|_| {
            *st ^= *st << 13;
            *st ^= *st >> 7;
            *st ^= *st << 17;
            ((*st >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * amp * 3.46
        })
        .collect()
}

/// A capped slab: quiet, a frame, then a carrier that never drops (the band stepped up). Returns the
/// engine, the slab, and the frame's true tail: the slab after the frame's last sample.
fn capped_slab() -> (
    ModemEngine,
    openpulse_modem::pipeline::AudioSamples,
    Duration,
) {
    let bk = LoopbackBackend::new();
    let mut tx = engine(&bk);
    let mode = tx.ota_tx_mode().unwrap().to_owned();
    let fec = tx.ota_tx_fec();
    tx.transmit_with_fec_mode(b"early in a capped slab", &mode, fec, None)
        .unwrap();
    let frame: Vec<f32> = bk.drain_samples().iter().map(|s| s * 0.1).collect();
    let mut st = 0x9e37_79b9_7f4a_7c15_u64;
    let mut audio = noise(24_000, 0.0005, &mut st);
    let floor = noise(frame.len(), 0.0005, &mut st);
    audio.extend(frame.iter().zip(&floor).map(|(s, n)| s + n));
    audio.extend(noise(60 * 8000, 0.02, &mut st));

    let rx_bk = LoopbackBackend::new();
    let mut rx = engine(&rx_bk);
    for chunk in audio.chunks(800) {
        if let Some(b) = rx
            .accumulate_capture(Some("BPSK250"), chunk.to_vec())
            .unwrap()
        {
            if b.samples.len() > 200_000 {
                // The slab opens `lead` samples before the frame's first read.
                let lead = rx.last_flush_lead();
                let tail = b.samples.len().saturating_sub(lead + frame.len());
                return (rx, b, Duration::from_secs_f64(tail as f64 / 8000.0));
            }
        }
    }
    panic!("no capped slab: the carrier did not hold to the length cap");
}

/// Decodes a fresh capped slab with the sender's window ending `window(true tail)` from now; returns
/// (decoded, ACK keyed). Asserts the decode itself finished inside the window, so a slow decode (the
/// ordinary late path) cannot pass for this rule.
fn decode(window: impl Fn(Duration) -> Option<Duration>) -> (bool, bool, Duration) {
    let (mut rx, slab, tail) = capped_slab();
    let deadline = window(tail).map(|w| Instant::now() + w);
    rx.set_ota_ack_deadline(deadline);
    let r = rx.ota_decode_burst(&slab, "DC0SK", None).unwrap();
    if let Some(d) = deadline {
        assert!(
            Instant::now() < d,
            "the decode itself ran past the window: this case tests nothing"
        );
    }
    (
        r.payload.as_deref() == Some(&b"early in a capped slab"[..]),
        r.ack.is_some(),
        tail,
    )
}

#[test]
fn a_frame_early_in_a_capped_slab_is_not_answered_past_its_window() {
    // The sender's window from the slab's end, as the daemon sets it on real-time audio: about 6 s.
    let (decoded, acked, tail) = decode(|_| Some(Duration::from_secs(6)));
    assert!(decoded, "precondition: the frame decodes");
    assert!(
        !acked,
        "an ACK was keyed for a frame that ended {tail:?} before its capped slab's end: it would land \
         in the sender's window for a later frame (task 21, run a2g)"
    );
}

#[test]
fn the_frames_end_is_its_airtime_not_its_slice_reserve() {
    // A window 2 s shorter than the frame's true tail: past it, so no ACK. The first bound (the slice
    // reserve, 9.4 s at SL6 against 4.2 s of airtime) read the tail 5.2 s short and answered.
    let (decoded, acked, tail) = decode(|tail| Some(tail.saturating_sub(Duration::from_secs(2))));
    assert!(decoded, "precondition: the frame decodes");
    assert!(
        !acked,
        "a frame whose true tail ({tail:?}) is 2 s past the window was answered: the frame's end was \
         taken later than it is"
    );
}

#[test]
fn a_frame_still_inside_its_window_is_answered() {
    // A window 2 s longer than the frame's true tail: inside it, so it is answered.
    let (decoded, acked, tail) = decode(|tail| Some(tail + Duration::from_secs(2)));
    assert!(decoded, "precondition: the frame decodes");
    assert!(
        acked,
        "a frame whose true tail ({tail:?}) fits the window was not answered"
    );
    // No deadline at all (audio faster than real time, the twin): unchanged, answered.
    let (_, acked, _) = decode(|_| None);
    assert!(
        acked,
        "with no deadline set, the frame must be answered as before"
    );
}
