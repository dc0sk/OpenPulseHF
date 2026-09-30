//! The spectral busy test gathers a frame the total-power carrier detect cannot see (#1454, REQ-DCD-01).
//!
//! A BPSK31 frame at +8 dB in its own 62 Hz band lifts a wide filter's total power by under 2 dB, so
//! the total-power squelch never opens on it (stage 1 pinned 0/4 even at +12 dB). The spectral test
//! judges each 62.5 Hz band against its own floor. These gates drive the production entry
//! (`accumulate_capture`, 400-sample reads) over the recorded IC-9700 idles.
//!
//! Two things decide whether a weak slow frame is gathered whole, and both are pinned here:
//!
//! - **Where the frame's symbols fall on the 512-sample analysis grid.** A BPSK31 reversal nulls its
//!   envelope at the symbol centre; with a centre under the Hann peak a window reads 0.38 of the band
//!   power, and the alternating preamble does that in every window at one parity. The placements below
//!   therefore cover both halves of the grid, and the forced cells pin both preamble parities against
//!   the best case (#1454 rounds 6–7).
//! - **The M2 ring.** A total-power flicker at the frame's onset must not take the frame's head.
//!
//! The counting cells run by default; decode counts are in the held-out suite
//! (`scripts/slow-tests.sh spectral`), because 16 BPSK31 decodes per cell cost minutes unoptimised.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_dsp::noise_floor::NoiseFloorTracker;
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::ModemEngine;
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

const TICK: usize = 400;
const RATE: f32 = 8000.0;
const WARM: usize = 3 * TICK * 20;
const PLACEMENTS: usize = 16;
const WIDE: &str = "ic9700-idle-wide-500hz-control.wav";
const NARROW_500: &str = "ic9700-idle-500hz.wav";
const NARROW_250: &str = "ic9700-idle-250hz.wav";

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

fn preamble_of(mode: &str) -> usize {
    use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
    BpskPlugin::new()
        .frame_geometry(&ModulationConfig {
            mode: mode.into(),
            ..ModulationConfig::default()
        })
        .expect("the mode publishes its frame geometry")
        .preamble_samples
}

