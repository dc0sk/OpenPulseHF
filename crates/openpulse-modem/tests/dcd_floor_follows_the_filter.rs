//! The daemon's carrier detect follows the band behind ANY receive filter (#1452, REQ-DCD-01).
//!
//! Before #1452 the squelch floor was a low percentile ACROSS the 300–2700 Hz bins, scaled as if the
//! noise were white across that band. Behind a 500 Hz or 250 Hz receive filter most of those bins are
//! stopband, so the floor read the stopband and the squelch collapsed to its clamp: every block read
//! as carrier, bursts flushed only at the 37 s runaway cap, and a frame the decoder could read lay
//! past the onset scan's reach (measured through this same entry: production 0/8, the same slab
//! decoded from the frame's offset 5/8; the other three trials fed less audio than the cap, so no
//! burst was ever flushed).
//!
//! Everything here drives the production entry (`accumulate_capture`) on the recorded IC-9700 and
//! FT-991A idle captures. Frame levels are set by the noise INSIDE the frame's band, computed here
//! from the capture itself — behind a narrow filter nearly all the noise is in-band, so a total-power
//! ratio would put the narrow-filter frames several dB below the wide-filter ones.
//!
//! The receiver notch is OFF here (the engine default; the daemon's config default is on), so the
//! floor measured is the band's, birdies included.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::pipeline::AudioSamples;
use openpulse_modem::ModemEngine;
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

const TICK: usize = 400;

/// Longest idle tail the spectral hold can add after a loud burst ends (#1454). MEASURED, not derived:
/// 4 000 on the loud fixtures here under the capped hold (the 16 000-sample cells read 3 200). It is
/// window alignment, the open test's own lookback, the capped hold's four windows and one read —
/// about 511 + 6·512 + 400 — but the constant is the measurement. The cap bounds it at any level;
/// uncapped, one loud window held the mean for seven more windows and this was 9·512 + TICK.
const SPECTRAL_TAIL_MAX: usize = 4_000;

/// The tail on a loud read of three or more windows (which opens the spectral test on every band) is
/// within one window and one read of `SPECTRAL_TAIL_MAX` — so the constant is pinned from BOTH sides,
/// and cannot grow unnoticed.
fn assert_tail(post: usize, loud: usize, what: &str) {
    assert!(
        post >= loud && post - loud <= SPECTRAL_TAIL_MAX,
        "{what}: burst of {post} for {loud} loud samples"
    );
    if loud >= 3 * 512 {
        assert!(
            post - loud + 512 + TICK >= SPECTRAL_TAIL_MAX,
            "{what}: tail {} is more than a window and a read under SPECTRAL_TAIL_MAX ({SPECTRAL_TAIL_MAX})",
            post - loud
        );
    }
}
const RATE: f32 = 8000.0;

fn engine() -> (ModemEngine, LoopbackBackend) {
    let lb = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    (e, lb)
}

fn corpus(name: &str) -> Capture {
    load_corpus(name).unwrap_or_else(|e| panic!("corpus file {name} must load: {e}"))
}

fn rms(x: &[f32]) -> f32 {
    let m = x.iter().sum::<f32>() / x.len() as f32;
    (x.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / x.len() as f32).sqrt()
}

/// Share of `x`'s power inside `lo..hi` Hz, from averaged 4096-point periodograms.
fn in_band_share(x: &[f32], lo: f32, hi: f32) -> f32 {
    const N: usize = 4096;
    let fft = FftPlanner::<f32>::new().plan_fft_forward(N);
    let (mut band, mut total) = (0.0f64, 0.0f64);
    for seg in x.as_chunks::<N>().0 {
        let m = seg.iter().sum::<f32>() / N as f32;
        let mut buf: Vec<Complex32> = seg.iter().map(|&v| Complex32::new(v - m, 0.0)).collect();
        fft.process(&mut buf);
        for (k, c) in buf.iter().enumerate().take(N / 2).skip(1) {
            let f = k as f32 * RATE / N as f32;
            let p = c.norm_sqr() as f64;
            total += p;
            if (lo..=hi).contains(&f) {
                band += p;
            }
        }
    }
    (band / total) as f32
}

