//! The RX burst accumulator's safety cap must not be shorter than the frames the ladder transmits.
//!
//! **The defect this pins** (archetype scan 2026-07-29, finding 1). `BURST_MAX_SAMPLES` was a flat
//! 240 000 samples — 30 s at 8 kHz — chosen as a runaway guard for "the carrier never drops". But
//! `hpx_hf`'s two slowest rungs emit frames *longer than that*: BPSK31 + Rs is ~66 s of audio and
//! BPSK63 + Rs ~33 s. On a real streaming capture the cap therefore fired mid-frame on every normal
//! transmission, force-flushing the burst into two preamble-less halves, neither decodable.
//!
//! SL2 (BPSK31) is `hpx_hf`'s `initial_level` — the rung every session starts on and must confirm
//! before it can climb — so this sat directly on the on-air critical path.
//!
//! **Why the suite could not have caught it.** Every test that reaches `accumulate_capture` uses a
//! fast mode (OFDM52-16QAM, QPSK500, BPSK250), whose frames are far under 30 s. The cap cannot fire
//! for them, so no existing gate could fail in the presence of this bug. The two constants — the cap
//! here and the frame length stated in prose at `profile.rs` — were simply never compared.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_modem::engine::ModemEngine;

const SAMPLE_RATE: usize = 8_000;

/// An engine with the BPSK plugin registered, for geometry lookups.
fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    e
}

/// Transmit one maximal frame in `mode`+`fec` and return how many samples it actually took.
fn transmitted_samples(mode: &str, fec: FecMode) -> usize {
    let lb = LoopbackBackend::new();
    let mut tx = ModemEngine::new(Box::new(lb.clone_shared()));
    tx.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    // A payload that fills a whole RS block is the worst case the cap has to survive.
    let payload = vec![0x5Au8; 200];
    tx.transmit_with_fec_mode(&payload, mode, fec, None)
        .expect("transmit");
    lb.drain_samples().len()
}

/// THE GATE: the burst cap must exceed the real coded frame length of the ladder's slow rungs.
///
/// Asserted against a *measured* transmission, not against a restated constant — a cap re-derived
/// from the same wrong assumption would otherwise still pass.
#[test]
fn the_burst_cap_exceeds_the_slow_rungs_real_frame_length() {
    for (mode, fec) in [
        ("BPSK31", FecMode::Rs),
        ("BPSK63", FecMode::Rs),
        ("BPSK31", FecMode::RsStrong),
    ] {
        let n = transmitted_samples(mode, fec);
        let cap = engine().burst_cap_samples(Some(mode));
        assert!(
            cap > n,
            "{mode}+{fec:?} transmits {n} samples ({:.1} s) but the burst cap is {cap} ({:.1} s) — \
             the accumulator would force-flush mid-frame on every real capture",
            n as f32 / SAMPLE_RATE as f32,
            cap as f32 / SAMPLE_RATE as f32,
        );
    }
}

/// Anti-vacuity: the slow rungs really are longer than the old flat 30 s cap, so the gate above is
/// testing something. If a future geometry change makes these frames short, this fails loudly rather
/// than letting the gate above pass for the wrong reason.
#[test]
fn the_slow_rungs_really_do_exceed_thirty_seconds() {
    let bpsk31 = transmitted_samples("BPSK31", FecMode::Rs);
    let bpsk63 = transmitted_samples("BPSK63", FecMode::Rs);
    assert!(
        bpsk31 > 240_000,
        "BPSK31+Rs is {bpsk31} samples; this suite assumes it exceeds the old 240k cap"
    );
    assert!(
        bpsk63 > 240_000,
        "BPSK63+Rs is {bpsk63} samples; this suite assumes it exceeds the old 240k cap"
    );
}

/// The cap must still BE a cap: an unknown mode, and a fast one, keep a bounded window so a carrier
/// that never drops cannot grow the accumulator without limit.
#[test]
fn the_cap_stays_bounded_for_fast_and_unknown_modes() {
    let unknown = engine().burst_cap_samples(None);
    assert!(
        (240_000..=1_600_000).contains(&unknown),
        "an unknown mode must still get a bounded, sane cap, got {unknown}"
    );
    let bogus = engine().burst_cap_samples(Some("NOT-A-REAL-MODE"));
    assert_eq!(
        bogus, unknown,
        "an unregistered mode must fall back to the default cap, not to zero or unbounded"
    );
}

/// The production streaming path must deliver a slow-rung frame as ONE burst.
///
/// This is the entry the daemon's `rx_ticker` actually uses; `receive()` and `ChannelSimHarness`
/// hand the receiver a whole buffer and cannot exercise the accumulator at all.
#[test]
fn a_slow_rung_frame_survives_the_streaming_accumulator_as_one_burst() {
    const MODE: &str = "BPSK63";
    let lb = LoopbackBackend::new();
    let mut tx = ModemEngine::new(Box::new(lb.clone_shared()));
    tx.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    tx.transmit_with_fec_mode(&[0x5Au8; 200], MODE, FecMode::Rs, None)
        .expect("transmit");
    let frame = lb.drain_samples();
    assert!(
        frame.len() > 240_000,
        "the fixture must be longer than the old cap or it proves nothing, got {}",
        frame.len()
    );

    let mut rx = ModemEngine::new(Box::new(LoopbackBackend::new()));
    rx.register_plugin(Box::new(BpskPlugin::new())).unwrap();

    // The daemon feeds tick-sized chunks (100 ms = 800 samples at 8 kHz), not the whole frame.
    let mut bursts = Vec::new();
    for chunk in frame.chunks(800) {
        if let Ok(Some(b)) = rx.accumulate_capture(Some(MODE), chunk.to_vec()) {
            bursts.push(b.samples.len());
        }
    }
    assert!(
        bursts.is_empty(),
        "the accumulator flushed mid-frame at {bursts:?} samples — a real frame was split into \
         preamble-less pieces, which is exactly the defect this suite pins"
    );

    // Carrier drops: now the whole frame comes out as a single burst.
    let burst = loop {
        match rx.accumulate_capture(Some(MODE), vec![0.0; 800]) {
            Ok(Some(b)) => break b,
            Ok(None) => continue,
            Err(e) => panic!("accumulate: {e}"),
        }
    };
    assert!(
        burst.samples.len() >= frame.len(),
        "the flushed burst ({}) is shorter than the transmitted frame ({}) — it was truncated",
        burst.samples.len(),
        frame.len()
    );
}