fn payload(n: usize) -> Vec<u8> {
    (0..n as u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect()
}

/// The frame start's phase against the 512-sample analysis grid, `(−f0) mod 512`. Values 128 and 384
/// put a BPSK31 symbol centre under the Hann peak (one preamble parity each); 0 is the best case.
fn lead_for(t: usize, align: Option<usize>) -> usize {
    let lead = 4000 + t * 17_137;
    match align {
        None => lead,
        Some(a) => {
            let off = (512 - (WARM + lead) % 512) % 512;
            lead + (off + 512 - a) % 512
        }
    }
}

/// What one placement produced over the frame.
struct Gathered {
    /// Bursts over the frame that could BE a frame (post-trigger length at least the preamble). A
    /// shorter one is a flicker, committed as band by the engine's own rule.
    frames: usize,
    /// Flicker bursts over the frame.
    flickers: usize,
    /// The first non-flicker burst starts at or before the frame (its ring covered the head).
    head: bool,
    /// Some non-flicker burst decoded to the payload (only when asked).
    decoded: bool,
}

/// Superimpose `tx` on `idle` at `in_band_db` (noise measured inside `mode`'s band), `lead` samples
/// after the warm-up, and feed it through `accumulate_capture` in `TICK` reads.
fn gather(
    idle: &Capture,
    t: usize,
    mode: &str,
    tx: &[f32],
    in_band_db: f32,
    lead: usize,
    decode: Option<&[u8]>,
) -> Gathered {
    let total = WARM + lead + tx.len() + 5 * 8000;
    let mut buf = idle.cycled(t * 9000, total);
    let noise = rms(&buf[..WARM]).powi(2)
        * in_band_share(
            &buf[..WARM],
            1500.0 - half_band(mode),
            1500.0 + half_band(mode),
        );
    let fms = tx.iter().map(|v| v * v).sum::<f32>() / tx.len() as f32;
    let g = (noise * 10f32.powf(in_band_db / 10.0) / fms).sqrt();
    let f0 = WARM + lead;
    for (i, s) in tx.iter().enumerate() {
        buf[f0 + i] += s * g;
    }
    let shortest = preamble_of(mode);
    let (mut e, _lb) = engine();
    let mut out = Gathered {
        frames: 0,
        flickers: 0,
        head: false,
        decoded: false,
    };
    let mut fed = 0usize;
    for chunk in buf.chunks(TICK) {
        fed += chunk.len();
        let Ok(Some(b)) = e.accumulate_capture(Some(mode), chunk.to_vec()) else {
            continue;
        };
        let end = fed - chunk.len();
        let start = end - b.samples.len();
        if start >= f0 + tx.len() || end <= f0 {
            continue;
        }
        let post = b.samples.len() - e.last_flush_lead();
        if post < shortest {
            out.flickers += 1;
            continue;
        }
        if out.frames == 0 {
            out.head = start <= f0;
        }
        out.frames += 1;
        if let Some(p) = decode {
            out.decoded |=
                matches!(e.decode_burst_with_fec(mode, FecMode::Rs, &b), Ok(q) if q == p);
        }
    }
    out
}

/// A cell: `PLACEMENTS` placements of one fixture. Returns (whole, heads, decoded, flickers).
fn cell(
    idle: &Capture,
    mode: &str,
    tx: &[f32],
    in_band_db: f32,
    align: Option<usize>,
    decode: Option<&[u8]>,
) -> (usize, usize, usize, usize) {
    let (mut whole, mut heads, mut decoded, mut flickers) = (0, 0, 0, 0);
    for t in 0..PLACEMENTS {
        let g = gather(idle, t, mode, tx, in_band_db, lead_for(t, align), decode);
        whole += (g.frames == 1) as usize;
        heads += (g.frames == 1 && g.head) as usize;
        decoded += g.decoded as usize;
        flickers += g.flickers;
    }
    (whole, heads, decoded, flickers)
}

/// P2: a BPSK31 frame at +8 dB behind a wide filter, over both halves of the alignment grid, is
/// gathered as ONE burst whose head is covered. Bar: 15 of 16 (round 7). Before #1454 round 7 the
/// 2-of-4 hold split 5 of these (11/16).
#[test]
fn a_bpsk31_frame_behind_a_wide_filter_is_gathered_whole() {
    let tx = frame(&payload(64), "BPSK31");
    let (whole, heads, _, flickers) = cell(&corpus(WIDE), "BPSK31", &tx, 8.0, None, None);
    eprintln!("P2: whole {whole}/16, head covered {heads}/16, flickers {flickers}");
    assert!(
        heads >= 15,
        "gathered whole with its head in {heads}/16 placements"
    );
}

/// Both preamble parities at the worst alignment are gathered whole, paired with the best alignment on
/// the same noise. With one analysis phase, the 128 parity reads its whole preamble at 0.38 of the
/// band power: the open waits for data and the head falls outside the ring (14/16, round 7). The
/// straddling phase sees the other parity. C0 is the control that the cells are otherwise equal.
#[test]
fn both_preamble_parities_open_in_time() {
    let tx = frame(&payload(64), "BPSK31");
    let idle = corpus(WIDE);
    // All three cells, THEN the verdict: a sabotage log must carry the control cells too.
    let got: Vec<(usize, usize, usize)> = [128, 384, 0]
        .into_iter()
        .map(|align| {
            let (whole, heads, _, _) = cell(&idle, "BPSK31", &tx, 8.0, Some(align), None);
            eprintln!("align {align}: whole {whole}/16, head covered {heads}/16");
            (align, whole, heads)
        })
        .collect();
    for (align, _, heads) in got {
        assert!(
            heads >= 15,
            "alignment {align}: gathered whole with its head in {heads}/16"
        );
    }
}

/// A steady tone at the same in-band level is gathered as one burst in every placement: the noise-only
/// control. A hold rule that splits it is splitting on noise, not on the frame's content.
#[test]
fn a_steady_tone_is_never_split() {
    let n = frame(&payload(64), "BPSK31").len();
    let tone: Vec<f32> = (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / RATE).sin())
        .collect();
    let (whole, _, _, _) = cell(&corpus(WIDE), "BPSK31", &tone, 8.0, None, None);
    assert_eq!(whole, 16, "the tone was split in {} placements", 16 - whole);
}

