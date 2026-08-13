//! MEASUREMENT HARNESS for #1118 — the comparison is assert-free; it prints a table.
//!
//! Replays the REAL on-air corpus in `tests/captures/` through BOTH receive paths. Every other
//! corpus test drives only `receive_with_timeout*` — the CLI path — so before this the corpus had
//! never been through `accumulate_capture`, and nobody knew whether the #1118 seam costs anything on
//! audio a radio actually produced.
//!
//! Measured (2026-08-13): **CLI 5/7, daemon 1/7.** The daemon loses every RS-coded capture the CLI
//! recovers, including the #1021 artifact and three independent-SDR recordings.
//!
//! **DO NOT read that as "the acquisition chain earns its keep".** That was the first conclusion
//! drawn here and adversarial review FALSIFIED it by ablation: with the energy gate, the settle, the
//! #1049 veto and the condemnation recovery all absent, and AFC pinned at 0, both lost bursts decode
//! as soon as the candidate loop is run over SCANNED ONSETS. The distinctive chain components
//! contributed nothing these captures needed (they sit at +2 Hz and ~+12 Hz, well inside BPSK250's
//! tolerance, so the settle was idle even on the CLI arm's wins).
//!
//! The real defect this table exposes is a BLIND SIBLING PATH, not a missing chain:
//! `decode_burst_inner` (the uncoded arm, `server.rs:930`) scans onsets in symbol-period steps across
//! up to 4 acquisition windows. `ota_decode_and_ack_inner` (the coded arm, `server.rs:866`) makes
//! ONE attempt per candidate, at offset 0, on the full burst.
//!
//! The demod's timing search spans a single symbol period (32 samples at BPSK250), so a frame a few
//! thousand samples into a burst is undecodable without a scan — and that is exactly where these
//! frames sit. The daemon's one success is its uncoded row: the coded/uncoded split tracks SCAN
//! PRESENCE, not chain presence.
//!
//! So this harness does NOT settle the #1118 design question, and in particular it does not refute
//! re-scoping: on this corpus the daemon's own adaptive-squelch DCD located the frames perfectly
//! well (verified — no burst was chopped; the failing bursts each contain the whole frame intact).
//! What WOULD settle it is a capture with a real carrier offset: these are near-on-frequency, while
//! the on-air notes record ~400 Hz inter-rig offsets, and a scan-only daemon would fail those. That
//! is the argument for porting the settle, and this corpus structurally cannot make it.
//!
//! Known limits, stated so the number is not over-read: n=7 and all from one rig pair on one band;
//! the pre-whitening rows fail on both paths by design; and `decode_burst`'s 4x-acquisition-window
//! scan bound cleared the #1021 lead-in by only 64 samples, so it is not obviously sufficient for a
//! coded-arm fix.
//!
//! Run: cargo test -p openpulse-modem --no-default-features --test daemon_vs_cli_on_real_captures -- --ignored --nocapture

use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::channel_sim::ChannelSimHarness;
use openpulse_modem::engine::ModemEngine;

const MODE: &str = "BPSK250";
/// Matches `capture_replay_corpus.rs`'s own listen budget for the coded case.
const LISTEN_MS: u64 = 40_000;
const SAMPLE_RATE: u64 = 8_000;

/// The frame captures, with what each is known to contain. Provenance: recorded 2026-07-29,
/// IC-9700 <-> FT-991A over 2 m at 5 W on 144.600 MHz (see `tests/captures/README.md`).
const FRAMES: &[(&str, FecMode, &str)] = &[
    (
        "ic9700-frame-bpsk250-none-whitened.wav",
        FecMode::None,
        "uncoded, whitened",
    ),
    (
        "ic9700-frame-bpsk250-rs-whitened.wav",
        FecMode::Rs,
        "RS-coded, whitened (the #1021 artifact)",
    ),
    (
        "ic9700-frame-bpsk250-none.wav",
        FecMode::None,
        "uncoded, pre-whitening",
    ),
    (
        "ic9700-frame-bpsk250-rs.wav",
        FecMode::Rs,
        "RS-coded, pre-whitening",
    ),
    (
        "sdr-ic9700tx-bpsk250-rs-1.wav",
        FecMode::Rs,
        "RS-coded, independent SDR receiver",
    ),
    (
        "sdr-ic9700tx-bpsk250-rs-2.wav",
        FecMode::Rs,
        "RS-coded, independent SDR receiver",
    ),
    (
        "sdr-ic9700tx-bpsk250-rs-3.wav",
        FecMode::Rs,
        "RS-coded, independent SDR receiver",
    ),
];

fn tick_samples() -> usize {
    let ms = openpulse_config::DaemonConfig::default().receive_tick_ms;
    (SAMPLE_RATE * ms / 1_000) as usize
}

fn cli_engine() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new()))
            .expect("register");
    }
    // Bound the CLI arm's search in WORK, not wall clock (#1066), as the saturating-floor corpus
    // tests do. Without this the CLI column is a measure of how much CPU time the machine had, and
    // the comparison is not portable between machines.
    h.rx_engine.set_deterministic_scan_positions(Some(3_000));
    h.rx_engine.set_deterministic_max_iterations(Some(400));
    h
}

fn daemon_engine() -> ModemEngine {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    e
}

