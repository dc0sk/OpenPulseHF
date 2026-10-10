//! The AFC settle must be corroborated by preamble CORRELATION, not by energy alone (#1049).
//!
//! Five separately-diagnosed defects — #1020 (capture level), #1021 (settle livelock), #1039 (gate
//! cold start), #1040 (re-anchor crawl), #1045 (saturated gate) — were one mechanism patched five
//! times: an energy gate deciding where a frame starts *and* triggering the AFC settle. Energy can
//! only answer "is something here"; on a band floor above the gate the answer is always yes, the
//! receiver settles AFC on idle noise before the frame arrives, and then spends its listen window
//! re-decoding that position. Every deployed reference modem (codec2/FreeDV `ofdm.c`, Mercury,
//! modem73) decides frame start on normalised correlation, which makes that failure impossible by
//! construction.
//!
//! These tests pin the two halves that must BOTH hold, because either alone is easy:
//!
//! 1. **False accept** — recorded idle noise must not be settled on (measured by the condemnation
//!    count, not by whether a decode eventually happened; `the_real_on_air_frame_decodes` passed
//!    throughout the 78-condemnation livelock).
//! 2. **False reject** — a real frame must still be settled on, including one far off-frequency,
//!    which is what makes the *placement* of this check load-bearing rather than incidental.

use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_core::fec::FecMode;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::channel_sim::ChannelSimHarness;

mod common;
use common::saturating_floor as fixture;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    }
    h
}

fn corpus(name: &str) -> Capture {
    load_corpus(name).unwrap_or_else(|e| panic!("corpus file {name} must load: {e}"))
}

/// THE ACCEPTANCE CASE: a coded frame in a saturating real noise floor decodes, and the correlation
/// check is demonstrably the thing refusing the noise.
///
/// **Read the bound carefully — it is 4, not 0, and the difference is a measured correction to my
/// own reasoning.** This test first asserted zero, on the argument that a correlation check needs no
/// trigger (unlike #1045's `condemned_floor`, which only learns a floor is bad *after* being fooled
/// once) and so should never settle on noise at all. Instrumented, that argument was right about the
/// noise and wrong about the count: the surviving settles are not noise. They sit on the frame's
/// leading EDGE — onsets 39328…39972 for a frame at 40000, ρ climbing 0.461 → 1.000 as the window
/// slides onto it — where a partial overlap genuinely contains preamble, clears the threshold
/// honestly, and still cannot be demodulated because the preamble is truncated.
///
/// Snapping the onset to the correlation's own answer removes them, and was built; see the note at
/// the check in `engine.rs` for the measurement that rejected it (an alternating preamble is
/// periodic, so the same search misplaces a *correct* onset by two symbols on the capture-AGC
/// fixture — and every rule separating the two cases is a constant fitted to those two fixtures).
///
/// So what this pins is what the veto actually buys, which is narrower than #1049 predicted: the
/// settle-on-NOISE class is gone, the frame decodes at every lead, and the residual edge settles
/// stay well inside #1045's ≤ 12 budget.
///
/// The lead is the point. A short lead passes even on the broken code because the recovery walk is
/// short enough to finish; before #1045 these leads gave 73-83 condemnations and no decode at all.
#[test]
fn the_receiver_never_settles_on_a_saturating_noise_floor() {
    let hot = corpus(fixture::CORPUS);
    // Guard the premise: if this file stopped saturating the energy gate, the test would silently
    // become an ordinary decode and prove nothing about correlation.
    assert!(
        hot.mean_sq() > fixture::GATE_CEILING_MEAN_SQ,
        "corpus floor {:.4} no longer saturates the gate — this test's premise is gone",
        hot.mean_sq()
    );

    for lead in fixture::LEADS {
        let mut h = harness();
        h.tx_engine
            .transmit_with_fec_mode(fixture::PAYLOAD, fixture::MODE, FecMode::Rs, None)
            .expect("transmit");
        h.route_embedded_in_capture(&hot, lead, fixture::TRAIL, fixture::EMBED_LEVEL);

        // #1066: bound the search in WORK, not wall clock — the same input decodes 5/5 idle and
        // 0/5 on eight busy cores, and debug-vs-release is a ~5x speed proxy for that. Chosen to
        // reconcile the #1058 family (PR #1070), not derived.
        h.rx_engine.set_deterministic_scan_positions(Some(8_000));
        h.rx_engine.set_deterministic_max_iterations(Some(64_000));
        let got = h
            .rx_engine
            .receive_with_fec_mode_timeout(
                "BPSK250",
                FecMode::Rs,
                None,
                Duration::from_millis(40_000),
            )
            .unwrap_or_else(|e| {
                panic!(
                    "lead {lead}: a coded frame must decode through a saturating floor: {e} \
                     (condemnations {}, rho rejections {})",
                    h.rx_engine.settle_condemnations(),
                    h.rx_engine.rho_rejected_settles()
                )
            });
        assert_eq!(String::from_utf8_lossy(&got), "correlation gate probe");

        // Bound taken from measurement (4 at every lead), not from taste. The residual settles are
        // the leading-edge ones described above; a rise past this means the NOISE class is back,
        // since noise settles arrive in the dozens (73-83 pre-#1045), never in ones and twos.
        let condemnations = h.rx_engine.settle_condemnations();
        assert!(
            condemnations <= 6,
            "lead {lead}: {condemnations} settle condemnations (~{} wasted fully-buffered decodes). \
             Measured behaviour is 4 leading-edge settles; a jump from here means the receiver is \
             settling on NOISE again, which is what the correlation check exists to prevent.",
            condemnations * 18
        );

        // The tripwire, and the reason the bound above is evidence rather than coincidence: a
        // completely inert gate also produces a low count on a lucky capture. This floor DOES pass
        // the energy gate — that is what saturation means — so something must have refused those
        // candidates, and only the correlation check can have.
        assert!(
            h.rx_engine.rho_rejected_settles() > 0,
            "lead {lead}: no settle was ever refused on correlation, yet this floor saturates the \
             energy gate — the check is inert and the count above proves nothing"
        );
    }
}