/// BPSK63 at +8 dB behind a wide and a 500 Hz filter is gathered as one burst in every placement.
/// (Some of these bursts are opened by TOTAL POWER after the frame starts and carry no ring; their
/// decodes are #1443's, reported by the held-out suite, not gated here.)
#[test]
fn bpsk63_frames_are_gathered_as_one_burst() {
    let tx = frame(&payload(64), "BPSK63");
    for cap in [WIDE, NARROW_500] {
        let (whole, _, _, flickers) = cell(&corpus(cap), "BPSK63", &tx, 8.0, None, None);
        eprintln!("BPSK63 {cap}: whole {whole}/16, flickers {flickers}");
        assert_eq!(whole, 16, "{cap}: one burst in {whole}/16");
    }
}

/// On every recorded idle, at the daemon's read sizes, the spectral test never opens, and its hold —
/// were an open to arm it — would not outlast 8 windows (printed below; measured 0 / 1 / 0 with the
/// capped hold). 8 keeps 8 windows below the 16 at which a failed S burst becomes ladder evidence.
#[test]
fn idle_never_opens_the_spectral_test_and_its_hold_is_short() {
    for cap in [WIDE, NARROW_500, NARROW_250] {
        let x = corpus(cap).samples;
        for read in [171, 400, 512, 4096] {
            let mut t = NoiseFloorTracker::new();
            let opens = x
                .chunks(read)
                .filter(|b| {
                    let v = t.judge(b);
                    t.learn_pending();
                    v.open
                })
                .count();
            assert_eq!(
                opens, 0,
                "{cap} at {read}-sample reads opened S {opens} times"
            );
        }
        let mut t = NoiseFloorTracker::new();
        let (mut run, mut longest) = (0usize, 0usize);
        for w in x.chunks(512) {
            let v = t.judge(w);
            t.learn_pending();
            if v.windows > 0 {
                run = if v.hold { run + 1 } else { 0 };
                longest = longest.max(run);
            }
        }
        eprintln!("{cap}: longest idle hold run {longest} windows");
        assert!(longest <= 8, "{cap}: idle hold run of {longest} windows");
    }
}

/// M2: a frame at +10 dB — close enough to the total-power squelch that one onset block can clear it
/// and close again before the spectral test opens — keeps its head. That flicker used to drain the
/// ring, so the S burst that followed started after the frame (round 6: two flicker placements at
/// +10 dB, at least one losing its head). The cell must actually contain flickers, or it proves
/// nothing. It also pins "opened by S" as S true at the open block rather than "total power false":
/// under that proxy the same cell loses the same heads — a dependence on this fixture containing
/// such placements, which nothing else asserts.
#[test]
fn an_onset_flicker_does_not_take_the_frame_s_head() {
    let tx = frame(&payload(64), "BPSK31");
    let (whole, heads, _, flickers) = cell(&corpus(WIDE), "BPSK31", &tx, 10.0, None, None);
    eprintln!("+10 dB: whole {whole}/16, head covered {heads}/16, flickers {flickers}");
    assert!(
        flickers > 0,
        "no onset flicker occurred: the cell is vacuous"
    );
    assert!(heads >= 15, "head covered in {heads}/16");
}

