//! A burst being gathered when the station keys is dropped, not flushed as a frame piece afterwards.
//!
//! On air (task 17, 2026-11-02) the receiving station keyed its station ID in the middle of the other
//! station's frame. After the transmit, the burst gathered before it was flushed on the reopened
//! stream; the piece failed, and the station keyed a NACK over the frame. The daemon now calls
//! `abandon_burst` wherever it drops the capture stream around its own transmission.

use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;

fn tone(n: usize, amp: f32, phase: &mut f32) -> Vec<f32> {
    (0..n)
        .map(|_| {
            *phase = (*phase + std::f32::consts::TAU * 1500.0 / 8000.0) % std::f32::consts::TAU;
            amp * phase.sin()
        })
        .collect()
}

#[test]
fn an_abandoned_burst_is_not_flushed_after_the_transmit() {
    let bk = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(bk.clone_shared()));
    let mut phase = 0.0f32;
    // Warm the floor on a quiet band, then gather two seconds of another station's signal.
    for _ in 0..20 {
        assert!(e
            .accumulate_capture(Some("BPSK250"), vec![0.0; 400])
            .unwrap()
            .is_none());
    }
    for _ in 0..40 {
        assert!(e
            .accumulate_capture(Some("BPSK250"), tone(400, 0.3, &mut phase))
            .unwrap()
            .is_none());
    }
    assert!(e.burst_open(), "precondition: a burst is being gathered");

    let dropped = e.abandon_burst();
    assert!(dropped >= 16_000, "dropped only {dropped} samples");
    assert!(!e.burst_open());
    // The first quiet read after the transmit: nothing to flush.
    let after = e
        .accumulate_capture(Some("BPSK250"), vec![0.0; 400])
        .unwrap();
    assert!(
        after.is_none(),
        "a burst gathered before the station keyed was flushed after it ({} samples)",
        after.map(|b| b.samples.len()).unwrap_or(0)
    );
    assert_eq!(e.abandon_burst(), 0, "nothing left to abandon");
}
