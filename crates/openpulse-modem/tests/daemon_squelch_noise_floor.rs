//! The DAEMON's carrier detector must square with the band it is listening to (REQ-DCD-01).
//!
//! **Why this file exists, and why it is not about any mode.** The receive machinery hardened by
//! #1020/#1021/#1039/#1040/#1045/#1049 — `EnergyGate`, the AFC settle, the preamble-correlation
//! veto, the condemnation recovery — lives entirely in the scanning `receive_with_timeout*` family.
//! The shipped daemon calls **none of it**: `server.rs`'s rx tick uses `accumulate_capture` →
//! `decode_burst` / `ota_decode_burst`. Its only frame-start decision is `DcdState`, and that was
//! created with a **fixed** 0.01 RMS squelch.
//!
//! A real band floor walks straight over a constant. The recorded IC-9700 idle capture measures
//! ≈ 0.126 RMS — **12× that squelch** — so on that band the DCD reads permanently busy, the burst
//! never ends on a carrier drop, and it flushes only when it hits the runaway cap. `decode_burst`
//! then scans just the first few acquisition windows of a cap-length buffer of noise.
//!
//! This is the mode-independent half of the problem, which is where it belongs: level, floor and
//! interference are properties of the environment, not of the waveform. Frame *detection* stays
//! per-waveform (codec2 correlates against per-mode templates too); what must be mode-independent is
//! the criterion, and "is the channel busy" is exactly that.
//!
//! Everything here drives the **production entry** (`accumulate_capture`), never the convenience
//! seam — the repo's standing rule for cross-cutting receive behaviour.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::ModemEngine;

fn engine() -> ModemEngine {
    let lb = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    e
}

fn corpus(name: &str) -> Capture {
    load_corpus(name).unwrap_or_else(|e| panic!("corpus file {name} must load: {e}"))
}

/// Feed `samples` to the production capture entry in realistic read-sized blocks, returning every
/// burst it flushed.
fn feed(e: &mut ModemEngine, mode: &str, samples: &[f32], block: usize) -> Vec<usize> {
    let mut bursts = Vec::new();
    for chunk in samples.chunks(block) {
        if let Ok(Some(b)) = e.accumulate_capture(Some(mode), chunk.to_vec()) {
            bursts.push(b.samples.len());
        }
    }
    bursts
}

/// THE DEFECT: a recorded idle noise floor must not read as a carrier.
///
/// This is the whole bug in one assertion. Nothing is transmitted — the input is 20 s of audio a
/// radio actually produced while nobody was talking. A receiver that calls that a carrier has no
/// squelch at all on that band, and every burst it hands the decoder is noise.
#[test]
fn a_recorded_idle_floor_is_not_mistaken_for_a_carrier() {
    let hot = corpus("ic9700-idle-hot.wav");
    // Guard the premise: this file exists to BE a floor above the fixed squelch. If it were quiet,
    // the test would pass on any code.
    let rms = hot.mean_sq().sqrt();
    assert!(
        rms > 0.01,
        "recorded floor is {rms:.4} RMS, no longer above the 0.01 fixed squelch — this test's \
         premise is gone"
    );

    let mut e = engine();
    // Feed MORE than the runaway cap. This length is load-bearing: below the cap a permanently-busy
    // receiver accumulates silently and flushes nothing, so a shorter feed passes this test while
    // the defect is fully present — measured, that is exactly what 160 000 samples did.
    let cap = e.burst_cap_samples(Some("BPSK250"));
    let idle = hot.cycled(0, cap + 40_000);
    let bursts = feed(&mut e, "BPSK250", &idle, 800);

    assert!(
        bursts.is_empty(),
        "the receiver flushed {} burst(s) of {:?} samples from PURE RECORDED IDLE at {rms:.4} RMS, \
         against a {cap}-sample runaway cap. The daemon's carrier detector is a fixed 0.01 \
         threshold, so a band whose floor sits above it reads as permanently busy: the burst never \
         ends on a carrier drop, and the cap alone flushes it — handing the decoder a bufferful of \
         noise and nothing else.",
        bursts.len(),
        bursts
    );
}

/// The other half: raising the squelch must not make the receiver deaf.
///
/// A threshold that adapts to the floor could trivially pass the test above by sitting above
/// everything. This is the negative control — a real frame in that same recorded floor must still
/// produce exactly one burst, and it must be bounded around the frame rather than a cap-length
/// dump.
#[test]
fn a_frame_in_that_same_floor_still_produces_a_bounded_burst() {
    let hot = corpus("ic9700-idle-hot.wav");
    let mut tx = engine();
    tx.transmit(b"adaptive squelch probe", "BPSK250", None)
        .expect("transmit");
    // The transmit went to the loopback backend; pull it back out as the signal to embed.
    let frame = {
        let lb = LoopbackBackend::new();
        let mut e2 = ModemEngine::new(Box::new(lb.clone_shared()));
        e2.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        e2.transmit(b"adaptive squelch probe", "BPSK250", None)
            .expect("transmit");
        lb.drain_samples()
    };
    assert!(!frame.is_empty(), "fixture frame is empty");

    let mut buf = hot.cycled(0, 24_000);
    buf.extend(frame.iter().map(|s| s * 0.3));
    buf.extend(hot.cycled(24_000, 24_000));

    let mut e = engine();
    let bursts = feed(&mut e, "BPSK250", &buf, 800);

    let cap = e.burst_cap_samples(Some("BPSK250"));
    assert!(
        !bursts.is_empty(),
        "no burst at all from a real frame in the recorded floor. Two different failures land here \
         and the burst lengths tell them apart: on a FIXED squelch the floor keeps the carrier \
         permanently 'present', so the burst never ends and the frame is still sitting in the \
         accumulator unflushed (this is the pre-fix behaviour); on an over-raised adaptive squelch \
         the frame never opens it at all. Cap is {cap} samples."
    );
    let longest = bursts.iter().copied().max().unwrap_or(0);
    assert!(
        longest < cap,
        "the longest burst is {longest} samples, at the {cap}-sample runaway cap — the carrier \
         never 'dropped', so this is the permanently-busy failure wearing a burst's clothes"
    );
}
