//! The correlation veto extended to QPSK — the ladder's faster rungs (#1053).
//!
//! #1049 made the AFC settle require preamble-correlation corroboration, which removes the
//! settle-on-noise class outright. It only runs where a plugin publishes a `preamble_template`, and
//! BPSK was the only one that did — so the protection stopped exactly where `hpx_hf` starts using
//! its faster rungs (BPSK on SL2–SL5, QPSK from SL6). A station on a hot band floor lost acquisition
//! on the modes it climbs to.
//!
//! **None of BPSK's constants transfer, and that is arithmetic rather than caution.** ρ is a
//! *normalised* correlation, so its noise floor is fixed by the template's length: measured over the
//! recorded idle corpus it follows `≈ 6.5/√len` for both waveforms. QPSK's preamble is 16 symbols to
//! BPSK's 32, so at the same baud its template is half as long and its noise ceiling √2 higher —
//! and BPSK's 0.40 threshold sits *below* QPSK500's measured idle-noise ceiling of 0.429. Inheriting
//! it would have corroborated settles on pure noise, silently, which is the defect the check exists
//! to prevent. The thresholds are therefore published per mode by the plugin
//! (`modulate::qpsk_preamble_threshold`), and the measurement that produced them is reproducible
//! with `tests/qpsk_preamble_rho_survey.rs`.

use std::time::Duration;

use openpulse_core::fec::FecMode;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_dsp::acquisition::IqMatchedFilter;
use openpulse_modem::capture_replay::{load_corpus, Capture};
use openpulse_modem::channel_sim::ChannelSimHarness;
use qpsk_plugin::QpskPlugin;

const FS: f32 = 8_000.0;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        sample_rate: 8_000,
        center_frequency: 1_500.0,
        ..Default::default()
    }
}

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    }
    h
}

fn corpus(name: &str) -> Capture {
    load_corpus(name).unwrap_or_else(|e| panic!("corpus file {name} must load: {e}"))
}

/// The engine's own search plan, so a measurement here is the measurement the receiver makes.
fn engine_grid(tlen: usize, half_width_hz: f32) -> Vec<f32> {
    let step = (0.25 * FS / tlen as f32).max(0.5);
    let n = (half_width_hz / step).round() as i32;
    (-n..=n).map(|k| k as f32 * step).collect()
}

/// Peak ρ of `window` against `mode`'s template, over the grid the engine would use.
fn rho(mode: &str, window: &[f32], grid_half_width_hz: Option<f32>) -> Option<f32> {
    let t = QpskPlugin::new().preamble_template(&cfg(mode))?;
    let hw = grid_half_width_hz.unwrap_or(t.rho_grid_hz);
    let grid = engine_grid(t.samples.len(), hw);
    let mf = IqMatchedFilter::new(t.samples);
    if window.len() <= mf.len() {
        return None;
    }
    mf.search_normalized_over_frequency(window, window.len() - mf.len(), 0.05, FS, &grid)
        .map(|(r, _)| r.rho)
}

/// THE ACCEPTANCE CASE: `QPSK500 + Rs` in the recorded saturating floor, at the leads where it
/// failed.
///
/// Measured on `main` before this change, `ic9700-idle-hot.wav` at `mean_sq` 0.0158:
///
/// | lead | decode | settle condemnations |
/// |---|---|---|
/// | 40 000 | **FAIL** | 92 |
/// | 80 000 | **FAIL** | 87 |
/// | 120 000 | OK | 6 |
///
/// (#1045's `condemned_floor` removal took those to OK/OK/OK, but only by un-sabotaging the
/// energy-only path — the receiver was still settling on noise hundreds of times per run. What this
/// pins is the settles being *refused*.)
#[test]
fn the_qpsk_receiver_never_settles_on_a_saturating_noise_floor() {
    let hot = corpus("ic9700-idle-hot.wav");
    // Guard the premise: without a saturating floor this is an ordinary decode and proves nothing.
    assert!(
        hot.mean_sq() > 0.0032,
        "corpus floor {:.4} no longer saturates the gate — this test's premise is gone",
        hot.mean_sq()
    );

    for lead in [40_000usize, 80_000] {
        let mut h = harness();
        h.tx_engine
            .transmit_with_fec_mode(b"qpsk correlation probe", "QPSK500", FecMode::Rs, None)
            .expect("transmit");
        h.route_embedded_in_capture(&hot, lead, 40_000, 0.3);

        let got = h
            .rx_engine
            .receive_with_fec_mode_timeout(
                "QPSK500",
                FecMode::Rs,
                None,
                Duration::from_millis(40_000),
            )
            .unwrap_or_else(|e| {
                panic!(
                    "lead {lead}: a coded QPSK frame must decode through a saturating floor: {e} \
                     (condemnations {}, rho rejections {})",
                    h.rx_engine.settle_condemnations(),
                    h.rx_engine.rho_rejected_settles()
                )
            });
        assert_eq!(String::from_utf8_lossy(&got), "qpsk correlation probe");

        // The tripwire, and the reason the decode above is evidence rather than luck: this floor
        // DOES pass the energy gate — that is what saturation means — so something must have refused
        // those candidates, and only the correlation check can have. A veto wired for BPSK alone
        // leaves this at zero while the decode still passes.
        assert!(
            h.rx_engine.rho_rejected_settles() > 0,
            "lead {lead}: no QPSK settle was ever refused on correlation, yet this floor saturates \
             the energy gate — the veto is not running on this mode"
        );
    }
}