/// M2 retention: a frame that was delivered leaves the ring. A short uncoded BPSK250 frame is followed,
/// 12 windows after it ends, by a weak BPSK31 frame that only the spectral test opens on. The BPSK31
/// burst's 16-window ring must not reach back past the BPSK250 burst's end — otherwise that frame is
/// handed over twice and the weak frame's burst starts with someone else's. The first frame is longer
/// than BPSK31's preamble, so it is a possible frame, not a flicker (a flicker keeps the ring by
/// design); the gap is longer than the hold's tail, so the two are separate bursts.
#[test]
fn a_delivered_frame_is_not_prepended_to_the_next_burst() {
    let idle = corpus(WIDE);
    let id = {
        let (mut e, lb) = engine();
        e.transmit_with_fec_mode(ID, "BPSK250", FecMode::None, None)
            .expect("tx");
        lb.drain_samples()
    };
    let weak = frame(&payload(64), "BPSK31");
    assert!(
        id.len() > preamble_of("BPSK31"),
        "the first frame must not be a flicker"
    );
    let gap = 12 * 512;
    let (id_at, weak_at) = (WARM + 20_000, WARM + 20_000 + id.len() + gap);
    let mut buf = idle.cycled(0, weak_at + weak.len() + 5 * 8000);
    let level = |x: &[f32], mode: &str, db: f32, buf: &[f32]| {
        let noise = rms(&buf[..WARM]).powi(2)
            * in_band_share(
                &buf[..WARM],
                1500.0 - half_band(mode),
                1500.0 + half_band(mode),
            );
        let fms = x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
        (noise * 10f32.powf(db / 10.0) / fms).sqrt()
    };
    let (g_id, g_weak) = (
        level(&id, "BPSK250", 12.0, &buf),
        level(&weak, "BPSK31", 8.0, &buf),
    );
    for (i, s) in id.iter().enumerate() {
        buf[id_at + i] += s * g_id;
    }
    for (i, s) in weak.iter().enumerate() {
        buf[weak_at + i] += s * g_weak;
    }
    let (mut e, _lb) = engine();
    let (mut fed, mut id_end, mut ids, mut weak_lead) = (0usize, None, 0usize, None);
    for chunk in buf.chunks(TICK) {
        fed += chunk.len();
        let Ok(Some(b)) = e.accumulate_capture(Some("BPSK31"), chunk.to_vec()) else {
            continue;
        };
        let end = fed - chunk.len();
        let start = end - b.samples.len();
        let lead = e.last_flush_lead();
        if start < id_at + id.len() && end > id_at {
            ids += matches!(e.decode_burst_with_fec("BPSK250", FecMode::None, &b), Ok(q) if q == ID)
                as usize;
            id_end = Some(end);
        } else if start < weak_at + weak.len() && end > weak_at + weak.len() / 2 {
            weak_lead = Some((start, lead));
        }
    }
    assert_eq!(ids, 1, "the ID must be gathered and decode exactly once");
    let id_end = id_end.expect("the ID burst flushed");
    let (start, lead) = weak_lead.expect("the weak frame was gathered");
    assert!(
        lead > 0,
        "the weak frame's burst carries no ring: the fixture is not S-opened"
    );
    assert!(
        start >= id_end,
        "the weak frame's burst starts {} samples before the ID burst ended (lead {lead})",
        id_end - start
    );
}

