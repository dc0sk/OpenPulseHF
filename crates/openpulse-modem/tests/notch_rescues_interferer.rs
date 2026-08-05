//! The receiver notch must earn its default-on status: it has to rescue a decode that fails without
//! it (REQ-QRM-01).
//!
//! **Why this test exists at all.** The auto-notch was built, documented as "a clear win against
//! out-of-band QRM", and then left **opt-in** — so it was off in every recorded on-air failure. That
//! is how "we already harden against interference" and "the station could not decode" stayed true at
//! the same time. Built-and-never-enabled is not the same archetype as a seam gap, and it is not
//! caught by the same tests: the wiring was correct the whole time, nothing ever switched it on.
//!
//! Flipping a default without evidence would just move the guess, so this pins the measurement that
//! justified it, at the level where it is decisive.
//!
//! **Measured operating band** (recorded IC-9700 hot floor, `BPSK250 + Rs`, 2200 Hz interferer just
//! outside the mode's ~1250–1750 Hz occupied band):
//!
//! | tone amplitude | notch off | notch on |
//! |---|---|---|
//! | 0.05, 0.15 | OK | OK — unnecessary, nothing to rescue |
//! | **0.30** | **FAIL** | **OK** — the whole reason for the default |
//! | 0.60 | FAIL | FAIL — the interferer wins regardless |
//!
//! A notch is not a cure for arbitrary QRM; it buys a band of conditions. Pinning both edges keeps
//! anyone from reading it as more than that.

use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::channel_sim::ChannelSimHarness;
use openpulse_modem::ModemEngine;

/// Interferer amplitude at which the notch is the difference between a link and no link.
const RESCUE_AMPLITUDE: f32 = 0.30;
/// Well outside BPSK250's occupied band at fc 1500, so it is notchable rather than a QSY case.
const INTERFERER_HZ: f32 = 2_200.0;

fn frame() -> Vec<f32> {
    let lb = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    e.transmit_with_fec_mode(b"notch rescue probe", "BPSK250", FecMode::Rs, None)
        .expect("transmit");
    lb.drain_samples()
}

/// Decode a frame in the recorded hot floor with an added out-of-band tone, with the notch on or off.
fn decodes_with(notch: bool, amplitude: f32) -> bool {
    let hot = load_corpus("ic9700-idle-hot.wav").expect("corpus");
    let f = frame();
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    }
    if notch {
        h.rx_engine.enable_notch();
    }
    // #1066: the receive verdict is bounded by WALL CLOCK, so the same input decodes 5/5 on an
    // idle machine and 0/5 on eight busy cores — and debug-vs-release is just a ~5x speed proxy
    // for that. Bound the search in WORK instead, so this asserts a property of the signal rather
    // than of the host. The budget reconciles every fixture in the #1058 family (PR #1070); it is
    // chosen, not derived, and a derived one sized to the reference hardware is still owed.
    h.rx_engine.set_deterministic_scan_positions(Some(8_000));
    h.rx_engine.set_deterministic_max_iterations(Some(64_000));

    let mut buf = hot.cycled(0, 40_000);
    buf.extend(f.iter().map(|s| s * 0.3));
    buf.extend(hot.cycled(40_000, 40_000));
    for (n, s) in buf.iter_mut().enumerate() {
        *s += amplitude * (2.0 * std::f32::consts::PI * INTERFERER_HZ * n as f32 / 8_000.0).cos();
    }
    h.feed_capture(&Capture {
        samples: buf,
        sample_rate: 8_000,
    });
    h.rx_engine
        .receive_with_fec_mode_timeout("BPSK250", FecMode::Rs, None, Duration::from_millis(40_000))
        .map(|got| got == b"notch rescue probe")
        .unwrap_or(false)
}

/// THE GATE: at the measured rescue level the notch turns a failure into a decode.
///
/// Both halves are asserted. Without the negative control this would pass on a build where the notch
/// does nothing at all and the interferer simply never mattered — which is exactly the state the
/// feature was in for its whole life.
#[test]
fn the_notch_rescues_a_decode_that_fails_without_it() {
    assert!(
        !decodes_with(false, RESCUE_AMPLITUDE),
        "the decode SUCCEEDED without the notch at interferer amplitude {RESCUE_AMPLITUDE}. The \
         rescue below then proves nothing — re-derive the level at which the interferer actually \
         breaks acquisition and move this test there."
    );
    assert!(
        decodes_with(true, RESCUE_AMPLITUDE),
        "the notch failed to rescue a decode at interferer amplitude {RESCUE_AMPLITUDE} — the \
         measurement that justified defaulting it on no longer holds"
    );
}

/// The honest upper edge: a strong enough interferer wins anyway.
///
/// Recorded so the default is not mistaken for immunity. This is also the case
/// `auto_qsy_on_interference` exists for.
#[test]
fn a_strong_enough_interferer_defeats_the_notch_too() {
    assert!(
        !decodes_with(true, 0.60),
        "a 0.60-amplitude interferer no longer defeats the notch. Good news, but the operating band \
         in this file's header is now wrong and should be re-measured rather than left stale."
    );
}