fn frame(payload: &[u8], mode: &str) -> Vec<f32> {
    let (mut e, lb) = engine();
    e.transmit_with_fec_mode(payload, mode, FecMode::Rs, None)
        .expect("tx");
    lb.drain_samples()
}

/// Half the occupied band of each BPSK rung used here (its baud).
fn half_band(mode: &str) -> f32 {
    match mode {
        "BPSK250" => 250.0,
        "BPSK63" => 63.0,
        "BPSK31" => 31.25,
        _ => panic!("no band for {mode}"),
    }
}

struct Trial {
    decoded: bool,
    decodable: bool,
    bursts_over_frame: usize,
}

/// One frame superimposed on `idle` at `in_band_db`, starting `lead` samples after a 3 s warm-up,
/// fed through the production entry in `TICK` reads. `decodable` decodes the frame plus 400 samples
/// of its own idle directly (no carrier detect) — the control that the level is decodable at all.
fn trial(
    idle: &Capture,
    from: usize,
    mode: &str,
    payload: &[u8],
    in_band_db: f32,
    lead: usize,
) -> Trial {
    let tx = frame(payload, mode);
    let warm = 3 * TICK * 20;
    let total = warm + lead + tx.len() + 5 * 8000;
    let mut buf = idle.cycled(from, total);
    let noise = rms(&buf[..warm]).powi(2)
        * in_band_share(
            &buf[..warm],
            1500.0 - half_band(mode),
            1500.0 + half_band(mode),
        );
    let fms = tx.iter().map(|v| v * v).sum::<f32>() / tx.len() as f32;
    let g = (noise * 10f32.powf(in_band_db / 10.0) / fms).sqrt();
    let f0 = warm + lead;
    for (i, s) in tx.iter().enumerate() {
        buf[f0 + i] += s * g;
    }
    let direct = buf[f0 - 400..f0 + tx.len() + 400].to_vec();
    let decodable = {
        let (mut e, _l) = engine();
        matches!(e.decode_burst_with_fec(mode, FecMode::Rs, &AudioSamples { samples: direct }), Ok(p) if p == payload)
    };
    let (mut e, _lb) = engine();
    let (mut fed, mut decoded, mut over) = (0usize, false, 0usize);
    for chunk in buf.chunks(TICK) {
        let before = fed;
        fed += chunk.len();
        if let Ok(Some(b)) = e.accumulate_capture(Some(mode), chunk.to_vec()) {
            let start = before.saturating_sub(b.samples.len());
            if start < f0 + tx.len() && before > f0 {
                over += 1;
                decoded |=
                    matches!(e.decode_burst_with_fec(mode, FecMode::Rs, &b), Ok(p) if p == payload);
            }
        }
    }
    Trial {
        decoded,
        decodable,
        bursts_over_frame: over,
    }
}

fn preamble_of(plugin: &dyn openpulse_core::plugin::ModulationPlugin, mode: &str) -> usize {
    plugin
        .frame_geometry(&openpulse_core::plugin::ModulationConfig {
            mode: mode.into(),
            ..openpulse_core::plugin::ModulationConfig::default()
        })
        .expect("the mode publishes its frame geometry")
        .preamble_samples
}

fn bpsk250_preamble() -> usize {
    preamble_of(&BpskPlugin::new(), "BPSK250")
}

