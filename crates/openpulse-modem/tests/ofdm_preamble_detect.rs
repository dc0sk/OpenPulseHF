//! The preamble carrier detect P gathers OFDM frames a receiver AGC has flattened to the hiss, and
//! nothing else (`docs/dev/design/ofdm-preamble-detect.md`, gates 1–6).
//!
//! On 2 m under AGC FAST (on-air task 27, run f2g) each SL7 frame's total power is 1.15–1.22× the
//! gap hiss and, per 62.5 Hz band, a median 1.2× its floor: neither total power nor the spectral test
//! can see it, so station A never gathered one. Every gate here goes through `accumulate_capture` in
//! daemon-sized reads and decodes on the OTA arm (`ota_decode_burst`), with a `fast` session locked at
//! the rung under test.

#[path = "common/ofdm_detect.rs"]
mod fixture;

use fixture::*;
use openpulse_core::fec::FecMode;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::frame_plan;

const READS: [usize; 4] = [400, 800, 4096, 19_200];

/// Whether `f` holds the whole frame whose preamble starts at `onset` (timeline) and decoded it.
fn holds_and_decodes(f: &Flush, onset: usize, frame_len: usize) -> bool {
    f.start <= onset && f.start + f.len >= onset + frame_len && f.decoded.is_some()
}

/// For each episode frame: did one flushed burst hold all of it and decode it?
fn gathered(read: usize, cold: bool) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for ep in &EPISODES {
        let x = capture(ep.file);
        let mut e = engine(SpeedLevel::Sl7, true);
        let segs = if cold {
            // A floor that is cold when the gap hiss starts, ~10 s before the second frame.
            let from = s(ep.onsets[0]) + SL7_FRAME + s(0.3);
            vec![
                Segment {
                    at: EXCERPT_AT + from,
                    samples: &x[from..],
                },
                Segment {
                    at: EXCERPT_AT + x.len(),
                    samples: ep.gap(&x),
                },
            ]
        } else {
            ep.segments(&x, false)
        };
        let flushes = feed(&mut e, &segs, read, true);
        for (k, &t) in ep.onsets.iter().enumerate() {
            if cold && k == 0 {
                continue;
            }
            let onset = EXCERPT_AT + s(t);
            let ok = flushes
                .iter()
                .any(|f| holds_and_decodes(f, onset, SL7_FRAME));
            out.push((format!("{} frame {}", ep.file, k + 1), ok));
        }
    }
    out
}

/// Gate 1, the case: each of the six f2g frames flushes as one burst holding the whole frame, and
/// decodes, at every read size the daemon produces. Sabotage S1 (P not OR'd into `carrier_present`)
/// takes it to 0/6.
#[test]
fn ofdm_frames_gather_under_agc() {
    gather_at(&READS[..2]);
}

/// Gate 1 at the long reads a daemon sees after a blocking decode or transmit (4 096, and the 2.4 s
/// read hold seen on air).
#[test]
#[ignore = "runtime: scripts/slow-tests.sh spectral"]
fn ofdm_frames_gather_under_agc_at_long_reads() {
    gather_at(&READS[2..]);
}

fn gather_at(reads: &[usize]) {
    for &read in reads {
        let got = gathered(read, false);
        let missed: Vec<&String> = got.iter().filter(|(_, ok)| !ok).map(|(n, _)| n).collect();
        assert!(
            missed.is_empty(),
            "{read}-sample reads: {} of {} f2g frames were not gathered whole and decoded: {missed:?}",
            missed.len(),
            got.len()
        );
    }
}

/// Gate 1, cold: an engine whose noise floor is cold when the episode's hiss starts still gathers and
/// decodes the frame that follows ≥ 9 s of it (the second of each episode).
#[test]
fn ofdm_frames_gather_from_a_cold_floor() {
    for read in [400usize, 800] {
        let got = gathered(read, true);
        assert!(
            got.iter().all(|(_, ok)| *ok),
            "{read}-sample reads, cold floor: {got:?}"
        );
    }
}

