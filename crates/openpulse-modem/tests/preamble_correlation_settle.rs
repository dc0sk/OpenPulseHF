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

/// A steady tone must NOT look like a preamble — and the frequency grid is what decides that.
///
/// This is the constraint that sizes `PREAMBLE_RHO_GRID_HZ`, and it is not a compute trade. The
/// BPSK preamble's 32 *bits* alternate, but NRZI flips phase only on a `1`, so the *symbols* are
/// `--++` repeating: a square wave of period **four** symbols whose energy sits in lines at
/// `fc ± baud/4` and odd harmonics (measured at BPSK250: ±62.5 Hz, ±187.5, ±312.5 — nothing at
/// ±125). Correlating a *steady* carrier against it cancels between half-symbols, which is why a
/// tone BETWEEN lines scores near zero — but a tone ON a line needs no rotation at all and scores
/// ρ ≈ 0.70 at any grid width. Corrected 2026-08-03; this said `baud/2`, twice the true spacing.
/// Measured here:
///
/// | grid half-width | tone BETWEEN lines | tone ON a line (`fc ± 62.5`) |
/// |---|---|---|
/// | ±20 Hz (shipped) | 0.017–0.042 | **0.700** |
/// | ±160 Hz | 0.659 | 0.700 |
/// | ±450 Hz (the full acquisition range) | 0.696 at every frequency | 0.700 |
///
/// **The second column does not depend on the grid**, which is why this test probes both. A tone
/// landing on a line captures ~half the template's energy — ρ ≈ √0.5 — with no rotation needed, so
/// narrowing the grid narrows the vulnerable bands around each line (±25 Hz at the shipped width)
/// but cannot remove them. The deployed chain still refuses a *lone* tone, because the settle lands
/// on it and parks it ~baud/4 from both rotated lines; sideband-symmetric interference gets no such
/// protection (see `preamble_veto_interference`).
///
/// At ±160 Hz a birdie outscores this receiver's best measured real on-air frame (0.654). That is
/// the measurement that rejected the otherwise-attractive design of searching the whole acquisition
/// range as a detector and seeding the settle from it: our sync word is a few spectral lines, not a
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
    let tlen = template.samples.len();
    let threshold = template.rho_threshold;
    let samples_for_spectrum = template.samples.clone();
    // The shipped grid, built the way the engine builds it: step = 0.25*fs/tlen.
    let step = (0.25 * 8_000.0 / tlen as f32).max(0.5);
    let n = (template.rho_grid_hz / step).round() as i32;
    let grid: Vec<f32> = (-n..=n).map(|k| k as f32 * step).collect();
    let mf = openpulse_dsp::acquisition::IqMatchedFilter::new(template.samples);

    // The probe frequencies are LOCATED FROM THE TEMPLATE'S OWN SPECTRUM, not written down.
    //
    // The previous list was 1250/1375/1500/1625/1750 — 125 Hz steps. The preamble's lines sit at
    // ODD multiples of baud/4 (±62.5, ±187.5, ±312.5 for BPSK250: the bits alternate but NRZI
    // flips only on a 1, so the SYMBOLS are `--++` repeating with a period of four), which put
    // every one of those probes on an EVEN multiple — maximally far from every line. The test
    // measured 0.017–0.043 and passed while a tone at 1437.5 scores 0.700. Hardcoding 1437.5
    // instead would be the same bug one wire-format change later, so the peaks are found at run
    // time and the sequence can change underneath this test without silently disarming it.
    let peaks = template_line_offsets(&samples_for_spectrum, 8_000.0, 1_500.0);
    assert!(
        peaks.len() >= 2,
        "no spectral lines found in the preamble template; the probe cannot locate its own targets \
         and every assertion below would be vacuous"
    );

    // ON a line, the correlator IS fooled — that is a property of a two-line sync word, not a
    // regression, and asserting otherwise would assert something false. What protects the deployed
    // receiver is the AFC settle, which lands on a lone tone and so parks it ~baud/4 from both
    // rotated lines; that is a SYSTEM property and is gated in `preamble_veto_interference`, not
    // here. This test's job is the component claim its name makes, scoped honestly.
    for &off in peaks.iter().take(2) {
        let f = 1_500.0 + off;
        let rho = tone_rho(&mf, f, tlen, &grid);
        assert!(
            rho > threshold,
            "a tone at {f} Hz — ON the template's own spectral line at {off:+.1} Hz — scores only \
             {rho:.3}. Either the preamble is no longer two-line (in which case this test's whole \
             premise, and #1062's, needs re-deriving) or the probe is no longer finding the lines."
        );
    }

    // BETWEEN the lines the correlator is not fooled, and that is what the shipped ±20 Hz grid
    // buys: the vulnerable bands stay narrow instead of covering the passband.
    for w in peaks.windows(2) {
        let f = 1_500.0 + (w[0] + w[1]) / 2.0;
        let rho = tone_rho(&mf, f, tlen, &grid);
        assert!(
            rho < threshold,
            "a pure {f} Hz tone — BETWEEN the template's lines — scores rho {rho:.3}, at or above \
             the settle threshold. The grid has been widened until it can rotate a line onto any \
             frequency, and the gate can no longer tell a birdie from a preamble."
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
        r.rho > threshold,
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
    assert_eq!(template.samples.len(), 31 * 32);

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

/// Offsets, in Hz from `fc`, of the dominant spectral lines in a modulated preamble template.
///
/// Located from the template itself so a change of sync word moves the probe points with it. A
/// hardcoded frequency here would go stale the moment the preamble changes and would then be
/// testing a frequency the new template has no energy at — passing, and meaning nothing.
fn template_line_offsets(samples: &[f32], fs: f32, fc: f32) -> Vec<f32> {
    use rustfft::{num_complex::Complex, FftPlanner};
    let n = 8_192;
    let mut buf: Vec<Complex<f32>> = samples
        .iter()
        .map(|&v| Complex::new(v, 0.0))
        .chain(std::iter::repeat_n(Complex::new(0.0, 0.0), n))
        .take(n)
        .collect();
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);
    let bin_hz = fs / n as f32;
    let mag: Vec<f32> = buf.iter().take(n / 2).map(|c| c.norm()).collect();
    let peak = mag.iter().cloned().fold(0.0f32, f32::max);
    // Local maxima above half the global peak, within +/-500 Hz of fc: the lines that carry enough
    // energy for a tone landing on them to capture a large share of the correlation.
    let mut out: Vec<f32> = (1..mag.len() - 1)
        .filter(|&k| {
            let f = k as f32 * bin_hz;
            (f - fc).abs() <= 500.0
                && mag[k] > peak * 0.5
                && mag[k] >= mag[k - 1]
                && mag[k] >= mag[k + 1]
        })
        .map(|k| k as f32 * bin_hz - fc)
        .collect();
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out.dedup_by(|a, b| (*a - *b).abs() < 10.0);
    out
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