/// CLI arm: the path every existing corpus test uses.
fn via_cli(c: &Capture, fec: FecMode) -> Option<String> {
    let mut h = cli_engine();
    h.feed_capture(c);
    h.rx_engine
        .receive_with_fec_mode_timeout(MODE, fec, None, Duration::from_millis(LISTEN_MS))
        .ok()
        .map(|b| String::from_utf8_lossy(&b).to_string())
}

/// Daemon arm: `accumulate_capture` in tick-sized chunks, then the DEFAULT decode arm
/// (`decode_burst`, which `server.rs:930` calls when `ota_enabled` is off).
///
/// Returns (payload, bursts_flushed, settle_attempts) so a "no decode" can be told apart from
/// "nothing was ever gathered to decode".
fn via_daemon(c: &Capture, fec: FecMode) -> (Option<String>, usize, u64) {
    let mut e = daemon_engine();
    // `decode_burst` is FecMode::None-only, so a coded capture must go through the OTA arm, whose
    // candidates come from the profile. Without this the coded rows would measure "the default arm
    // cannot do RS", which is true but is NOT a fact about the acquisition chain.
    if fec != FecMode::None {
        let profile = SessionProfile::hpx_hf();
        // Lock the OTA level to the rung whose mode IS the captured one. `rx_candidates` offers only
        // the recommended + confirmed levels, which on a fresh session are both the ENTRY rung
        // (BPSK31) — so without this the daemon arm never tries BPSK250+Rs at all and a "no decode"
        // would be about candidate coverage, not about the acquisition chain. The rung is SEARCHED,
        // not transcribed, so a profile change cannot silently invalidate this.
        let level = (1u8..=20)
            .filter_map(openpulse_core::rate::SpeedLevel::from_u8)
            .find(|&l| profile.mode_for(l) == Some(MODE))
            .unwrap_or_else(|| {
                panic!("hpx_hf has no rung running {MODE}; the comparison cannot be set up")
            });
        e.start_ota_session(profile);
        e.ota_lock_level(level);
    }
    let tick = tick_samples();
    let quiet = vec![0.0f32; tick];
    let mut bursts = Vec::new();
    for chunk in c.samples.chunks(tick) {
        if let Ok(Some(b)) = e.accumulate_capture(Some(MODE), chunk.to_vec()) {
            bursts.push(b);
        }
    }
    for _ in 0..8 {
        if let Ok(Some(b)) = e.accumulate_capture(Some(MODE), quiet.clone()) {
            bursts.push(b);
        }
    }
    let n = bursts.len();
    // Try every flushed burst: the daemon would too, tick after tick.
    let mut got = None;
    for b in &bursts {
        let r = if fec == FecMode::None {
            e.decode_burst(MODE, b)
        } else {
            e.ota_decode_burst(b, "measure", Some(MODE))
                .map(|o| o.payload.unwrap_or_default())
        };
        if let Ok(bytes) = r {
            if !bytes.is_empty() {
                got = Some(String::from_utf8_lossy(&bytes).to_string());
                break;
            }
        }
    }
    (got, n, e.afc_settle_attempts())
}

#[test]
#[ignore = "measurement"]
fn m1_daemon_vs_cli_on_the_real_on_air_corpus() {
    println!("\n=== #1118 measurement: real on-air captures, CLI path vs daemon path ===");
    println!(
        "tick = {} samples ({} ms)\n",
        tick_samples(),
        openpulse_config::DaemonConfig::default().receive_tick_ms
    );
    println!(
        "{:<44} {:<8} {:>7}  {:<22} {:<22}",
        "capture", "mean_sq", "bursts", "CLI", "DAEMON"
    );

    let mut cli_ok = 0;
    let mut daemon_ok = 0;
    for (file, fec, what) in FRAMES {
        let c = match load_corpus(file) {
            Ok(c) => c,
            Err(e) => {
                println!("{file:<44} SKIP (not loadable: {e})");
                continue;
            }
        };
        assert!(
            c.mean_sq() > 1e-6,
            "{file} has gone silent (mean_sq {:.8}) — the comparison would be meaningless",
            c.mean_sq()
        );

        let cli = via_cli(&c, *fec);
        let (dae, bursts, settles) = via_daemon(&c, *fec);

        assert_eq!(
            settles, 0,
            "{file}: the daemon arm entered the acquisition chain ({settles} settles) — #1118 says \
             it cannot, so this harness is not measuring what it claims"
        );

        if cli.is_some() {
            cli_ok += 1;
        }
        if dae.is_some() {
            daemon_ok += 1;
        }
        let fmt = |o: &Option<String>| match o {
            Some(s) => format!("OK {:.18}", s.replace('\n', " ")),
            None => "-- no decode".to_string(),
        };
        println!(
            "{file:<44} {:<8.5} {bursts:>7}  {:<22} {:<22}  [{what}, fec={fec:?}]",
            c.mean_sq(),
            fmt(&cli),
            fmt(&dae)
        );
    }

    println!(
        "\nCLI decoded {cli_ok}/{} | daemon decoded {daemon_ok}/{}",
        FRAMES.len(),
        FRAMES.len()
    );
    println!(
        "\nReading: a daemon loss is NOT evidence for the acquisition chain — ablation showed these\n\
           \x20        bursts decode with the chain absent, once the candidate loop scans onsets.\n\
           \x20        bursts == 0 -> the daemon never gathered anything; the row says nothing.\n"
    );
}