fn payload(n: usize) -> Vec<u8> {
    (0..n as u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect()
}

const IDLES: [&str; 5] = [
    "ic9700-idle-hot.wav",
    "ic9700-idle-wide-500hz-control.wav",
    "ic9700-idle-500hz.wav",
    "ic9700-idle-250hz.wav",
    "ft991a-idle.wav",
];

/// THE #1452 GATE: on every recorded idle — wide, 500 Hz, 250 Hz, and a quiet rig — the squelch sits
/// at the intended margin over the idle level, at the daemon's read sizes.
///
/// Before #1452 the two narrow captures read squelch/idle ≈ 0.014 and 0.022 (the 0.001 clamp against
/// 0.071 / 0.045 RMS), the wide ones ≈ 1.37 (a floor biased 1.1× high), and the FT-991A 1.67 (the
/// clamp). The range [1.20, 1.35] is the 1.25 margin with the estimator's measured error and its
/// quantile noise; it admits none of those.
// VERIFIES: REQ-DCD-01 — the squelch tracks the noise power the block RMS sees, behind any filter
#[test]
fn the_squelch_sits_at_its_margin_over_every_recorded_idle() {
    let mut outside = Vec::new();
    for name in IDLES {
        let idle = corpus(name);
        let audio = idle.cycled(0, 30 * 8000);
        let level = rms(&audio);
        for block in [171usize, TICK, 512, 4096] {
            let (mut e, _lb) = engine();
            for chunk in audio.chunks(block) {
                let _ = e.accumulate_capture(Some("BPSK250"), chunk.to_vec());
            }
            let ratio = e.dcd_squelch() / level;
            println!(
                "{name} block {block}: squelch {:.5} idle {level:.5} ratio {ratio:.3}",
                e.dcd_squelch()
            );
            if !(1.20..=1.35).contains(&ratio) {
                outside.push(format!("{name} at {block}-sample reads: {ratio:.3}"));
            }
        }
    }
    assert!(
        outside.is_empty(),
        "squelch/idle outside [1.20, 1.35]:\n{}",
        outside.join("\n")
    );
}

/// Idle audio never holds the carrier open: no burst reaches the runaway cap, on any capture.
///
/// This is what the narrow captures did before #1452 — every block read as carrier.
#[test]
fn idle_never_holds_the_carrier_open() {
    for name in IDLES {
        let idle = corpus(name);
        let audio = idle.cycled(0, 60 * 8000);
        let (mut e, _lb) = engine();
        let preamble = bpsk250_preamble();
        let mut gathered = 0usize;
        let mut longest = 0usize;
        for chunk in audio.chunks(TICK) {
            if let Ok(Some(b)) = e.accumulate_capture(Some("BPSK250"), chunk.to_vec()) {
                gathered += b.samples.len();
                longest = longest.max(b.samples.len());
            }
        }
        let share = gathered as f32 / audio.len() as f32;
        println!(
            "{name}: longest idle burst {longest}, {:.1} % of idle gathered",
            100.0 * share
        );
        assert!(
            longest < preamble,
            "{name}: an idle burst of {longest} samples could hold a {preamble}-sample preamble"
        );
        assert!(
            share < 0.10,
            "{name}: {:.1} % of pure idle read as carrier",
            100.0 * share
        );
    }
}

/// A frame the decoder can read, behind a 500 Hz filter, now decodes through the daemon's path.
///
/// Before #1452: production 0/8 at in-band +8 and +12 dB, while the same slab decoded from the
/// frame's offset 5/8 (the other three trials fed less audio than the cap, so no burst was ever
/// flushed) — the frame was buried in a cap-flushed slab past the onset scan.
#[test]
fn a_frame_behind_a_500hz_filter_decodes_through_the_daemon_path() {
    let idle = corpus("ic9700-idle-500hz.wav");
    let p = payload(64);
    let (mut ok, mut decodable) = (0, 0);
    for t in 0..8usize {
        let r = trial(&idle, t * 9000, "BPSK250", &p, 12.0, 4000 + t * 17_137);
        println!(
            "t{t}: decoded {} decodable {} bursts {}",
            r.decoded, r.decodable, r.bursts_over_frame
        );
        ok += r.decoded as u32;
        decodable += r.decodable as u32;
    }
    assert_eq!(
        decodable, 8,
        "the level is not decodable at all — this fixture proves nothing"
    );
    assert!(
        ok >= 7,
        "{ok}/8 decoded through accumulate_capture behind a 500 Hz filter"
    );
}

/// A long frame does not close its own burst (#1304's shape, on ordinary traffic).
///
/// A two-block BPSK250 + Rs frame (220 B, ~16.5 s) behind a 500 Hz filter carries most of the band's
/// power; a floor that learned from it would climb until the squelch passed the frame's own level
/// and the burst closed mid-frame. The tracker is held while a burst is gathered, so it cannot.
///
/// The frame must be longer than the one that proved too short: at 200 B (ONE RS block, ~8 s; the
/// boundary is 209 B) this test passed with the hold disabled, at the same +8 dB level. At 220 B it
/// fails 0/8 without the hold (sabotage S1). How long a frame must be to raise an unheld floor
/// depends on its level as well as its length, so the sabotage is the evidence, not a formula.
#[test]
fn a_long_frame_is_gathered_as_one_burst() {
    let idle = corpus("ic9700-idle-500hz.wav");
    let p = payload(220);
    let len = frame(&p, "BPSK250").len();
    let too_short = frame(&payload(200), "BPSK250").len();
    assert!(
        len > too_short,
        "the fixture frame ({len} samples) is no longer than the one-block frame ({too_short}) that \
         passed with the hold disabled — vacuous"
    );
    let (mut ok, mut decodable, mut split) = (0, 0, 0);
    for t in 0..8usize {
        let r = trial(&idle, t * 9000, "BPSK250", &p, 8.0, 4000 + t * 17_137);
        println!(
            "t{t}: decoded {} decodable {} bursts {}",
            r.decoded, r.decodable, r.bursts_over_frame
        );
        ok += r.decoded as u32;
        decodable += r.decodable as u32;
        split += (r.bursts_over_frame != 1) as u32;
    }
    assert_eq!(
        decodable, 8,
        "the level is not decodable at all — this fixture proves nothing"
    );
    assert_eq!(
        split, 0,
        "{split}/8 long frames were not gathered as exactly one burst"
    );
    assert!(ok >= 7, "{ok}/8 long frames decoded");
}

/// The operator's squelch is a LOWER bound: it raises the threshold and survives the adaptive seam
/// (before #1452 it was overwritten within one window), and a value below the band cannot lower it.
#[test]
fn the_operator_squelch_raises_the_threshold_and_never_lowers_it() {
    let audio = corpus("ic9700-idle-hot.wav").cycled(0, 10 * 8000);
    let (mut e, _lb) = engine();
    e.set_dcd_squelch(0.5);
    for chunk in audio.chunks(TICK) {
        let _ = e.accumulate_capture(Some("BPSK250"), chunk.to_vec());
    }
    assert_eq!(
        e.dcd_squelch(),
        0.5,
        "the operator squelch did not survive 10 s of audio"
    );

    let (mut e, _lb) = engine();
    for chunk in audio.chunks(TICK) {
        let _ = e.accumulate_capture(Some("BPSK250"), chunk.to_vec());
    }
    let adaptive = e.dcd_squelch();
    e.set_dcd_squelch(0.001);
    assert_eq!(
        e.dcd_squelch(),
        adaptive,
        "an operator value below the band lowered the squelch — the receiver can be made deaf"
    );
}

/// Idle flicker behind a narrow filter is not ladder evidence, and the rule is not wholesale.
///
/// With a correct floor the squelch sits ~2σ above a 250 Hz filter's block-RMS spread, so idle trips
/// a few percent of blocks. Before #1452 such a filter never flushed at all; each flicker must not now
/// drive a NACK or a demotion. A failed burst longer than a candidate's recognition window still does.
#[test]
fn idle_flicker_is_not_ladder_evidence_but_a_longer_failure_still_is() {
    let idle = corpus("ic9700-idle-250hz.wav");
    let (mut e, _lb) = engine();
    e.start_ota_session(SessionProfile::hpx_hf());
    let before = e.ota_rx_recommended_level().expect("session");

    let short = AudioSamples {
        samples: idle.cycled(0, 2 * TICK),
    };
    let r = e
        .ota_decode_burst(&short, "flicker", Some("BPSK250"))
        .expect("decode");
    assert!(
        r.payload.is_none() && r.ack.is_none(),
        "a two-block idle burst produced an ACK"
    );
    assert_eq!(
        e.ota_rx_recommended_level(),
        Some(before),
        "the ladder moved on a flicker"
    );

    // Positive control: longer than every hpx_hf entry candidate's recognition window (BPSK31's is 8 448).
    let long = AudioSamples {
        samples: idle.cycled(0, 16_000),
    };
    let r = e
        .ota_decode_burst(&long, "flicker", Some("BPSK250"))
        .expect("decode");
    assert!(
        r.ack.is_some(),
        "a 16 000-sample failed burst produced no ACK — the rule is wholesale"
    );
}

/// Audio learned on a one-shot path while a burst is held is not committed with that burst (#1452).
///
/// A burst's hold ends only when `accumulate_capture` sees the carrier drop. Between the two, a
/// one-shot receive (an ACK listen, or reads during a transmit) feeds the same tracker, so the hold
/// can end up far longer than the burst. The flicker rule commits a sub-preamble burst's hold as
/// band; committing a hold that is mostly something else would teach the floor that instead — here,
/// ten seconds of silence, which would collapse the squelch.
#[test]
fn a_hold_longer_than_its_burst_is_not_committed() {
    let idle = corpus("ic9700-idle-hot.wav");
    let run = |stale: bool| {
        let (mut e, lb) = engine();
        for chunk in idle.cycled(0, 20 * 8000).chunks(TICK) {
            let _ = e.accumulate_capture(Some("BPSK250"), chunk.to_vec());
        }
        let before = e.dcd_squelch();
        let loud: Vec<f32> = idle.cycled(0, TICK).iter().map(|v| v * 10.0).collect();
        let _ = e.accumulate_capture(Some("BPSK250"), loud);
        if stale {
            lb.fill_samples(&vec![0.0; 10 * 8000]);
            let _ = e.receive("BPSK250", None);
        }
        let mut flushed = 0;
        for chunk in idle.cycled(20 * 8000, 4 * TICK).chunks(TICK) {
            if let Ok(Some(b)) = e.accumulate_capture(Some("BPSK250"), chunk.to_vec()) {
                flushed = b.samples.len();
            }
        }
        (before, e.dcd_squelch(), flushed)
    };
    let (b0, a0, f0) = run(false);
    println!("control: squelch {b0:.5} -> {a0:.5}, burst {f0}");
    assert_eq!(f0, TICK, "the control did not flush a one-block burst");
    assert!(a0 / b0 > 0.9, "the flicker commit alone moved the squelch");
    let (b1, a1, f1) = run(true);
    println!("stale hold: squelch {b1:.5} -> {a1:.5}, burst {f1}");
    assert_eq!(f1, TICK, "the stale case did not flush a one-block burst");
    assert!(
        a1 / b1 > 0.9,
        "a hold holding 10 s of one-shot silence was committed: squelch {b1:.5} -> {a1:.5}"
    );
}

/// The floor follows the band UP at the cap flush, and back DOWN afterwards (#1452).
///
/// A step up reads as carrier while the floor is held, so the squelch recovers only at the runaway
/// cap. What must not happen after that flush is a hold no burst owns: the seam started one on the
/// next block, re-aimed the squelch above it, no burst opened, and the floor froze at the committed
/// value — measured at 2.49x the idle a minute after the band dropped back 6 dB.
#[test]
fn the_floor_follows_the_band_up_at_the_cap_and_back_down() {
    let idle = corpus("ic9700-idle-hot.wav");
    let quiet: Vec<f32> = idle.cycled(0, 20 * 8000).iter().map(|v| v * 0.5).collect();
    let loud = idle.cycled(0, 60 * 8000);
    let quiet_again: Vec<f32> = idle.cycled(0, 40 * 8000).iter().map(|v| v * 0.5).collect();
    let (lq, ll) = (rms(&quiet), rms(&loud));
    let (mut e, _lb) = engine();
    let feed = |e: &mut ModemEngine, x: &[f32]| {
        let mut flushed = Vec::new();
        for c in x.chunks(TICK) {
            if let Ok(Some(b)) = e.accumulate_capture(Some("BPSK250"), c.to_vec()) {
                flushed.push(b.samples.len());
            }
        }
        flushed
    };
    feed(&mut e, &quiet);
    let before = e.dcd_squelch() / lq;
    let during = feed(&mut e, &loud);
    let after_up = e.dcd_squelch() / ll;
    feed(&mut e, &quiet_again);
    let after_down = e.dcd_squelch() / lq;
    println!(
        "quiet {before:.3}; loud: flushes {during:?}, then {after_up:.3}; quiet again {after_down:.3}"
    );
    let cap = e.burst_cap_samples(Some("BPSK250"));
    let first = during.first().copied().unwrap_or(0);
    assert!(
        (cap..cap + TICK).contains(&first),
        "the step up did not end at the cap flush ({first} samples, cap {cap})"
    );
    assert!(
        (1.20..=1.35).contains(&after_up),
        "{after_up:.3}x the louder band after the cap flush"
    );
    assert!(
        (1.20..=1.35).contains(&after_down),
        "{after_down:.3}x the quiet band 40 s after it dropped back — the floor did not follow"
    );
}

/// Two transmissions 0.4 s apart are two bursts, at any level (#1454 round 9). Every arm decodes one
/// frame per burst, so a merged pair loses its second frame. Total power alone ends a burst on the
/// first quiet read; the spectral hold adds a tail, and uncapped that tail was ~0.55 s after a STRONG
/// frame (one window ≥ 13× the floor carried the 8-window mean), which merged exactly this pair. The
/// 0.2 s pair is the negative control: inside the tail it is one burst, so the gate can fail both ways.
#[test]
fn two_transmissions_four_tenths_of_a_second_apart_are_two_bursts() {
    let idle = corpus("ic9700-idle-wide-500hz-control.wav");
    let f = {
        let (mut e, lb) = engine();
        e.transmit(b"first frame", "BPSK250", None).expect("tx");
        lb.drain_samples()
    };
    let warm = 24_000;
    let fr = rms(&f);
    for db in [8.0f32, 12.0, 20.0] {
        for (gap, want) in [(3_200usize, 2usize), (1_600, 1)] {
            let mut buf = idle.cycled(0, warm + 5_000 + 2 * f.len() + gap + 5 * 8000);
            let noise = rms(&buf[..warm]).powi(2)
                * in_band_share(
                    &buf[..warm],
                    1500.0 - half_band("BPSK250"),
                    1500.0 + half_band("BPSK250"),
                );
            let g = (noise * 10f32.powf(db / 10.0)).sqrt() / fr;
            let (a, b) = (warm + 5_000, warm + 5_000 + f.len() + gap);
            for (i, s) in f.iter().enumerate() {
                buf[a + i] += s * g;
                buf[b + i] += s * g;
            }
            let (mut e, _lb) = engine();
            let (mut fed, mut bursts) = (0usize, 0usize);
            for chunk in buf.chunks(TICK) {
                fed += chunk.len();
                if let Ok(Some(burst)) = e.accumulate_capture(Some("BPSK250"), chunk.to_vec()) {
                    let end = fed - chunk.len();
                    let start = end - burst.samples.len();
                    bursts += (start < b + f.len() && end > a) as usize;
                }
            }
            assert_eq!(
                bursts, want,
                "+{db} dB, frames {gap} samples apart: {bursts} bursts over the pair, expected {want}"
            );
        }
    }
}

/// The recognition window (acquisition window plus one symbol) of the mode `profile` runs at
/// `level`, from the plugin that ships it — the bound the not-evidence rule reads, from the same
/// two geometry fields, summed as the engine's `frame_scan_geometry` sums them.
fn recognition_window(profile: &SessionProfile, level: openpulse_core::rate::SpeedLevel) -> usize {
    use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
    let mode = profile.mode_for(level).expect("the profile maps the level");
    let plugin: Box<dyn ModulationPlugin> = if mode.starts_with("OFDM") {
        Box::new(ofdm_plugin::OfdmPlugin::new())
    } else if mode.starts_with("QPSK") {
        Box::new(qpsk_plugin::QpskPlugin::new())
    } else if mode.starts_with("MFSK16") {
        Box::new(mfsk16_plugin::Mfsk16Plugin::new())
    } else if mode.starts_with("PILOT") {
        Box::new(pilot_plugin::PilotPlugin::new())
    } else {
        Box::new(BpskPlugin::new())
    };
    let g = plugin
        .frame_geometry(&ModulationConfig {
            mode: mode.into(),
            ..ModulationConfig::default()
        })
        .expect("the rung publishes its frame geometry");
    g.preamble_samples.max(g.symbol_period_samples) + g.symbol_period_samples
}

fn hpx_window(level: openpulse_core::rate::SpeedLevel) -> usize {
    recognition_window(&SessionProfile::hpx_hf(), level)
}

/// Feed warm idle, then loud reads of the given sizes, then idle until the burst flushes; decode it
/// under an `hpx_hf` session (locked to `level` if given). Returns (post-trigger burst length, ACK
/// keyed): a failed burst that counts as ladder evidence keys an ACK; one that does not, keys nothing.
/// The length excludes the pre-trigger ring, which a one-read onset of three or more windows carries
/// even though total power opened it (#1454).
fn ota_verdict(level: Option<openpulse_core::rate::SpeedLevel>, reads: &[usize]) -> (usize, bool) {
    ota_verdict_on(SessionProfile::hpx_hf(), level, reads)
}

fn ota_verdict_on(
    profile: SessionProfile,
    level: Option<openpulse_core::rate::SpeedLevel>,
    reads: &[usize],
) -> (usize, bool) {
    let idle = corpus("ic9700-idle-hot.wav");
    let (mut e, _lb) = engine();
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .expect("register");
    e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
        .expect("register");
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .expect("register");
    e.register_plugin(Box::new(pilot_plugin::PilotPlugin::new()))
        .expect("register");
    e.start_ota_session(profile);
    if let Some(l) = level {
        e.ota_lock_level(l);
    }
    for chunk in idle.cycled(0, 5 * 8000).chunks(TICK) {
        let _ = e.accumulate_capture(Some("BPSK250"), chunk.to_vec());
    }
    let total: usize = reads.iter().sum();
    let loud: Vec<f32> = idle
        .cycled(5 * 8000, total)
        .iter()
        .map(|v| v * 10.0)
        .collect();
    let mut burst = None;
    let mut at = 0;
    for &n in reads {
        if let Ok(Some(b)) = e.accumulate_capture(Some("BPSK250"), loud[at..at + n].to_vec()) {
            burst.get_or_insert(b);
        }
        at += n;
    }
    // Twice the longest tail, so the flush is decided by the carrier detect and never by the end of
    // this feed — otherwise the length assertions against `SPECTRAL_TAIL_MAX` could not fail.
    for chunk in idle.cycled(8 * 8000, 2 * SPECTRAL_TAIL_MAX).chunks(TICK) {
        if let Ok(Some(b)) = e.accumulate_capture(Some("BPSK250"), chunk.to_vec()) {
            burst.get_or_insert(b);
        }
    }
    let b = burst.expect("the loud reads must flush a burst");
    // Read before the decode, which takes it.
    let lead = e.last_flush_lead();
    let r = e
        .ota_decode_burst(&b, "flicker", Some("BPSK250"))
        .expect("decode");
    (b.samples.len() - lead, r.ack.is_some())
}

/// A failed burst shorter than every candidate's recognition window is not ladder evidence, and the verdict
/// depends on the burst's DURATION, never on how many reads delivered it (#1452).
///
/// At SL9 (OFDM52-16QAM, window 576) a 400-sample flicker drops and an 800-sample one counts, whether
/// it came as one read or several; at SL5 (BPSK250, window 1 056) 800 drops either way. The one-read
/// OFDM flicker drops only because the default `receive_tick_ms = 50` gives 400-sample reads; at a
/// longer tick every one-read flicker at SL7+ counts. That is a rate, not a category: longer flickers
/// count, which is accepted here and tracked on the controller side (#1456).
#[test]
fn a_flicker_is_judged_by_its_duration_not_its_reads() {
    use openpulse_core::rate::SpeedLevel::{Sl5, Sl9};
    let (f9, f5) = (hpx_window(Sl9), hpx_window(Sl5));
    assert!(
        TICK < f9 && f9 < 2 * TICK && 2 * TICK < f5,
        "the fixture needs 400 < SL9's window ({f9}) < 800 < SL5's window ({f5}); a changed read size \
         (`receive_tick_ms`) or geometry makes these cells prove nothing"
    );
    let cells: [(_, &[usize], bool); 8] = [
        (Sl9, &[400], false),
        (Sl9, &[100, 100, 100, 100], false),
        (Sl9, &[800], true),
        (Sl9, &[400, 400], true),
        (Sl5, &[800], false),
        (Sl5, &[400, 400], false),
        (Sl9, &[16_000], true),
        (Sl5, &[16_000], true),
    ];
    for (level, reads, counts) in cells {
        let (len, ack) = ota_verdict(Some(level), reads);
        println!("{level:?} reads {reads:?}: burst {len}, ack {ack}");
        let sum = reads.iter().sum::<usize>();
        assert_tail(len, sum, &format!("{level:?} {reads:?}"));
        assert_eq!(
            ack, counts,
            "{level:?} reads {reads:?}: counted {ack}, expected {counts}"
        );
    }
    let (len, ack) = ota_verdict(None, &[16_000]);
    println!("entry rungs, one read of 16 000: burst {len}, ack {ack}");
    assert!(
        ack,
        "a one-read 16 000-sample failure at the entry rungs was dropped"
    );
}

/// The bound follows the session's CANDIDATES, not the whole profile, and sits exactly at their scan
/// floor: a fade-split fragment as long as the floor is still evidence (#1452).
#[test]
fn the_not_evidence_bound_is_the_candidates_recognition_window() {
    use openpulse_core::rate::SpeedLevel::{Sl3, Sl9};
    let (f3, f9) = (hpx_window(Sl3), hpx_window(Sl9));
    assert!(
        2_000 < f3 && f9 < 2_000,
        "the fixture needs SL9's window ({f9}) < 2 000 < SL3's ({f3})"
    );
    let (_, ack) = ota_verdict(Some(Sl3), &[2_000]);
    assert!(
        !ack,
        "a 2 000-sample failure counted at SL3, whose window is {f3}"
    );
    let (_, ack) = ota_verdict(Some(Sl9), &[2_000]);
    assert!(
        ack,
        "a 2 000-sample failure was dropped at SL9, whose window is {f9}"
    );
    let (_, ack) = ota_verdict(Some(Sl3), &[f3]);
    assert!(ack, "a failure exactly at SL3's window ({f3}) was dropped");
    let (_, ack) = ota_verdict(Some(Sl3), &[f3 - 1]);
    assert!(
        !ack,
        "a failure one sample under SL3's window ({f3}) counted"
    );
}

/// At SL1 the bound is MFSK16's recognition window, not its scan floor (#1452): MFSK16 sends one
/// fixed 17 s frame, so its `min_frame_samples` is the whole frame, and with SL1 the sole candidate
/// a bound taken from it would drop every fade-split fragment — no NACK keyed, and the sender
/// abandons after two silent windows. The edge is pinned from both sides: the preamble alone (1 792)
/// counts 2 047, `min_frame_samples` drops 2 048.
#[test]
fn at_sl1_the_bound_is_mfsk16s_recognition_window() {
    use openpulse_core::rate::SpeedLevel::Sl1;
    let w = hpx_window(Sl1);
    assert_eq!(
        w, 2_048,
        "MFSK16's recognition window moved (Costas length or tone spacing changed); re-derive the cells"
    );
    for (len, counts) in [(1_000, false), (w - 1, false), (w, true), (4_000, true)] {
        let (got, ack) = ota_verdict(Some(Sl1), &[len]);
        println!("SL1, {len}: burst {got}, ack {ack}");
        assert_tail(got, len, &format!("SL1, {len}"));
        assert_eq!(ack, counts, "SL1, {len} samples");
    }
}

/// The pilot profiles' bound fell with the switch from `min_frame_samples` (#1452): at 1000 baud the
/// recognition window (392) is under one default 400-sample read, so every one-read flicker on
/// `hpx_pilot_fast`'s SL2 counts — the "rate, not category" regime, stated rather than hidden.
#[test]
fn on_hpx_pilot_fast_a_one_read_flicker_counts() {
    use openpulse_core::rate::SpeedLevel::Sl2;
    let profile = SessionProfile::hpx_pilot_fast();
    let w = recognition_window(&profile, Sl2);
    assert!(
        w < TICK,
        "PILOT-QPSK1000's window ({w}) is no longer under one read; this cell moved"
    );
    let (len, ack) = ota_verdict_on(profile, Some(Sl2), &[TICK]);
    println!("hpx_pilot_fast SL2, one read: burst {len}, ack {ack}");
    assert!(
        ack,
        "a one-read burst over the pilot window ({w}) was dropped"
    );
}