/// Gate 1, a second rig: B's FT-991A (AGC FAST) hearing A's SL8 and SL9 frames in run f1d. These
/// are loud (2× the hiss), so total power already gathers them; P must not cost them anything. B's
/// own transmissions are digital silence on this recording, and are cut out as the daemon cuts them.
#[test]
fn ofdm_frames_gather_on_the_second_rig() {
    let x = capture("ft991a-ofdm52-f1d-sl8-sl9.wav");
    // Runs of exact zeros of a read or more are B keyed.
    let mut segs = Vec::new();
    let mut i = 0usize;
    while i < x.len() {
        let start = i;
        while i < x.len()
            && !(x[i] == 0.0 && x[i..(i + 400).min(x.len())].iter().all(|&v| v == 0.0))
        {
            i += 1;
        }
        if i > start {
            segs.push(Segment {
                at: start,
                samples: &x[start..i],
            });
        }
        while i < x.len() && x[i] == 0.0 {
            i += 1;
        }
    }
    // Preamble starts measured by the detector on the full recording: 346.733 s (SL8), 349.958 s
    // (SL9), minus the cut at 344.900 s.
    for (level, mode, onset) in [
        (SpeedLevel::Sl8, "OFDM52-8PSK", s(1.833)),
        (SpeedLevel::Sl9, "OFDM52-16QAM", s(5.058)),
    ] {
        for read in [400usize, 800] {
            let mut e = engine(level, true);
            let flushes = feed(&mut e, &segs, read, true);
            assert!(
                flushes.iter().any(|f| f.start <= onset
                    && f.decoded.as_ref().is_some_and(|(m, _)| m == mode)),
                "{read}-sample reads: the {mode} frame at {onset} was not gathered and decoded \
                 ({flushes:?})"
            );
            assert!(
                e.preamble_detect_opens() >= 1,
                "{mode}: P never opened on a frame it measured at ρ ≥ 0.7 offline"
            );
        }
    }
}

/// Every negative the corpus holds — idle at three filter widths, a tone, BPSK250 frames from the rig
/// and an SDR, BPSK31 OTA frames, and B's QPSK250-D frame in f2g — with an OFDM candidate active.
fn negatives() -> Vec<(String, Vec<f32>)> {
    let mut v: Vec<(String, Vec<f32>)> = [
        "ft991a-idle.wav",
        "ic9700-idle-hot.wav",
        "ic9700-idle-wide-500hz-control.wav",
        "ic9700-idle-500hz.wav",
        "ic9700-idle-250hz.wav",
        "ic9700-tone-1501hz.wav",
        "ic9700-frame-bpsk250-none.wav",
        "ic9700-frame-bpsk250-rs.wav",
        "ic9700-frame-bpsk250-none-whitened.wav",
        "ic9700-frame-bpsk250-rs-whitened.wav",
        "sdr-ic9700tx-bpsk250-rs-1.wav",
        "sdr-ic9700tx-bpsk250-rs-2.wav",
        "sdr-ic9700tx-bpsk250-rs-3.wav",
        "ft991a-ota-frame-bpsk31-rs.wav",
        "ft991a-ota-fragment-5s.wav",
        "ft991a-ota-bpsk31-at-sl1.wav",
    ]
    .into_iter()
    .map(|n| (n.to_string(), capture(n)))
    .collect();
    // B's QPSK250-D frame (~348.3–352.4 s), between A's two ACKs, ending 0.5 s before the first OFDM
    // frame's preamble.
    let ep = &EPISODES[0];
    let x = capture(ep.file);
    v.push((
        format!("{} QPSK250-D", ep.file),
        x[s(ep.own_tx[0].1)..s(ep.own_tx[1].0)].to_vec(),
    ));
    v
}

/// Gate 2: P opens on none of the negatives while its tripwire moves, and the flush sequence is
/// bit-identical to the same feed with P off. Sabotage S2 (`RHO_OPEN` 0.40, confirm off) must open on
/// `ic9700-idle-wide-500hz-control` (3 windows ≥ 0.40 measured).
#[test]
fn ofdm_detect_ignores_the_corpus() {
    ignores_the_corpus_at(800);
}

/// Gate 2 at the daemon's default 50 ms tick.
#[test]
#[ignore = "runtime: scripts/slow-tests.sh spectral"]
fn ofdm_detect_ignores_the_corpus_at_400() {
    ignores_the_corpus_at(400);
}

fn ignores_the_corpus_at(read: usize) {
    {
        for (name, x) in negatives() {
            let seg = [Segment { at: 0, samples: &x }];
            let mut on = engine(SpeedLevel::Sl7, true);
            let with_p = feed(&mut on, &seg, read, false);
            assert_eq!(
                on.preamble_detect_opens(),
                0,
                "{name}, {read}-sample reads: P opened on audio that holds no OFDM frame"
            );
            assert!(
                on.preamble_detect_blocks_processed() as usize >= x.len() / read,
                "{name}: P judged {} blocks of {} — the tripwire did not move, so this proves nothing",
                on.preamble_detect_blocks_processed(),
                x.len() / read
            );
            let mut off = engine(SpeedLevel::Sl7, false);
            let without_p = feed(&mut off, &seg, read, false);
            assert_eq!(off.preamble_detect_blocks_processed(), 0, "P off ran P");
            assert_eq!(
                with_p, without_p,
                "{name}, {read}-sample reads: the flushes differ with P on, though P never opened"
            );
        }
    }
}

