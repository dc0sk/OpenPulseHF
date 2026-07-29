//! The loopback input can deliver audio at a REAL-TIME rate, not all at once.
//!
//! **The gap this closes.** The default `read()` returns the entire buffered capture in a single
//! call, so a receive loop always has the whole frame the instant it begins scanning. A real
//! capture arrives at the sample rate, and a scan pass slower than the audio it covers falls
//! permanently behind — the buffer grows faster than the scan walks it. That is a shipped failure
//! mode (the retry that starved the capture read loop, fixed 2026-07-20), and `CLAUDE.md` records
//! it as untestable in-process precisely because there was "no read cadence to starve".
//!
//! These tests are wall-clock based, so they assert on *progressiveness and ordering* — never on
//! exact sample counts, which would be flaky on a loaded machine.

use std::thread::sleep;
use std::time::Duration;

use openpulse_audio::LoopbackBackend;
use openpulse_core::audio::{AudioBackend, AudioConfig};

fn cfg() -> AudioConfig {
    AudioConfig::default()
}

/// Unpaced is the historical behaviour and must stay that way: one read drains everything.
/// This is the control — if it ever stops holding, the paced assertions below mean nothing.
#[test]
fn without_pacing_a_single_read_drains_the_whole_buffer() {
    let backend = LoopbackBackend::new();
    backend.fill_samples(&vec![0.25f32; 10_000]);
    let mut stream = backend.open_input(None, &cfg()).expect("input");
    let first = stream.read().expect("read");
    assert_eq!(
        first.len(),
        10_000,
        "unpaced loopback must hand back the entire capture in one read"
    );
}

/// Paced delivery must hand back only what has "arrived", and must not deliver the whole buffer up
/// front — that is the entire point.
#[test]
fn pacing_delivers_progressively_rather_than_all_at_once() {
    // 8 kHz: 40 000 samples is 5 s of audio, far more than this test will wait for.
    let backend = LoopbackBackend::new().with_pacing(8_000.0);
    backend.fill_samples(&vec![0.1f32; 40_000]);
    let mut stream = backend.open_input(None, &cfg()).expect("input");

    let first = stream.read().expect("read");
    assert!(
        first.len() < 40_000,
        "paced read returned {} of 40 000 samples immediately — pacing is not being applied, and \
         any starvation test built on it would be vacuous",
        first.len()
    );

    sleep(Duration::from_millis(120));
    let second = stream.read().expect("read");
    assert!(
        !second.is_empty(),
        "after 120 ms at 8 kHz more audio must have arrived, but the paced read returned nothing"
    );

    // Total delivered must still be a small fraction of the buffer after ~120 ms of a 5 s capture.
    let delivered = first.len() + second.len();
    assert!(
        delivered < 20_000,
        "delivered {delivered} samples in ~120 ms of a 5 s capture — the pacing rate is not being \
         honoured"
    );
}

/// Samples must arrive in order and without loss: pacing changes WHEN audio is delivered, never
/// WHAT. A pacing implementation that dropped samples would corrupt every decode built on it.
#[test]
fn pacing_preserves_sample_order_and_loses_nothing() {
    let n = 4_000usize;
    let expected: Vec<f32> = (0..n).map(|i| i as f32).collect();
    let backend = LoopbackBackend::new().with_pacing(20_000.0);
    backend.fill_samples(&expected);
    let mut stream = backend.open_input(None, &cfg()).expect("input");

    let mut got: Vec<f32> = Vec::new();
    // 4 000 samples at 20 kHz is 200 ms; poll well past that.
    for _ in 0..80 {
        got.extend(stream.read().expect("read"));
        if got.len() >= n {
            break;
        }
        sleep(Duration::from_millis(10));
    }

    assert_eq!(got.len(), n, "paced delivery lost or duplicated samples");
    assert_eq!(got, expected, "paced delivery reordered samples");
}

/// A zero or negative rate disables pacing rather than stalling the stream forever — a silent
/// stall in a receive loop is far worse than an unpaced read.
#[test]
fn a_non_positive_rate_disables_pacing() {
    let backend = LoopbackBackend::new().with_pacing(0.0);
    backend.fill_samples(&vec![0.5f32; 2_048]);
    let mut stream = backend.open_input(None, &cfg()).expect("input");
    assert_eq!(
        stream.read().expect("read").len(),
        2_048,
        "a non-positive pacing rate must fall back to unpaced delivery, not stall"
    );
}
