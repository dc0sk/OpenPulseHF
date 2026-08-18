//! MEASUREMENT for #1118 — what does the phase-2 acquisition pass cost on a busy band?
//!
//! The two-phase design's cost argument is that phase 2 runs only on bursts that are already lost.
//! Adversarial review demolished the *rarity* half of that claim: on a real band every burst that
//! crosses the DCD and is not our own on-frequency traffic fails phase 1 by definition, so phase 2
//! is effectively always-on for everything except what already works. The design ships on the
//! no-regression property instead — and that makes the worst case a number somebody has to measure
//! rather than argue.
//!
//! **The quantity that matters is not seconds, it is the RATIO of CPU time to audio time.** The
//! daemon's rx tick decodes inline on flush, so time spent in the decoder is time not spent reading
//! the capture stream. A ratio at or above 1.0 means the receiver cannot keep up with its own audio
//! and goes deaf while it thinks — which is a different failure from "slow", and the one that loses
//! the *next* frame.
//!
//! Two rows, both from real runs rather than from a compile-time switch:
//!
//! * **busy band** — recorded IC-9700 hot idle, which sits above the adaptive DCD squelch and so
//!   flushes bursts continuously. Nothing decodes, so this is phase 1 + phase 2, the worst case.
//! * **on-frequency frame** — the same audio with a real frame in it. Phase 1 decodes, so phase 2
//!   never runs; this row is the phase-1-only cost, measured the same way.
//!
//! **Run in RELEASE.** #1066 records this host's debug build at ~5x slower, so a debug ratio is a
//! pessimistic bound rather than a prediction — and a machine under load measures the machine
//! (which is why the header prints both the profile and a caveat).
//!
//! Run: cargo test --release -p openpulse-modem --no-default-features --test phase2_cost -- --ignored --nocapture

use std::time::Instant;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::channel_sim::ChannelSimHarness;
use openpulse_modem::engine::ModemEngine;

const MODE: &str = "BPSK250";
const SAMPLE_RATE: usize = 8_000;
/// Enough audio for several DCD-flushed bursts.
const BUSY_SECONDS: usize = 30;

fn tick_samples() -> usize {
    SAMPLE_RATE * openpulse_config::DaemonConfig::default().receive_tick_ms as usize / 1_000
}

fn engine() -> ModemEngine {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    let profile = SessionProfile::hpx_hf();
    let level = (1u8..=20)
        .filter_map(SpeedLevel::from_u8)
        .find(|&l| profile.mode_for(l) == Some(MODE))
        .expect("hpx_hf must have a BPSK250 rung");
    e.start_ota_session(profile);
    e.ota_lock_level(level);
    e
}

/// Feed `samples` through the daemon's own entry points, timing only the DECODE work.
///
/// Accumulation is excluded deliberately: it runs whether or not phase 2 exists, so including it
/// would dilute the very ratio under test.
fn cost_of(samples: &[f32]) -> (f64, f64, usize, u64) {
    let mut e = engine();
    let tick = tick_samples();
    let mut bursts = Vec::new();
    for chunk in samples.chunks(tick) {
        if let Ok(Some(b)) = e.accumulate_capture(Some(MODE), chunk.to_vec()) {
            bursts.push(b);
        }
    }
    for _ in 0..8 {
        if let Ok(Some(b)) = e.accumulate_capture(Some(MODE), vec![0.0; tick]) {
            bursts.push(b);
        }
    }
    let burst_samples: usize = bursts.iter().map(|b| b.samples.len()).sum();
    let t0 = Instant::now();
    for b in &bursts {
        let _ = e.ota_decode_burst(b, "cost", Some(MODE));
    }
    let cpu = t0.elapsed().as_secs_f64();
    let audio = burst_samples as f64 / SAMPLE_RATE as f64;
    (audio, cpu, bursts.len(), e.afc_settle_attempts())
}

#[test]
#[ignore = "measurement"]
fn phase2_cost_on_a_busy_band() {
    let idle = load_corpus("ic9700-idle-hot.wav").expect("corpus idle");
    let busy = idle.cycled(0, BUSY_SECONDS * SAMPLE_RATE);

    // The same busy audio with a real frame in the middle: phase 1 decodes it, so phase 2 never
    // runs and this row is the phase-1-only cost.
    let mut tx = ChannelSimHarness::new();
    tx.tx_engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    tx.tx_engine
        .transmit_with_fec_mode(b"phase 2 cost probe", MODE, FecMode::Rs, None)
        .expect("transmit");
    let mut awgn = openpulse_channel::awgn::AwgnChannel::new(openpulse_channel::AwgnConfig::new(
        40.0,
        Some(7),
    ))
    .expect("awgn");
    let (_, frame) = tx.route_tapped(&mut awgn);
    let mut with_frame = idle.cycled(0, 4 * SAMPLE_RATE);
    with_frame.extend_from_slice(&frame);
    with_frame.extend(idle.cycled(4 * SAMPLE_RATE, 2 * SAMPLE_RATE));

    println!("\n=== #1118: phase-2 cost, CPU time against audio time ===");
    println!(
        "profile: {}  (a loaded machine measures the machine — run this idle)",
        if cfg!(debug_assertions) {
            "DEBUG — ~5x slower than release on this host (#1066); read as a pessimistic bound"
        } else {
            "release"
        }
    );
    println!(
        "\n{:<22} {:>10} {:>10} {:>9} {:>8} {:>9}",
        "fixture", "audio s", "cpu s", "cpu/audio", "bursts", "settles"
    );
    for (name, samples) in [
        ("busy band (no frame)", busy),
        ("on-frequency frame", with_frame),
    ] {
        let (audio, cpu, bursts, settles) = cost_of(&samples);
        println!(
            "{name:<22} {audio:>10.2} {cpu:>10.2} {:>9.3} {bursts:>8} {settles:>9}",
            cpu / audio.max(1e-9)
        );
    }
    println!(
        "\ncpu/audio >= 1.0 means the receive tick cannot keep up with its own capture stream:"
    );
    println!("time in the decoder is time not reading audio, so the NEXT frame is the one lost.");
}
