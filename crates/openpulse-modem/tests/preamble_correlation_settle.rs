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

/// THE ACCEPTANCE CASE: a coded frame in a saturating real noise floor decodes with the receiver
/// never once settling on the noise.
///
/// This is `a_coded_frame_decodes_through_a_saturating_floor` measured on a stricter axis. That
/// test allows up to 12 condemnations because the #1045 fix is *triggered* by a condemnation — the
/// gate only learns the floor is bad after it has already been fooled once. A correlation check
/// needs no such trigger: it can tell noise from a preamble on the first window, so the count
/// should be zero rather than merely small.
///
/// The lead is the point. A short lead passes even on the broken code because the recovery walk is
/// short enough to finish; before #1045 these leads gave 73-83 condemnations and no decode at all.
#[test]
fn the_receiver_never_settles_on_a_saturating_noise_floor() {
    let hot = corpus("ic9700-idle-hot.wav");
    // Guard the premise: if this file stopped saturating the energy gate, the test would silently
    // become an ordinary decode and prove nothing about correlation.
    assert!(
        hot.mean_sq() > 0.0032,
        "corpus floor {:.4} no longer saturates the gate — this test's premise is gone",
        hot.mean_sq()
    );

    for lead in [40_000usize, 80_000, 120_000] {
        let mut h = harness();
        h.tx_engine
            .transmit_with_fec_mode(b"correlation gate probe", "BPSK250", FecMode::Rs, None)
            .expect("transmit");
        h.route_embedded_in_capture(&hot, lead, 40_000, 0.3);

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

        assert_eq!(
            h.rx_engine.settle_condemnations(),
            0,
            "lead {lead}: the receiver settled on noise {} time(s) and had to recover. The \
             correlation check exists so that never happens in the first place — a condemnation \
             here means it is not gating the settle.",
            h.rx_engine.settle_condemnations()
        );

        // The tripwire. Zero condemnations is also what a completely inert gate produces on an
        // easy capture, so the count above is only evidence if the gate was actually exercised:
        // this floor DOES pass the energy gate (that is what saturation means), so something must
        // have refused those candidates.
        assert!(
            h.rx_engine.rho_rejected_settles() > 0,
            "lead {lead}: no settle was ever refused on correlation, yet this floor saturates the \
             energy gate — the check is inert and the zero above proves nothing"
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

/// A steady tone must NOT look like a preamble — and the frequency grid is what decides that.
///
/// This is the constraint that sizes `PREAMBLE_RHO_GRID_HZ`, and it is not a compute trade. The
/// BPSK preamble is 32 phase-alternating symbols: a square-wave-modulated carrier whose energy sits
/// in lines at `fc ± baud/2`. Correlating a *steady* carrier against it cancels between the +1 and
/// −1 half-symbols, which is why a tone scores near zero — but rotate the template by baud/2 and one
/// of those lines lands on plain carrier, and the cancellation is gone. Measured here:
///
/// | grid half-width | ρ of a pure tone |
/// |---|---|
/// | ±20 Hz (shipped) | 0.017–0.042 |
/// | ±160 Hz | 0.659 |
/// | ±450 Hz (the full acquisition range) | 0.696 at every frequency |
///
/// At ±160 Hz a birdie outscores this receiver's best measured real on-air frame (0.654). That is
/// the measurement that rejected the otherwise-attractive design of searching the whole acquisition
/// range as a detector and seeding the settle from it: our sync word is two spectral lines, not a
/// pseudo-random sequence, so it cannot survive being searched over its own line spacing. Raising
/// the ceiling needs a different preamble, not a bigger grid — a wire-format change.
///
/// The repo has real exposure here: the second OTA rig was blocked by PC/USB birdies, not by the
/// modem. This test fails if a future grid widening quietly reopens that.
#[test]
fn the_gate_is_not_fooled_by_a_steady_tone() {
    let p = BpskPlugin::new();
    let cfg = ModulationConfig {
        mode: "BPSK250".into(),
        sample_rate: 8_000,
        center_frequency: 1_500.0,
        ..Default::default()
    };
    let template = p.preamble_template(&cfg).expect("template");
    let tlen = template.len();
    let mf = openpulse_dsp::acquisition::IqMatchedFilter::new(template);
    // The shipped grid: PREAMBLE_RHO_GRID_HZ = 20 Hz, step 0.25*fs/tlen = 2 Hz.
    let grid: Vec<f32> = (-10..=10).map(|k| k as f32 * 2.0).collect();

    for f in [1_250.0f32, 1_375.0, 1_500.0, 1_625.0, 1_750.0] {
        let tone: Vec<f32> = (0..tlen + 200)
            .map(|k| (2.0 * std::f32::consts::PI * f * k as f32 / 8_000.0).cos())
            .collect();
        let (r, _) = mf
            .search_normalized_over_frequency(&tone, 200, 0.05, 8_000.0, &grid)
            .expect("search");
        assert!(
            r.rho < 0.40,
            "a pure {f} Hz tone scores rho {:.3}, at or above the settle threshold — the grid has \
             been widened past ~baud/2 and the gate can no longer tell a birdie from a preamble",
            r.rho
        );
    }

    // The falsifier: widening the grid MUST break it, or the assertion above is passing for some
    // unrelated reason and says nothing about the grid width.
    let wide: Vec<f32> = (-40..=40).map(|k| k as f32 * 4.0).collect();
    let tone: Vec<f32> = (0..tlen + 200)
        .map(|k| (2.0 * std::f32::consts::PI * 1_500.0 * k as f32 / 8_000.0).cos())
        .collect();
    let (r, _) = mf
        .search_normalized_over_frequency(&tone, 200, 0.05, 8_000.0, &wide)
        .expect("search");
    assert!(
        r.rho > 0.40,
        "a +-160 Hz grid left a pure tone at rho {:.3}; the narrow-grid assertions above are then \
         not testing the grid width at all",
        r.rho
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
    // 31 of the 32 preamble symbols at 32 samples/symbol: the last is dropped because the
    // rectangular pulse crossfades a third of the first DATA symbol into it.
    assert_eq!(template.len(), 31 * 32);

    let frame = p.modulate(b"payload does not matter", &cfg).unwrap();
    let mf = openpulse_dsp::acquisition::IqMatchedFilter::new(template);
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