/// The false-reject half: a real frame must still be acquired when it is far off-frequency.
///
/// This is what fixes the placement of the check. A matched filter integrates coherently, so ρ of a
/// real BPSK250 frame against its own preamble template falls to 0.332 at a 20 Hz carrier offset
/// and 0.016 at 400 Hz — while this acquisition chain is required to pull in ±600 Hz
/// (`carrier_offset_acquisition.rs`). Checking correlation *before* the AFC settle, as issue #1049
/// originally proposed, would therefore reject nearly every off-frequency frame. Running it after
/// the settle, against the corrected frequency, is what makes it a waveform test rather than a
/// frequency test.
#[test]
fn an_off_frequency_frame_is_still_settled_on() {
    for offset_hz in [0.0f32, 20.0, 200.0, 400.0] {
        let payload = b"off-frequency but real".to_vec();
        let mut h = harness();
        h.tx_engine.transmit(&payload, "BPSK250", None).expect("tx");
        h.route_with_cfo(offset_hz);
        let got = h
            .rx_engine
            .receive_with_timeout("BPSK250", None, Duration::from_millis(8_000))
            .unwrap_or_else(|e| {
                panic!(
                    "a real frame at {offset_hz} Hz offset must still be acquired: {e} \
                     ({} settles refused on correlation)",
                    h.rx_engine.rho_rejected_settles()
                )
            });
        assert_eq!(got, payload);
    }
}

/// A mode whose plugin publishes no preamble template keeps the pre-#1049 energy-only behaviour.
///
/// The trait method defaults to `None` so this degrades gracefully rather than forking the engine,
/// and the counter must stay at zero — if it did not, the correlation path would be running against
/// some other mode's template, which is worse than not running at all.
#[test]
fn a_mode_without_a_template_is_unaffected() {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
            .unwrap();
    }
    let payload = b"no template here".to_vec();
    h.tx_engine.transmit(&payload, "QPSK500", None).expect("tx");
    h.route_clean();
    let got = h
        .rx_engine
        .receive_with_timeout("QPSK500", None, Duration::from_millis(8_000))
        .expect("QPSK500 must decode exactly as before");
    assert_eq!(got, payload);
    assert_eq!(
        h.rx_engine.rho_rejected_settles(),
        0,
        "a plugin with no preamble template must not be correlation-gated"
    );
}

