//! The OTA burst scan runs its attempts onset-major (`docs/dev/design/ota-scan-onset-major.md`).
//!
//! Fixture: `tests/captures/ft991a-ota-frame-bpsk31-rs.wav`, station B (the FT-991A) receiving an
//! IC-9700 `BPSK31|rs` OTA data frame on 2 m, recorded beside its daemon on 2026-10-31 (on-air task
//! 12, run B2). Replayed through the daemon's receive entry the way `server::run` drives it:
//! `accumulate_capture` in 800-sample reads, then `ota_decode_burst` on the flushed burst.
//!
//! On that burst the settle at the trigger point lands one preamble sideband off (−12.8 Hz against a
//! carrier measured at +3.2 Hz). Run pass-major, the corrected pass walked all 129 onsets before the
//! uncorrected pass decoded at onset 2560 — 139 failed attempts, 8.1 s on x86, past a Pi's 9 s ACK
//! window. Onset-major, the same attempt set stops at onset 2560.

use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::capture_replay::load_corpus;
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
    e
}

/// Replay the fixture; return (decoded payload length, decode attempts spent on the frame's burst).
fn replay() -> (Option<usize>, u64) {
    let cap = load_corpus("ft991a-ota-frame-bpsk31-rs.wav").expect("corpus");
    let bk = LoopbackBackend::new();
    let mut rx = engine(&bk);
    for chunk in cap.samples.chunks(800) {
        let Some(burst) = rx
            .accumulate_capture(Some("BPSK250"), chunk.to_vec())
            .unwrap()
        else {
            continue;
        };
        // Idle flickers before the frame flush as short bursts; the frame is the long one.
        if burst.samples.len() < 400_000 {
            let _ = rx.ota_decode_burst(&burst, "DC0SK", Some("BPSK250"));
            continue;
        }
        let before = rx.decode_attempts();
        let res = rx
            .ota_decode_burst(&burst, "DC0SK", Some("BPSK250"))
            .unwrap();
        return (res.payload.map(|p| p.len()), rx.decode_attempts() - before);
    }
    (None, 0)
}

#[test]
fn a_real_off_correction_burst_decodes_without_walking_the_whole_span() {
    let (payload, attempts) = replay();
    assert_eq!(payload, Some(64), "the on-air frame must decode");
    println!("decode attempts on the frame's burst: {attempts}");
    assert!(
        attempts <= ATTEMPT_BUDGET,
        "{attempts} decode attempts on one burst: the scan walked a pass to the end of the span \
         before trying the next (budget {ATTEMPT_BUDGET})"
    );
}

/// Measured on this fixture at the commit that introduced it: 26 attempts onset-major, 144 with the
/// scan reverted to pass-major (sabotage run). 40 leaves room for upstream changes to the arms before
/// the scan, and is still far below what a pass walked to the end of the span costs.
const ATTEMPT_BUDGET: u64 = 40;

/// A burst shorter than one coded frame of every candidate must not be onset-scanned, and its uncoded
/// fallback rescue must stay bounded.
///
/// Fixture: `tests/captures/ft991a-ota-fragment-5s.wav`, 16 s around a 5.1 s burst station B
/// gathered between two of A's SL2 frames (on-air task 13, run b3). An SL2 frame is 66.6 s, so the
/// burst cannot hold one. Before the guard it cost 1088 coded attempts and 79 s on x86 at SL2/SL3.
#[test]
fn a_burst_shorter_than_a_coded_frame_is_not_onset_scanned() {
    let cap = load_corpus("ft991a-ota-fragment-5s.wav").expect("corpus");
    let bk = LoopbackBackend::new();
    let mut rx = engine(&bk);
    let mut worst = (0usize, 0u64);
    for chunk in cap.samples.chunks(800) {
        let Some(burst) = rx
            .accumulate_capture(Some("BPSK250"), chunk.to_vec())
            .unwrap()
        else {
            continue;
        };
        let before = rx.decode_attempts();
        let res = rx
            .ota_decode_burst(&burst, "DC0SK", Some("BPSK250"))
            .unwrap();
        assert!(res.payload.is_none(), "no frame fits in this fixture");
        let spent = rx.decode_attempts() - before;
        if burst.samples.len() > worst.0 {
            worst = (burst.samples.len(), spent);
        }
    }
    println!(
        "longest burst {} samples: {} decode attempts",
        worst.0, worst.1
    );
    assert!(
        worst.0 > 30_000,
        "the fragment must flush as a burst (got {} samples)",
        worst.0
    );
    assert!(
        worst.1 <= FRAGMENT_BUDGET,
        "{} decode attempts on a burst too short for an SL2 frame (budget {FRAGMENT_BUDGET})",
        worst.1
    );
}

