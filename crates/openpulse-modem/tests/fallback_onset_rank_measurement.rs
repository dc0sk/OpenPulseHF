//! Rank of a frame's true onset among the #1123 fallback's correlation-ranked onsets
//! (`docs/dev/design/fallback-onset-ranking.md`). This is what sets the engine's `FALLBACK_RANKED_ONSETS` (K).
//!
//! An uncoded BPSK250 frame is placed behind a lead cut from a REAL idle recording (the ring a
//! flushed burst opens with), at a signal-to-idle ratio swept down to where the frame stops
//! decoding, on a flat channel and on Watterson `moderate_f1`. For each frame that decodes when
//! handed its own onset, the rank is the position of the first ranked onset within ±16 samples of
//! the truth; "miss" is none in the top `RANKS_SHOWN`. Frames that do not decode are counted but
//! not ranked: a ranking miss on a frame the receiver cannot decode costs nothing.
//!
//! The 250 Hz-filter idle is the hardest ring in the corpus for this: its own BPSK250 ρ peaks at
//! 0.579 over 45 s (`tests/captures/README.md`), so near the floor noise really competes.
//!
//! `measure_fallback_onset_ranks` prints the histogram and is `#[ignore]`d (release build, minutes):
//! `cargo test --release -p openpulse-modem --no-default-features --test fallback_onset_rank_measurement -- --ignored --nocapture`.
//! The default-run tests pin the production path: a control frame in a real ring decodes at rank 0
//! through `accumulate_capture`, and a frame deep in a long real lead ranks first.
//!
//! What this cannot measure: a real on-air frame. The corpus's frame captures predate the current
//! wire format and preamble (#1148, #1062) and decode against nothing, so the on-air half of the
//! K measurement waits for the re-record; the miss counter is how it is checked on air meanwhile.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::profile::SessionProfile;
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;

const MODE: &str = "BPSK250";
/// The onset range at a 4 096-sample read: the ring (up to 8 192) plus the trigger read. The
/// measurement also runs at four times this, the range after a slow decode lengthens the read.
const ONSET_BOUND: usize = 12_288;
const TOLERANCE: usize = 16;
const RANKS_SHOWN: usize = 8;

/// The shipped K, read from the rank counters (one column per rank, plus the miss column).
fn shipped_k() -> usize {
    engine().fallback_onset_ranks().len() - 1
}

fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    e
}

fn frame(payload: &[u8]) -> Vec<f32> {
    let bk = LoopbackBackend::new();
    let mut tx = ModemEngine::new(Box::new(bk.clone_shared()));
    tx.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    tx.transmit(payload, MODE, None).expect("transmit");
    bk.drain_samples()
}

fn mean_sq(x: &[f32]) -> f32 {
    x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32
}

/// Continuous `idle` audio with `signal` added on top from sample `lead`, scaled to `snr_db` above
/// the idle's mean square. The idle runs under the frame too, as a receiver hears it.
fn embed(idle: &Capture, from: usize, lead: usize, signal: &[f32], snr_db: f32) -> Vec<f32> {
    let gain = (mean_sq(&idle.samples) * 10f32.powf(snr_db / 10.0) / mean_sq(signal)).sqrt();
    let mut buf = idle.cycled(from, lead + signal.len() + 8_000);
    for (b, &s) in buf[lead..].iter_mut().zip(signal) {
        *b += s * gain;
    }
    buf
}

fn rank_of(onsets: &[usize], truth: usize) -> Option<usize> {
    onsets.iter().position(|&o| o.abs_diff(truth) <= TOLERANCE)
}

/// Deterministic xorshift, so a run is reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[test]
#[ignore = "measurement: sets FALLBACK_RANKED_ONSETS; release build, prints a histogram"]
fn measure_fallback_onset_ranks() {
    let idles = [
        "ic9700-idle-250hz.wav",
        "ic9700-idle-500hz.wav",
        "ic9700-idle-hot.wav",
    ];
    let trials: usize = std::env::var("RANK_TRIALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24);
    // The worst rank any decodable frame reached; RANKS_SHOWN means beyond the shown ranks.
    let mut worst = 0usize;
    for bound in [ONSET_BOUND, 4 * ONSET_BOUND] {
        for idle_name in idles {
            let idle = load_corpus(idle_name).expect("idle capture");
            for fading in [false, true] {
                for snr_db in [12.0f32, 9.0, 6.0, 3.0, 0.0, -3.0, -6.0] {
                    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ snr_db.to_bits() as u64);
                    let mut hist = [0usize; RANKS_SHOWN + 1];
                    let mut undecodable = 0usize;
                    for t in 0..trials {
                        let len = 8 + rng.below(200) as usize;
                        let payload: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
                        let mut signal = frame(&payload);
                        if fading {
                            let mut cfg = WattersonConfig::moderate_f1(Some(t as u64 + 1));
                            cfg.snr_db = 60.0;
                            signal = WattersonChannel::new(cfg).unwrap().apply(&signal);
                        }
                        let lead = rng.below(bound as u64 - 1_024) as usize;
                        let from = rng.below(idle.samples.len() as u64) as usize;
                        let burst = embed(&idle, from, lead, &signal, snr_db);

                        // Decodable at all, when handed its own onset?
                        let own = AudioSamples {
                            samples: burst[lead..].to_vec(),
                        };
                        if engine().decode_burst(MODE, &own).ok().as_deref() != Some(&payload[..]) {
                            undecodable += 1;
                            continue;
                        }
                        let onsets = engine()
                            .fallback_onset_ranking(MODE, &burst, bound, RANKS_SHOWN)
                            .expect("BPSK250 publishes a template");
                        let r = rank_of(&onsets, lead).unwrap_or(RANKS_SHOWN);
                        hist[r] += 1;
                        worst = worst.max(r);
                    }
                    println!(
                    "bound {bound:>6} {idle_name:<24} {:<9} {snr_db:>5.1} dB | undecodable {undecodable:>3} | rank 0..{} {:?} | miss {}",
                    if fading { "moderate" } else { "flat" },
                    RANKS_SHOWN - 1,
                    &hist[..RANKS_SHOWN],
                    hist[RANKS_SHOWN],
                );
                }
            }
        }
    }
    println!(
        "worst rank of a decodable frame: {worst} (K = {}; {RANKS_SHOWN} = not in the top {RANKS_SHOWN})",
        shipped_k()
    );
}

