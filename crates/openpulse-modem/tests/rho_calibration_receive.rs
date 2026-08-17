//! #1060 — the correlation threshold must track the station's own receive bandwidth.
//!
//! Driven through the **production receive entry** (`receive_with_timeout_fec`, the function the CLI
//! `receive --listen-ms` path and the on-air scripts call) on the three recorded IC-9700 idle
//! captures, which differ in exactly one variable: the rig's receive filter width, set over CAT and
//! verified from the audio at record time.
//!
//! The unit behaviour of the estimator lives beside it in `src/rho_calibration.rs`. What this file
//! adds is the half a unit test cannot reach: that the calibration is *wired*, that it is fed from
//! real recorded noise rather than a synthetic stream, and that its tripwire moves.

use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::ModemEngine;

const MODE: &str = "BPSK250";
const PUBLISHED: f32 = bpsk_plugin::modulate::PREAMBLE_RHO_THRESHOLD;
/// Bounded work, not wall clock: a verdict that depends on machine load is a measurement of the
/// machine (#1066).
const SCAN_POSITIONS: usize = 1_200;
const MAX_ITERATIONS: usize = 250;
/// Listen window, and the slice of capture fed in.
///
/// Sized from the measured query rate, not guessed: a full 45 s replay of `ic9700-idle-500hz.wav`
/// accumulates 281 thinned samples, so 12 s clears `MIN_SAMPLES` (64) with room to spare while
/// keeping the gate to about a minute per capture.
const LISTEN_SECS: u64 = 20;
const SLICE_SECS: usize = 20;

/// Listen to a recorded capture through the production receive path and report what the calibration
/// made of it.
fn listen(capture: &str) -> (usize, f32, (bool, u64)) {
    listen_scaled(capture, 1.0)
}

/// As `listen`, with the capture's level scaled.
///
/// Scaling exists for one reason and it is not cosmetic: the calibration is fed by settle attempts,
/// and settles only happen when the **energy gate** fires. The 250 Hz capture's floor (mean-square
/// 2.1e-3) sits under the gate's clamped threshold, so replayed as recorded it produces zero settles
/// and zero samples — the receiver correctly hears nothing. Scaling it into the hot-floor regime the
/// veto exists for makes the gate fire without touching the quantity under test: ρ is a *normalised*
/// correlation, so it is invariant to level by construction.
fn listen_scaled(capture: &str, gain: f32) -> (usize, f32, (bool, u64)) {
    listen_full(
        capture,
        gain,
        SLICE_SECS,
        LISTEN_SECS,
        (SCAN_POSITIONS, MAX_ITERATIONS),
    )
}

/// As `listen_scaled`, with the slice and listen window chosen per fixture.
fn listen_full(
    capture: &str,
    gain: f32,
    slice_secs: usize,
    listen_secs: u64,
    budget: (usize, usize),
) -> (usize, f32, (bool, u64)) {
    let backend = LoopbackBackend::new();
    let handle = backend.clone_shared();
    let mut rx = ModemEngine::new(Box::new(backend));
    rx.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register BPSK");
    rx.set_deterministic_scan_positions(Some(budget.0));
    rx.set_deterministic_max_iterations(Some(budget.1));

    let c = load_corpus(capture).unwrap_or_else(|e| panic!("{capture}: {e}"));
    let n = (slice_secs * c.sample_rate as usize).min(c.samples.len());
    let slice: Vec<f32> = c.samples[..n].iter().map(|s| s * gain).collect();
    handle.fill_samples(&slice);
    // Idle audio: this is expected to find nothing, and finding nothing is the point — the
    // calibration is fed by the settle attempts the search makes along the way.
    let _ =
        rx.receive_with_fec_mode_timeout(MODE, FecMode::Rs, None, Duration::from_secs(listen_secs));
    println!(
        "{capture}: samples={} derived={:.3} stand_down={:?} rho_rejected={} condemnations={}",
        rx.rho_calibration_samples(),
        rx.rho_effective_threshold(MODE).unwrap_or(f32::NAN),
        rx.rho_stand_down(),
        rx.rho_rejected_settles(),
        rx.condemned_positions().len(),
    );

    (
        rx.rho_calibration_samples(),
        rx.rho_effective_threshold(MODE)
            .expect("BPSK250 publishes a template"),
        rx.rho_stand_down(),
    )
}