/// Measured on this fixture: 133 attempts with both bounds; 511 with the fallback-rescue cap removed
/// (`OTA_FALLBACK_RESCUE_ONSETS` unbounded), 888 with the short-of-block guard disabled too (sabotage
/// runs). 200 fails if either bound is removed.
const FRAGMENT_BUDGET: u64 = 200;

/// Phase 2's fallback entry acquires across the onsets it will scan, not across a span that grows with
/// the read that triggered the flush (task 15: a 14.6 s burst cost ~4700 settle passes, 19 s on x86).
/// The fragment fixture fed in 2 s reads gives the burst a read-sized `onset_bound`.
#[test]
fn the_fallback_rescue_acquisition_does_not_grow_with_the_read() {
    let cap = load_corpus("ft991a-ota-fragment-5s.wav").expect("corpus");
    let bk = LoopbackBackend::new();
    let mut rx = engine(&bk);
    let mut worst = (0usize, 0u64);
    // The fixture's own idle (its first 2 s) twice more, so the last burst is closed by a quiet read.
    let idle = cap.samples[..16_000].to_vec();
    let reads = cap
        .samples
        .chunks(16_000)
        .map(|c| c.to_vec())
        .chain([idle.clone(), idle]);
    for chunk in reads {
        let Some(burst) = rx.accumulate_capture(Some("BPSK250"), chunk).unwrap() else {
            continue;
        };
        let before = rx.afc_settle_attempts();
        let _ = rx
            .ota_decode_burst(&burst, "DC0SK", Some("BPSK250"))
            .unwrap();
        let spent = rx.afc_settle_attempts() - before;
        if burst.samples.len() > worst.0 {
            worst = (burst.samples.len(), spent);
        }
    }
    println!("longest burst {} samples: {} settles", worst.0, worst.1);
    assert!(
        worst.0 > 30_000,
        "the fragment must flush as a burst (got {} samples)",
        worst.0
    );
    assert!(
        worst.1 <= SETTLE_BUDGET,
        "{} settles on one burst: the fallback rescue acquired across the read-sized span \
         (budget {SETTLE_BUDGET})",
        worst.1
    );
}

/// Measured on this fixture in 2 s reads: 17 settles with the acquisition bounded to the rescue's
/// onsets, 206 with the fallback entry acquiring across the full span (sabotage run).
/// A tripwire fitted to this one fixture: it shows the span no longer grows with the read, not that the
/// rescue decodes anything.
const SETTLE_BUDGET: u64 = 60;