/// The false-reject half: a real QPSK frame must still be acquired far off-frequency.
///
/// This is what fixes the *placement* of the check, and it is the half that a threshold chosen too
/// high destroys. A matched filter integrates coherently, so ρ against the template collapses with
/// carrier offset — running the check before the settle (as issue #1049 originally proposed) would
/// reject nearly every off-frequency frame. After the settle, QPSK500's measured post-settle ρ is
/// 1.000 / 1.000 / 0.994 / 0.989 at 0 / 100 / 200 / 400 Hz, so the veto sees a corrected carrier and
/// tests the *waveform*.
#[test]
fn an_off_frequency_qpsk_frame_is_still_settled_on() {
    for offset_hz in [0.0f32, 20.0, 200.0, 400.0] {
        let payload = b"off-frequency qpsk".to_vec();
        let mut h = harness();
        h.tx_engine.transmit(&payload, "QPSK500", None).expect("tx");
        h.route_with_cfo(offset_hz);
        let got = h
            .rx_engine
            .receive_with_timeout("QPSK500", None, Duration::from_millis(8_000))
            .unwrap_or_else(|e| {
                panic!(
                    "a real QPSK frame at {offset_hz} Hz offset must still be acquired: {e} \
                     ({} settles refused on correlation)",
                    h.rx_engine.rho_rejected_settles()
                )
            });
        assert_eq!(got, payload);
    }
}

/// Every published threshold clears its mode's recorded idle-noise ceiling.
///
/// The false-accept side of the derivation, re-measured against both idle captures through the
/// engine's own window and grid. This is the assertion BPSK's 0.40 would fail on QPSK500.
#[test]
fn every_qpsk_threshold_clears_its_modes_recorded_noise_ceiling() {
    // (mode, measured idle-noise ceiling over both captures, 2026-08-01)
    let measured = [
        ("QPSK125", 0.216f32),
        ("QPSK250", 0.291),
        ("QPSK500", 0.429),
    ];
    let captures = [corpus("ic9700-idle-hot.wav"), corpus("ft991a-idle.wav")];

    for (mode, recorded_ceiling) in measured {
        let t = QpskPlugin::new()
            .preamble_template(&cfg(mode))
            .unwrap_or_else(|| panic!("{mode} must publish a template"));
        let win = t.samples.len() / 15 * 17; // the engine's settle window: preamble + 1 symbol
        let step = t.samples.len() / 15;

        let mut peak = 0.0f32;
        for cap in &captures {
            let mut start = 0usize;
            while start + win <= cap.samples.len() {
                if let Some(r) = rho(mode, &cap.samples[start..start + win], None) {
                    peak = peak.max(r);
                }
                start += step;
            }
        }
        assert!(
            peak < t.rho_threshold,
            "{mode}: recorded idle noise reaches rho {peak:.3}, at or above its published \
             threshold {:.2} — the veto would corroborate settles on pure noise",
            t.rho_threshold
        );
        // Pin the measurement itself, so a drift in the corpus or the correlator shows up here
        // rather than silently eroding the margin the threshold was chosen from.
        assert!(
            (peak - recorded_ceiling).abs() < 0.05,
            "{mode}: idle-noise ceiling measured {peak:.3}, recorded {recorded_ceiling:.3} — \
             re-derive the threshold table before trusting it"
        );
    }
}