/// An uncoded control frame inside a real idle recording decodes through the production entry
/// (`accumulate_capture` → `ota_decode_burst`) at the rank-0 onset, not by the exhaustive scan.
///
/// Synthetic frame, real ring: the corpus's own frame captures predate the #1148 keystream and the
/// #1062 preamble and decode against nothing current (`tests/captures/README.md`, wire-format epoch).
#[test]
fn a_control_frame_in_a_real_ring_decodes_at_rank_zero() {
    let idle = load_corpus("ic9700-idle-250hz.wav").expect("idle capture");
    let payload = b"STATION ID DC0SK";
    let audio = embed(&idle, 0, 16_000, &frame(payload), 9.0);
    let mut rx = engine();
    for p in [
        Box::new(qpsk_plugin::QpskPlugin::new())
            as Box<dyn openpulse_core::plugin::ModulationPlugin>,
        Box::new(ofdm_plugin::OfdmPlugin::new()),
        Box::new(fsk4_plugin::Fsk4Plugin::new()),
        Box::new(mfsk16_plugin::Mfsk16Plugin::new()),
    ] {
        rx.register_plugin(p).unwrap();
    }
    rx.start_ota_session(SessionProfile::fast());
    // As the daemon does: decode each burst the moment it flushes, then flush the tail with idle.
    let reads = audio.chunks(4_096).map(<[f32]>::to_vec);
    let tail = (0..8).map(|i| idle.cycled(40_000 + i * 4_096, 4_096));
    let mut decoded = None;
    for read in reads.chain(tail) {
        let Some(burst) = rx.accumulate_capture(Some(MODE), read).unwrap() else {
            continue;
        };
        let before = rx.fallback_onset_ranks();
        let r = rx.ota_decode_burst(&burst, "rank", Some(MODE)).unwrap();
        if r.payload.is_some() {
            decoded = Some((r, before, rx.fallback_onset_ranks()));
            break;
        }
    }
    let (r, before, after) = decoded.expect("the frame decodes through the production entry");
    assert_eq!(r.payload.as_deref(), Some(&payload[..]));
    assert!(r.ack.is_none(), "an uncoded frame is not ladder traffic");
    let moved: Vec<usize> = (0..=shipped_k())
        .filter(|&i| after[i] != before[i])
        .collect();
    assert_eq!(
        moved,
        vec![0],
        "decoded at rank 0, not later and not by the exhaustive scan: {before:?} -> {after:?}"
    );
}

/// A frame well above a real idle floor, deep in a long lead, ranks first.
#[test]
fn a_frame_behind_a_long_real_lead_ranks_first() {
    let idle = load_corpus("ic9700-idle-250hz.wav").expect("idle capture");
    let signal = frame(b"rank me first");
    for lead in [0usize, 2_731, 9_000] {
        let burst = embed(&idle, 1_000, lead, &signal, 9.0);
        let onsets = engine()
            .fallback_onset_ranking(MODE, &burst, ONSET_BOUND, shipped_k())
            .expect("template");
        assert_eq!(rank_of(&onsets, lead), Some(0), "lead {lead}: {onsets:?}");
    }
}

/// Two control frames in one keying (#1461), the second louder so it ranks first: the first is
/// still delivered first and the second rides along in `more`. Ranked onsets are attempted in time
/// order; attempting them in ρ order decoded the second frame and lost the first.
#[test]
fn the_first_of_two_frames_in_one_keying_is_not_lost() {
    let idle = load_corpus("ic9700-idle-500hz.wav").expect("idle capture");
    let a = frame(b"FRAG A");
    let b = frame(b"FRAG B");
    let gap = 400usize;
    let mut keying = a.clone();
    keying.extend(std::iter::repeat_n(0.0, gap));
    keying.extend(b.iter().map(|&s| s * 2.0));
    // 15 dB over the keying's mean puts A near 11 dB, above the cliff, and B 6 dB above A.
    let audio = embed(&idle, 0, 16_000, &keying, 15.0);

    let mut rx = engine();
    rx.start_ota_session(SessionProfile::fast());
    // One read spans both frames, as the daemon's does after a slow decode, so both onsets fall
    // inside the scan range.
    let reads = audio.chunks(32_768).map(<[f32]>::to_vec);
    let tail = (0..8).map(|i| idle.cycled(40_000 + i * 32_768, 32_768));
    let mut got = None;
    for read in reads.chain(tail) {
        let Some(burst) = rx.accumulate_capture(Some(MODE), read).unwrap() else {
            continue;
        };
        let r = rx.ota_decode_burst(&burst, "rank", Some(MODE)).unwrap();
        if r.payload.is_some() {
            got = Some(r);
            break;
        }
    }
    let r = got.expect("the keying decodes");
    assert_eq!(
        r.payload.as_deref(),
        Some(&b"FRAG A"[..]),
        "first frame first"
    );
    assert_eq!(
        r.more,
        vec![b"FRAG B".to_vec()],
        "second frame handed out too"
    );
}