/// The spectral half of the ladder-evidence rule, both sides (#1454 round 8). A failed burst the
/// spectral test carried counts when S was still OPEN at least `S_RING_WINDOWS` windows after the
/// trigger — measured to the last open, not to the flush, because the hold keeps an armed burst up to
/// eight idle windows past its occupant. A narrowband tone (S opens, total power never does, nothing
/// decodes) of 3 s must key a NACK; a shorter one must not, although its burst with the hold's tail
/// runs past the threshold — the case the flush-measured rule got wrong. +8 dB over its band: at +9 dB
/// total power already opens the tone. The short tone is sized to the hold's TAIL (tone + tail ≥ SL2's
/// 8 448-sample window, tone + the open's lag < it); any change to the hold re-rolls it, and a failure
/// of its vacuity line is then a fixture to re-size, not an evidence-rule regression.
#[test]
fn a_failed_spectral_burst_is_ladder_evidence_only_past_sixteen_windows_of_open() {
    use openpulse_core::profile::SessionProfile;
    use openpulse_core::rate::SpeedLevel::Sl2;
    let idle = corpus(WIDE);
    let verdict = |tone_len: usize| {
        let (mut e, _lb) = engine();
        e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
            .expect("register");
        e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
            .expect("register");
        e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
            .expect("register");
        e.register_plugin(Box::new(pilot_plugin::PilotPlugin::new()))
            .expect("register");
        e.start_ota_session(SessionProfile::hpx_hf());
        e.ota_lock_level(Sl2);
        let at = WARM + 4_000;
        let mut buf = idle.cycled(0, at + tone_len + 5 * 8000);
        let noise = rms(&buf[..WARM]).powi(2)
            * in_band_share(
                &buf[..WARM],
                1500.0 - half_band("BPSK31"),
                1500.0 + half_band("BPSK31"),
            );
        let g = (noise * 10f32.powf(8.0 / 10.0) * 2.0).sqrt();
        for i in 0..tone_len {
            buf[at + i] += g * (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / RATE).sin();
        }
        let mut burst = None;
        for chunk in buf.chunks(TICK) {
            if let Ok(Some(b)) = e.accumulate_capture(Some("BPSK31"), chunk.to_vec()) {
                burst.get_or_insert((b, e.last_flush_lead()));
            }
        }
        let (b, lead) = burst.expect("the tone must be gathered");
        assert!(
            lead > 0,
            "the tone's burst carries no ring: it was not opened by the spectral test"
        );
        let post = b.samples.len() - lead;
        let r = e
            .ota_decode_burst(&b, "tone", Some("BPSK31"))
            .expect("decode");
        (post, r.ack.is_some())
    };
    let (post, ack) = verdict(3 * 8000);
    assert!(
        ack,
        "a 3 s spectral burst ({post} samples after its trigger) keyed no NACK"
    );
    // The control must outrun the evidence threshold with its hold tail included — 16 windows, and
    // SL2's recognition window (BPSK31's preamble plus one symbol) — or it cannot tell the rules apart.
    let (post, ack) = verdict(8_000);
    assert!(
        post >= (16 * 512).max(preamble_of("BPSK31") + 256),
        "the short tone's burst ({post}) no longer outruns 16 windows: the control is vacuous"
    );
    assert!(
        !ack,
        "an 8 000-sample spectral burst counted as ladder evidence (burst {post})"
    );
}

const ID: &[u8] = b"DE N0CALL DE N0CALL DE N0CALL DE N0CALL DE N0CALL";

/// Decode counts for every cell — the held-out half of the gates above (minutes unoptimised).
#[test]
#[ignore = "held out for runtime: run by scripts/slow-tests.sh spectral"]
fn weak_frames_decode_through_the_daemon_path() {
    let p = payload(64);
    let b31 = frame(&p, "BPSK31");
    let b63 = frame(&p, "BPSK63");
    let (wide, n500) = (corpus(WIDE), corpus(NARROW_500));
    let mut failures = Vec::new();
    for (name, idle, mode, tx, db, align, bar) in [
        ("A +8", &wide, "BPSK31", &b31, 8.0, None, 15),
        ("B128 +8", &wide, "BPSK31", &b31, 8.0, Some(128), 15),
        ("B384 +8", &wide, "BPSK31", &b31, 8.0, Some(384), 15),
        ("C0 +8", &wide, "BPSK31", &b31, 8.0, Some(0), 15),
        ("I +10", &wide, "BPSK31", &b31, 10.0, None, 15),
        ("E +7 (report)", &wide, "BPSK31", &b31, 7.0, None, 0),
        ("F +6 (report)", &wide, "BPSK31", &b31, 6.0, None, 0),
        ("BPSK63 wide (#1443)", &wide, "BPSK63", &b63, 8.0, None, 13),
        (
            "BPSK63 500 Hz (#1443)",
            &n500,
            "BPSK63",
            &b63,
            8.0,
            None,
            13,
        ),
    ] {
        let (whole, heads, decoded, flickers) = cell(idle, mode, tx, db, align, Some(&p));
        eprintln!(
            "{name}: decoded {decoded}/16, whole {whole}/16, head {heads}/16, flickers {flickers}"
        );
        if decoded < bar {
            failures.push(format!("{name}: decoded {decoded}/16 < {bar}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