/// The counter-assertion that makes the table above mean something: the modes that publish NOTHING
/// are the ones with no room, not an oversight.
///
/// QPSK1000's template is 120 samples, and 120 samples of recorded idle reach ρ ≈ 0.58 while its
/// weakest decodable frame measures 0.879 — 1.23× on each side of the geometric mean, with the
/// decode column measured on AWGN (a fade lowers it) and the noise column on one band (a hotter one
/// raises it). There is no threshold to place there, so it keeps the energy-only settle.
#[test]
fn qpsk_publishes_no_template_where_the_margin_does_not_exist() {
    let p = QpskPlugin::new();
    for mode in ["QPSK1000", "QPSK2000", "QPSK9600"] {
        assert!(
            p.preamble_template(&cfg(mode)).is_none(),
            "{mode} publishes a template, but its template length leaves no gap between recorded \
             noise and the decode cliff"
        );
    }
    // -RRC and -HF shape the preamble differently and were never measured; claiming nothing is the
    // honest default. -RRC additionally breaks the "drop the last symbol" rule the crossfade
    // template relies on: the RRC pulse spans 12 symbols, so data smears back over the preamble tail.
    for mode in ["QPSK500-RRC", "QPSK1000-HF", "QPSK2000-RRC"] {
        assert!(
            p.preamble_template(&cfg(mode)).is_none(),
            "{mode} publishes a template, but its pulse shape was never measured"
        );
    }

    // The falsifier: QPSK1000's short template really is the reason, not an arbitrary baud cut.
    // Correlate the *would-be* 120-sample template against recorded idle and show it exceeds every
    // threshold in the table — i.e. no published value could have served it.
    let full = p.modulate(&[], &cfg("QPSK1000")).expect("modulate");
    let t: Vec<f32> = full[..8 * 15].to_vec();
    let mf = IqMatchedFilter::new(t);
    let grid = engine_grid(mf.len(), 20.0);
    let cap = corpus("ic9700-idle-hot.wav");
    let win = 8 * 17;
    let mut peak = 0.0f32;
    let mut start = 0usize;
    while start + win <= cap.samples.len() {
        if let Some((r, _)) = mf.search_normalized_over_frequency(
            &cap.samples[start..start + win],
            win - mf.len(),
            0.05,
            FS,
            &grid,
        ) {
            peak = peak.max(r.rho);
        }
        start += 8;
    }
    assert!(
        peak > 0.50,
        "a 120-sample QPSK1000 template reaches only rho {peak:.3} on recorded idle noise — if it \
         is that quiet, the mode should have a template and the exclusion above is unjustified"
    );
}

/// A steady tone must not pass the veto — with the grid-width falsifier that sizes the search.
///
/// **QPSK's tone behaviour is the opposite of BPSK's, and both facts are load-bearing.** BPSK's
/// alternating preamble is a square-wave-modulated carrier with lines at `fc ± baud/2`: a tone
/// cancels between the +1 and −1 half-symbols (ρ 0.017 at ±20 Hz) until the grid can rotate a line
/// onto plain carrier, and then it does not (0.659 at ±160 Hz). QPSK's designed sequence is
/// *aperiodic*, so a tone finds a partial alignment at any grid width and its ρ barely moves —
/// measured across QPSK125/250/500 from ±20 Hz to ±450 Hz: 0.014→0.524, 0.316→0.524, 0.506→0.524.
///
/// So for QPSK250 the widen-the-grid falsifier still fires (0.316 < 0.45 < 0.524) and is asserted
/// here. For QPSK500 no grid width pushes a tone past its 0.60 threshold — which is why the
/// threshold is derived from *noise* and the decode cliff, not from tone rejection. A birdie that
/// passes the veto degrades to the pre-#1049 behaviour (settle → decode fails → condemnation
/// recovery); a threshold that rejects decodable frames does not degrade, it breaks.
#[test]
fn a_steady_tone_does_not_pass_the_qpsk250_veto() {
    let t = QpskPlugin::new()
        .preamble_template(&cfg("QPSK250"))
        .expect("template");
    let win = 32 * 17;

    for f in [1_250.0f32, 1_400.0, 1_500.0, 1_600.0, 1_750.0] {
        let tone: Vec<f32> = (0..win)
            .map(|k| (2.0 * std::f32::consts::PI * f * k as f32 / FS).cos())
            .collect();
        let r = rho("QPSK250", &tone, None).expect("search");
        assert!(
            r < t.rho_threshold,
            "a pure {f} Hz tone scores rho {r:.3}, at or above QPSK250's threshold {:.2}",
            t.rho_threshold
        );
    }

    // The falsifier: widening the grid MUST break it, or the assertions above are passing for some
    // unrelated reason and say nothing about the grid width.
    let tone: Vec<f32> = (0..win)
        .map(|k| (2.0 * std::f32::consts::PI * 1_500.0 * k as f32 / FS).cos())
        .collect();
    let wide = rho("QPSK250", &tone, Some(450.0)).expect("search");
    assert!(
        wide > t.rho_threshold,
        "a +-450 Hz grid left a pure tone at rho {wide:.3}; the narrow-grid assertions above are \
         then not testing the grid width at all"
    );
}