/// An MFSK16 candidate whose acquisition finds no lock from offset 0 is not onset-scanned: its
/// acquisition already searched every onset of the burst (`docs/dev/design/ota-scan-onset-major.md`,
/// addendum 4).
///
/// Fixture: `tests/captures/ft991a-ota-bpsk31-at-sl1.wav`, 32 s of station B's recording (on-air task 15,
/// run b5) while station A sent an SL2 `BPSK31|rs` frame and B's receiver had stepped down to SL1. A
/// 27 s piece of that frame flushes as one burst; at SL1 it is tried as MFSK16 only. Before the bound,
/// the live-loop replay of the same run spent 205 failing MFSK16 attempts (56 s on x86) on such a burst.
#[test]
fn an_mfsk16_candidate_with_no_lock_is_not_onset_scanned() {
    let cap = load_corpus("ft991a-ota-bpsk31-at-sl1.wav").expect("corpus");
    let bk = LoopbackBackend::new();
    let mut rx = engine(&bk);
    rx.ota_lock_level(SpeedLevel::Sl1);
    let mut worst = (0usize, 0u64);
    // 2 s reads, as a receive loop held by a decode delivers them (the live-loop regime that gives the
    // burst a read-sized onset bound), then two silent reads to close the last burst.
    let reads = cap
        .samples
        .chunks(16_000)
        .map(|c| c.to_vec())
        .chain(std::iter::repeat_n(vec![0.0f32; 16_000], 2));
    for chunk in reads {
        let Some(burst) = rx.accumulate_capture(Some("BPSK250"), chunk).unwrap() else {
            continue;
        };
        let before = rx.decode_attempts();
        let res = rx
            .ota_decode_burst(&burst, "DC0SK", Some("BPSK250"))
            .unwrap();
        assert!(res.payload.is_none(), "no MFSK16 frame is in this fixture");
        let spent = rx.decode_attempts() - before;
        if burst.samples.len() > worst.0 {
            worst = (burst.samples.len(), spent);
        }
    }
    println!(
        "longest burst {} samples: {} decode attempts",
        worst.0, worst.1
    );
    assert!(
        worst.0 > 150_000,
        "the frame piece must flush as one burst longer than an MFSK16 frame (got {} samples)",
        worst.0
    );
    assert!(
        worst.1 <= MFSK16_BUDGET,
        "{} decode attempts on a burst MFSK16 found no lock in (budget {MFSK16_BUDGET})",
        worst.1
    );
}

/// Measured on this fixture in 2 s reads (x86, release): 69 attempts in 2.9 s with the bound, 171 in
/// 41.7 s with MFSK16's uncorrected pass scanned again (sabotage run). Most of the 69 are the uncoded
/// fallback rescue. 110 fails without the bound. A tripwire fitted to this one fixture.
const MFSK16_BUDGET: u64 = 110;

/// The bound's residual case (addendum 4): an MFSK16 frame that follows other energy in the same
/// burst. Offset 0's acquisition must still find the frame, because no later onset is tried for it.
///
/// Four seconds of the fixture's BPSK31 audio, then an SL1 frame scaled to the same RMS, gathered as one
/// burst through the production entry in 2 s reads. Review round 5, finding 7.
#[test]
fn an_mfsk16_frame_after_other_energy_in_one_burst_still_decodes() {
    const PAYLOAD: &[u8] = b"MFSK16 after a BPSK31 piece";
    let cap = load_corpus("ft991a-ota-bpsk31-at-sl1.wav").expect("corpus");
    // t = 155–159 s of the recording: A's BPSK31 signal, after B's own transmission.
    let lead = &cap.samples[9 * 8000..13 * 8000];
    let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
    let tx_bk = LoopbackBackend::new();
    let mut tx = engine(&tx_bk);
    tx.ota_lock_level(SpeedLevel::Sl1);
    tx.transmit_with_fec(PAYLOAD, "MFSK16", None)
        .expect("transmit");
    let frame = tx_bk.drain_samples();
    let gain = rms(lead) / rms(&frame);
    let audio: Vec<f32> = lead
        .iter()
        .copied()
        .chain(frame.iter().map(|x| x * gain))
        .chain(std::iter::repeat_n(0.0, 32_000))
        .collect();

    let bk = LoopbackBackend::new();
    let mut rx = engine(&bk);
    rx.ota_lock_level(SpeedLevel::Sl1);
    let mut got = None;
    let mut longest = 0usize;
    for chunk in audio.chunks(16_000) {
        let Some(burst) = rx
            .accumulate_capture(Some("BPSK250"), chunk.to_vec())
            .unwrap()
        else {
            continue;
        };
        longest = longest.max(burst.samples.len());
        let res = rx
            .ota_decode_burst(&burst, "DC0SK", Some("BPSK250"))
            .unwrap();
        if res.payload.is_some() {
            got = res.payload;
        }
    }
    assert!(
        longest >= lead.len() + frame.len() - 16_000,
        "the BPSK31 piece and the frame must gather as one burst (longest {longest} samples)"
    );
    assert_eq!(
        got.as_deref(),
        Some(PAYLOAD),
        "an MFSK16 frame behind other energy must decode from offset 0's acquisition"
    );
}