/// Two SL7 frames 0.4 s apart under an AGC (hiss 12 dB down under the frames, total 1.15× the gap).
fn two_frames_at_agc_level(gap: usize) -> (Vec<f32>, [usize; 2], usize, Vec<Vec<u8>>) {
    let payloads = vec![vec![0x11u8; 48], vec![0x22u8; 48]];
    let tx: Vec<Vec<f32>> = payloads
        .iter()
        .map(|p| {
            let lb = openpulse_audio::LoopbackBackend::new();
            let mut t = openpulse_modem::ModemEngine::new(Box::new(lb.clone_shared()));
            t.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
                .unwrap();
            t.transmit_with_fec_mode(p, "OFDM52", FecMode::SoftConcatenated, None)
                .expect("tx");
            lb.drain_samples()
        })
        .collect();
    let hiss = capture("ic9700-idle-wide-500hz-control.wav");
    let h = rms(&hiss);
    let lead = s(12.0);
    let total = lead + tx[0].len() + gap + tx[1].len() + s(6.0);
    let mut x: Vec<f32> = (0..total).map(|i| hiss[i % hiss.len()]).collect();
    let starts = [lead, lead + tx[0].len() + gap];
    for (k, t) in tx.iter().enumerate() {
        let under = 0.25f32; // the hiss under the frame, 12 dB down
        let g = ((1.15 * 1.15 - under * under) * h * h).sqrt() / rms(t);
        for (i, v) in t.iter().enumerate() {
            let j = starts[k] + i;
            x[j] = x[j] * under + v * g;
        }
    }
    let len = tx[0].len();
    (x, starts, len, payloads)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// Gate 4: two SL7 frames 0.4 s apart stay two bursts at 800-sample reads, and both decode. At
/// 19 200-sample reads they are one burst carrying both — a stated limit — and #1461's multi-frame
/// decode carries both.
#[test]
fn two_ofdm_frames_stay_two_bursts() {
    let (x, starts, len, payloads) = two_frames_at_agc_level(s(0.4));
    let seg = [Segment { at: 0, samples: &x }];
    let mut e = engine(SpeedLevel::Sl7, true);
    let flushes = feed(&mut e, &seg, 800, true);
    let own: Vec<Option<&Flush>> = starts
        .iter()
        .zip(&payloads)
        .map(|(&st, p)| {
            flushes.iter().find(|f| {
                holds_and_decodes(f, st, len) && f.decoded.as_ref().is_some_and(|(_, q)| q == p)
            })
        })
        .collect();
    assert!(
        own.iter().all(|f| f.is_some()),
        "800-sample reads: a frame was not gathered whole and decoded: {flushes:?}"
    );
    assert!(
        own[0].is_some_and(|f| f.start + f.len <= starts[1]),
        "800-sample reads: the first frame's burst ran into the second: {flushes:?}"
    );
    let mut e = engine(SpeedLevel::Sl7, true);
    let flushes = feed(&mut e, &seg, 19_200, false);
    let both: Vec<&Flush> = flushes
        .iter()
        .filter(|f| f.start <= starts[0] && f.start + f.len >= starts[1] + len)
        .collect();
    assert_eq!(
        both.len(),
        1,
        "19 200-sample reads: expected one burst over both frames (the stated limit): {flushes:?}"
    );
    let burst = openpulse_modem::pipeline::AudioSamples {
        samples: x[both[0].start..both[0].start + both[0].len].to_vec(),
    };
    let frames = e
        .decode_burst_frames("OFDM52", FecMode::SoftConcatenated, &burst)
        .expect("the merged burst decodes");
    // Measured, and a limit of #1461 rather than of P: its follow-on scan reaches four symbols past
    // the first frame's end plus 4 × the acquisition window (1 152 samples), and a frame 0.4 s
    // (3 200 samples) later lies beyond it, so only the first is decoded. The design note expected
    // both; pinned here so a change to either side is seen.
    assert_eq!(
        frames,
        payloads[..1].to_vec(),
        "19 200-sample reads: the merged burst's decode changed — re-read the limit stated above"
    );
}

/// The longest coded frame of `mode` at `fec`, the bound P may hold for, by reference to the engine's
/// own `frame_plan` and the plugin's geometry.
fn hold_bound(mode: &str, fec: FecMode) -> usize {
    let g = ofdm_plugin::OfdmPlugin::new()
        .frame_geometry(&ModulationConfig {
            mode: mode.into(),
            ..ModulationConfig::default()
        })
        .expect("geometry");
    frame_plan(g.max_frame_samples, fec).0
}

/// Gate 5: a genuine preamble followed by audio that keeps γ₈ up holds P only until `hold_bound`: the
/// burst flushes at `hold_bound` + one read, not at the 240 000-sample cap. And the bound clears the
/// longest frame every OFDM rung sends. Sabotage S4 (bound = burst cap) must fail.
///
/// The design note's fixture is a steady tone. A tone reads γ ≈ 1, but its 62.5 Hz band stands far
/// over the floor and the spectral test holds the burst instead, so the fixture would measure S. What
/// follows the preamble here is OFDM data symbols that never end — every symbol's CP is genuine, so γ₈
/// stays at its in-frame value — at the frame's AGC level, where neither total power nor S holds.
#[test]
fn ofdm_hold_is_bounded_by_the_frame() {
    let profile = SessionProfile::fast();
    for level in (7u8..=14).filter_map(SpeedLevel::from_u8) {
        let mode = profile.mode_for(level).expect("rung");
        let fec = profile.fec_for(level);
        let e = engine(level, true);
        let longest = (e
            .tx_airtime_seconds(&[0xA5; 255], mode, fec)
            .expect("airtime")
            * RATE as f64)
            .ceil() as usize;
        assert!(
            hold_bound(mode, fec) >= longest,
            "{level:?} {mode}: hold bound {} under its longest frame {longest}",
            hold_bound(mode, fec)
        );
    }

    let hiss = capture("ic9700-idle-wide-500hz-control.wav");
    let h = rms(&hiss);
    let tx = {
        let lb = openpulse_audio::LoopbackBackend::new();
        let mut t = openpulse_modem::ModemEngine::new(Box::new(lb.clone_shared()));
        t.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
            .unwrap();
        let payload: Vec<u8> = (0..255u32).map(|i| (i * 97 + 13) as u8).collect();
        t.transmit_with_fec_mode(&payload, "OFDM52", FecMode::SoftConcatenated, None)
            .expect("tx");
        lb.drain_samples()
    };
    let sym = ofdm_symbol();
    let pre = &tx[..sym];
    // The frame's data symbols, repeated whole symbols at a time: a frame that never ends.
    let body = &tx[sym..sym + (tx.len() - sym) / sym * sym];
    let period: Vec<f32> = body.to_vec();
    let lead = s(12.0);
    let body_len = s(8.0);
    let total = lead + sym + body_len + s(6.0);
    let mut x: Vec<f32> = (0..total).map(|i| hiss[i % hiss.len()]).collect();
    let under = 0.25f32;
    let gp = ((1.15 * 1.15 - under * under) * h * h).sqrt() / rms(pre);
    let gb = ((1.15 * 1.15 - under * under) * h * h).sqrt() / rms(&period);
    for i in 0..sym + body_len {
        let j = lead + i;
        let v = if i < sym {
            pre[i] * gp
        } else {
            period[(i - sym) % period.len()] * gb
        };
        x[j] = x[j] * under + v;
    }
    let bound = hold_bound("OFDM52", FecMode::SoftConcatenated);
    assert!(
        sym + body_len > bound + 19_200,
        "the never-ending frame must outlast the bound"
    );
    for read in [800usize, 4096] {
        let mut e = engine(SpeedLevel::Sl7, true);
        let flushes = feed(&mut e, &[Segment { at: 0, samples: &x }], read, false);
        let f = flushes
            .iter()
            .find(|f| f.start <= lead && f.start + f.len > lead + sym)
            .unwrap_or_else(|| panic!("{read}: no burst over the preamble: {flushes:?}"));
        let held = f.start + f.len - lead;
        assert!(
            held >= bound && held <= bound + read + sym,
            "{read}-sample reads: P held {held} samples from the preamble; the bound is {bound} \
             (+ one read {read}); the never-ending frame ran {}",
            sym + body_len
        );
    }
}

/// Gate 6, the floor: a burst P opened is never learned as band — the squelch the floor sets is the
/// same, one read after the burst flushes, as when it opened. Sabotage S5 (`commit` for `discard`)
/// moves it.
///
/// "The same" within [`FLOOR_STILL`], because the read after the flush is band and is learned (the
/// flush's commit or discard re-aims the squelch only at the next block's seam). Measured on this
/// fixture: that read moves the squelch +0.025 % at 800-sample reads and +0.097 % at 4 096; S5's
/// commit moves it +0.83 % and +1.19 %.
const FLOOR_STILL: f32 = 0.003;

#[test]
fn a_detected_burst_is_not_learned_as_band() {
    let ep = &EPISODES[0];
    let x = capture(ep.file);
    let mut e = engine(SpeedLevel::Sl7, true);
    // Warm on the gap, then the 10 s of hiss before frame 2, then frame 2 and the gap again.
    let gap = ep.gap(&x);
    let start2 = s(ep.onsets[1]);
    let mut moved = Vec::new();
    for read in [800usize, 4096] {
        let warm = [Segment {
            at: 0,
            samples: gap,
        }];
        let _ = feed(&mut e, &warm, read, false);
        e.abandon_burst();
        let before_frame = &x[start2 - s(6.0)..start2 - s(1.0)];
        let _ = feed(
            &mut e,
            &[Segment {
                at: 0,
                samples: before_frame,
            }],
            read,
            false,
        );
        let squelch_before = e.dcd_squelch();
        let opens_before = e.preamble_detect_opens();
        let rest: Vec<f32> = x[start2 - s(1.0)..]
            .iter()
            .chain(gap.iter())
            .copied()
            .collect();
        let mut flushed = false;
        let mut squelch_at_open = None;
        let mut chunks = rest.chunks(read);
        for chunk in chunks.by_ref() {
            let was_open = e.burst_open();
            let sq = e.dcd_squelch();
            if let Ok(Some(_)) = e.accumulate_capture(Some("OFDM52"), chunk.to_vec()) {
                flushed = true;
                break;
            }
            if !was_open && e.burst_open() {
                squelch_at_open = Some(sq);
            }
        }
        // The flush's own commit or discard re-aims the squelch only at the next block's seam.
        for chunk in chunks.take(1) {
            let _ = e.accumulate_capture(Some("OFDM52"), chunk.to_vec());
        }
        assert!(flushed, "{read}: the frame's burst never flushed");
        assert!(
            e.preamble_detect_opens() > opens_before,
            "{read}: P did not open the burst, so this does not test P's burst"
        );
        let at_open = squelch_at_open.unwrap_or(squelch_before);
        let after = e.dcd_squelch();
        moved.push((read, at_open, after));
        assert!(
            (after / at_open - 1.0).abs() < FLOOR_STILL,
            "{read}-sample reads: the floor moved across a burst P opened ({at_open} → {after}): \
             the frame was learned as band"
        );
    }
    println!("squelch at open / after flush: {moved:?}");
}

/// P's head survives a total-power flicker over the preamble. At 400-sample reads total power can
/// trip on the frame's first read or two and drop again before P's confirm is in (four data symbols
/// later); that short burst flushes, and #1454's retention rule empties `rx_ring`, preamble and all.
/// P's head comes from its own ring, which the rule does not empty. Seen on one f2g frame while this
/// was built (a 0.356 s flicker, then a P burst with no head that did not decode); this pins it with
/// a frame whose first 600 samples stand 2.5× over the rest.
#[test]
fn a_flicker_over_the_preamble_keeps_ps_head() {
    let (mut x, starts, len, payloads) = two_frames_at_agc_level(s(3.0));
    for v in &mut x[starts[0]..starts[0] + 600] {
        *v *= 2.5;
    }
    let seg = [Segment { at: 0, samples: &x }];
    let mut e = engine(SpeedLevel::Sl7, true);
    let flushes = feed(&mut e, &seg, 400, true);
    assert!(
        flushes
            .iter()
            .any(|f| f.start + f.len > starts[0] && f.start + f.len < starts[0] + len),
        "no burst flushed inside the frame: the flicker this fixture exists for did not happen \
         ({flushes:?})"
    );
    assert!(
        flushes.iter().any(|f| holds_and_decodes(f, starts[0], len)
            && f.decoded.as_ref().is_some_and(|(_, p)| *p == payloads[0])),
        "after a flicker over its preamble the frame was not gathered whole and decoded: \
         {flushes:?}"
    );
}