/// Every published template must be the front of a real frame in that mode, and only that mode.
///
/// A hand-copied or mis-spanned template drifts out of step with the modulator the first time
/// either changes, and a template that no longer matches the wire stops corroborating settles
/// *silently* — it does not fail, it just never detects a frame again.
#[test]
fn every_qpsk_template_matches_the_front_of_a_real_frame() {
    let p = QpskPlugin::new();
    for (mode, sps) in [("QPSK125", 64usize), ("QPSK250", 32), ("QPSK500", 16)] {
        let t = p
            .preamble_template(&cfg(mode))
            .unwrap_or_else(|| panic!("{mode} must publish a template"));
        // 15 of the 16 preamble symbols: the last is dropped because the crossfade pulse blends a
        // third of the first DATA symbol into it.
        assert_eq!(t.samples.len(), 15 * sps, "{mode} template span");

        let frame = p.modulate(b"payload does not matter", &cfg(mode)).unwrap();
        let mf = IqMatchedFilter::new(t.samples);
        let r = mf
            .search_normalized(&frame, 2_000, 0.05)
            .expect("the template must be findable in the frame it was built from");
        assert!(
            r.rho > 0.95,
            "{mode}: template correlates at only rho {:.3} against a real frame — it is not the \
             same waveform the modulator emits",
            r.rho
        );
        assert!(
            r.offset < sps,
            "{mode}: template found at offset {} rather than the start of the frame",
            r.offset
        );

        // The falsifier: a different mode's frame must not correlate, or an all-zero or degenerate
        // template would pass the assertion above.
        let other_mode = if mode == "QPSK500" {
            "QPSK250"
        } else {
            "QPSK500"
        };
        let other = p.modulate(b"different waveform", &cfg(other_mode)).unwrap();
        let r2 = mf.search_normalized(&other, 2_000, 0.05).expect("search");
        assert!(
            r2.rho < 0.7,
            "{mode}: template correlates at rho {:.3} against a {other_mode} frame — it is not \
             discriminating between waveforms",
            r2.rho
        );
    }
}

/// The differential rung the HF ladder actually uses (`hpx_hf` SL6) is covered.
///
/// `-D` changes only the *data* encoding — the preamble symbols and pulse are identical — so its
/// template is the same samples as the coherent mode's and its threshold is the same number. That
/// is worth asserting rather than assuming: the differential decoder references the last preamble
/// symbol, so a preamble change has to move both ends together, and a template that silently
/// diverged between the two would leave SL6 unprotected.
#[test]
fn the_differential_rung_shares_the_coherent_template() {
    let p = QpskPlugin::new();
    for (coherent, differential) in [("QPSK250", "QPSK250-D"), ("QPSK500", "QPSK500-D")] {
        let a = p.preamble_template(&cfg(coherent)).expect("coherent");
        let b = p.preamble_template(&cfg(differential)).unwrap_or_else(|| {
            panic!("{differential} must publish a template — it is an hpx_hf rung")
        });
        assert_eq!(a, b, "{differential} template diverged from {coherent}");

        // And it must still be the front of a *differential* frame, which is the thing that could
        // silently change independently.
        let frame = p
            .modulate(b"differential payload", &cfg(differential))
            .unwrap();
        let mf = IqMatchedFilter::new(b.samples);
        let r = mf.search_normalized(&frame, 2_000, 0.05).expect("search");
        assert!(
            r.rho > 0.95,
            "{differential}: template correlates at only rho {:.3} against a real -D frame",
            r.rho
        );
    }
}