/// A steady tone must NOT look like BPSK250's preamble, at any frequency near the carrier (#1062).
///
/// Rewritten at the #1062 flag day. Its predecessor pinned the alternating `--++` preamble's flaw: a
/// period-4 run is two spectral lines at `fc ± baud/4`, so a tone ON a line scored ρ ≈ 0.70 at any
/// grid width and only the ±20 Hz grid kept tones BETWEEN lines out (`docs/dev/sharp-edges.md`,
/// item 2, and `pn-preamble.md`). PN-63 has no lines, so the claim becomes the one the old test
/// could not make: a lone tone swept across fc ± 200 Hz stays under the threshold everywhere.
///
/// The falsifier is the same sweep, same grid rule, on the retired alternating template
/// (`BPSK250-ALT`): it must be fooled somewhere, or the sweep cannot detect a fooling tone at all.
#[test]
fn the_gate_is_not_fooled_by_a_steady_tone() {
    let sweep = |mode: &str| -> (f32, f32, f32) {
        let p = if mode.ends_with("-ALT") {
            BpskPlugin::measurement_arms()
        } else {
            BpskPlugin::new()
        };
        let cfg = ModulationConfig {
            mode: mode.into(),
            sample_rate: 8_000,
            center_frequency: 1_500.0,
            ..Default::default()
        };
        let template = p.preamble_template(&cfg).expect("template");
        let tlen = template.samples.len();
        // The shipped grid, built the way the engine builds it: step = 0.25*fs/tlen.
        let step = (0.25 * 8_000.0 / tlen as f32).max(0.5);
        let n = (template.rho_grid_hz / step).round() as i32;
        let grid: Vec<f32> = (-n..=n).map(|k| k as f32 * step).collect();
        let threshold = template.rho_threshold;
        let mf = openpulse_dsp::acquisition::IqMatchedFilter::new(template.samples);
        let (worst, at) = (-200..=200)
            .step_by(2)
            .map(|d| {
                let f = 1_500.0 + d as f32;
                (tone_rho(&mf, f, tlen, &grid), f)
            })
            .fold((0.0f32, 0.0f32), |acc, x| if x.0 > acc.0 { x } else { acc });
        println!("{mode}: worst steady tone rho {worst:.3} at {at} Hz, threshold {threshold}");
        (worst, at, threshold)
    };

    let (worst, at, threshold) = sweep("BPSK250");
    assert!(
        worst < threshold,
        "a steady tone at {at} Hz scores rho {worst:.3} against BPSK250's PN-63 template, at or above \
         its threshold {threshold}: the veto would corroborate a birdie"
    );

    let (alt_worst, alt_at, alt_threshold) = sweep("BPSK250-ALT");
    assert!(
        alt_worst > alt_threshold,
        "the alternating template's worst tone ({alt_worst:.3} at {alt_at} Hz) stays under its \
         threshold {alt_threshold}: the sweep no longer finds the on-line tone that fools it, so the \
         PN assertion above is not evidence"
    );
}

/// The template must come from the modulator, and must stop before the data symbols.
///
/// A hand-copied template drifts out of step with the modulator the first time either changes, and
/// a template that no longer matches the wire stops detecting frames *silently* — it does not fail,
/// it just never corroborates a settle again. Correlating a mode's own preamble against its own
/// modulated frame is the cheap check that they are still the same waveform.
#[test]
fn the_bpsk_template_matches_the_front_of_a_real_frame() {
    let p = BpskPlugin::new();
    let cfg = ModulationConfig {
        mode: "BPSK250".into(),
        sample_rate: 8_000,
        center_frequency: 1_500.0,
        ..Default::default()
    };
    let template = p
        .preamble_template(&cfg)
        .expect("BPSK must publish a preamble template");
    // 62 of the 63 PN-63 preamble symbols at 32 samples/symbol: the last is dropped because the
    // rectangular pulse crossfades a third of the first DATA symbol into it.
    assert_eq!(template.samples.len(), 62 * 32);

    let frame = p.modulate(b"payload does not matter", &cfg).unwrap();
    let mf = openpulse_dsp::acquisition::IqMatchedFilter::new(template.samples);
    let r = mf
        .search_normalized(&frame, 2_000, 0.05)
        .expect("the template must be findable in the frame it was built from");
    assert!(
        r.rho > 0.95,
        "template correlates at only rho {:.3} against a real frame — it is not the same waveform \
         the modulator emits",
        r.rho
    );
    assert!(
        r.offset < 32,
        "template found at offset {} rather than the start of the frame",
        r.offset
    );

    // The falsifier: a DIFFERENT mode's frame must not correlate. Without this, a template of all
    // zeros or a degenerate one would pass the assertion above.
    let other = p
        .modulate(
            b"different waveform",
            &ModulationConfig {
                mode: "BPSK31".into(),
                ..cfg.clone()
            },
        )
        .unwrap();
    let r2 = mf.search_normalized(&other, 2_000, 0.05).expect("search");
    assert!(
        r2.rho < 0.6,
        "the BPSK250 template correlates at rho {:.3} against a BPSK31 frame — it is not \
         discriminating between waveforms",
        r2.rho
    );
}

/// Peak rho of a pure tone at `f` against `mf`, over `grid`.
fn tone_rho(
    mf: &openpulse_dsp::acquisition::IqMatchedFilter,
    f: f32,
    tlen: usize,
    grid: &[f32],
) -> f32 {
    let tone: Vec<f32> = (0..tlen + 200)
        .map(|k| (2.0 * std::f32::consts::PI * f * k as f32 / 8_000.0).cos())
        .collect();
    mf.search_normalized_over_frequency(&tone, 200, 0.05, 8_000.0, grid)
        .map(|(r, _)| r.rho)
        .unwrap_or(0.0)
}