/// The tripwire first: a calibration that never runs reads as a working feature.
///
// VERIFIES: REQ-RX-02
#[test]
fn the_calibration_is_fed_by_the_production_receive_path() {
    let (samples, _, _) = listen("ic9700-idle-500hz.wav");
    assert!(
        samples > 0,
        "no correlation samples reached the calibration — it is not wired into the receive path"
    );
}

/// The requirement itself: a narrower receive filter must raise the threshold.
///
// VERIFIES: REQ-RX-02
#[test]
fn a_narrow_receive_filter_raises_the_threshold_above_the_published_constant() {
    let (n_wide, wide, _) = listen("ic9700-idle-wide-500hz-control.wav");
    let (n_narrow, narrow, _) = listen("ic9700-idle-500hz.wav");
    assert!(
        n_wide > 0 && n_narrow > 0,
        "both captures must feed samples"
    );

    // The control: at SSB-class bandwidth the station's own noise demands nothing, so the published
    // constant stands. Without this row the test would pass on a calibration that raises the
    // threshold unconditionally.
    assert!(
        (wide - PUBLISHED).abs() < 1e-6,
        "wide-filter capture moved the threshold to {wide}; it must stay at the published {PUBLISHED}"
    );
    assert!(
        narrow > PUBLISHED,
        "500 Hz capture derived {narrow}, not above the published {PUBLISHED} — the measured \
         per-window noise ceiling there is 0.334 (p99) with a 45 s peak of 0.413, so a threshold at \
         {PUBLISHED} corroborates noise"
    );
}

/// REQ-RX-03: when the derived threshold passes the delivered-frame bound, the veto stands down
/// rather than rejecting every settle.
///
// VERIFIES: REQ-RX-03
///
/// `#[ignore]`d for cost, and the cost is itself the finding: a station whose veto is already broken
/// spends its scan budget **decoding the noise it wrongly corroborates**, so it makes far fewer
/// settle queries per unit of audio than a healthy one (measured: 159 queries in 45 s here against
/// 963 in 20 s on the 500 Hz capture, at the same budget). Reaching a calibrated state on that
/// station therefore needs roughly ten times the scan budget — which is minutes of CPU, not seconds.
/// The mechanism itself is gated cheaply in `src/rho_calibration.rs` (hysteresis, poison direction,
/// threshold arithmetic), all sabotage-verified; this is the production-path corroboration.
#[test]
#[ignore = "verification"]
fn the_veto_stands_down_when_no_threshold_separates() {
    // 2.6x lifts the 250 Hz floor to the hot-floor level the other captures were recorded at; see
    // `listen_scaled` for why that is required and why it cannot affect ρ.
    // The whole 45 s: a station whose veto is already broken spends its scan budget DECODING the
    // noise it wrongly corroborates, so it makes far fewer settle queries per second than a healthy
    // one (159 in 20 s here against 963 on the 500 Hz capture). Reaching a calibrated state
    // therefore takes more audio, not less.
    let (n, derived, (stood_down, settles)) =
        listen_full("ic9700-idle-250hz.wav", 2.6, 45, 120, (12_000, 4_000));
    assert!(n > 0, "capture must feed samples");
    assert!(
        derived > bpsk_plugin::modulate::DELIVERED_FRAME_RHO_BOUND,
        "250 Hz capture derived {derived}, which does not exceed the delivered-frame bound — this \
         fixture no longer exercises the stand-down path"
    );
    assert!(
        stood_down,
        "derived {derived} is past the delivered-frame bound {} yet the veto did not stand down",
        bpsk_plugin::modulate::DELIVERED_FRAME_RHO_BOUND
    );
    assert!(
        settles > 0,
        "stand-down is latched but no settle was let through by it — the state is not observable"
    );
}
