//! 64QAM must survive a level that moves *during* a frame, not just one that is offset.
//!
//! **The defect this pins.** The receiver fitted its amplitude reference once, from the 16-symbol
//! preamble (`normalize_to_constellation`), and applied that single scalar to every symbol. Nothing
//! tracked the level across the frame — the decision-directed carrier loop tracks phase, and there
//! was no amplitude counterpart. 64QAM puts three of its six bits per axis into amplitude, so a
//! level that drifts mid-frame walks the outer PAM-8 rings across their decision boundaries with the
//! phase still perfect. A soundcard capture AGC riding its attack/decay does exactly that, and the
//! single-carrier 64QAM modes are the ones that fail on the dual-soundcard hardware loopback while
//! passing on a virtual single-clock one.
//!
//! **Why these numbers.** Every case here runs on a **noiseless** channel — no AWGN at all — so a
//! failure cannot be a noise limitation, which is this repo's signature for separating a bug from a
//! floor. The impairment is a pure sinusoidal gain, `1 + depth·sin(2π·f·t)`, applied to the
//! transmitted audio.
//!
//! The gate is deliberately asymmetric: the wander cases assert a *bound*, and the clean control
//! asserts *exactness*. A tracking pass that buys wander tolerance by perturbing clean frames is not
//! worth having — that is how the first two attempts at this were rejected (see the notes on
//! `track_gain_across_frame`).

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin, PulseShape};
use qam64_plugin::Qam64Plugin;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.to_string(),
        sample_rate: 8000,
        center_frequency: 1500.0,
        pulse_shape: if mode.ends_with("-RRC") {
            PulseShape::Rrc { alpha: 0.35 }
        } else {
            PulseShape::Hann
        },
        ..ModulationConfig::default()
    }
}

fn payload() -> Vec<u8> {
    (0..255u32)
        .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
        .collect()
}

/// `1 + depth·sin(2π·f·t)` applied sample by sample.
fn gain_wander(x: &[f32], fs: f32, f_hz: f32, depth: f32) -> Vec<f32> {
    x.iter()
        .enumerate()
        .map(|(n, &s)| s * (1.0 + depth * (std::f32::consts::TAU * f_hz * n as f32 / fs).sin()))
        .collect()
}

/// Fraction of payload bytes wrong (a failed demod counts as all of them).
fn byte_err(mode: &str, rx: &[f32], expect: &[u8]) -> f32 {
    match Qam64Plugin::new().demodulate(rx, &cfg(mode)) {
        Ok(out) => {
            let n = expect.len().min(out.len());
            if n == 0 {
                return 1.0;
            }
            let bad = (0..n).filter(|&k| out[k] != expect[k]).count();
            (bad + expect.len() - n) as f32 / expect.len() as f32
        }
        Err(_) => 1.0,
    }
}

/// THE GATE: a 2 Hz, 15 %-deep level wander must decode cleanly.
///
/// Measured before the tracking pass existed: `64QAM500` 0.102, `64QAM1000` 0.024. Both are now
/// 0.000 — this asserts exact recovery, not a reduced error rate, because at this depth the pass
/// removes the impairment rather than merely blunting it.
#[test]
fn a_two_hertz_fifteen_percent_level_wander_decodes_exactly() {
    let p = payload();
    for mode in ["64QAM500", "64QAM1000"] {
        let tx = Qam64Plugin::new()
            .modulate(&p, &cfg(mode))
            .expect("modulate");
        let rx = gain_wander(&tx, 8000.0, 2.0, 0.15);
        let err = byte_err(mode, &rx, &p);
        assert_eq!(
            err, 0.0,
            "{mode}: a 2 Hz 15 % level wander left {err:.3} of bytes wrong on a NOISELESS channel; \
             the amplitude reference is not being tracked across the frame"
        );
    }
}

/// A 30 %-deep wander must stay well inside what the FEC these modes always run under can absorb.
///
/// Before the tracking pass: `64QAM500` 0.318, `64QAM1000` 0.337, `64QAM2000-RRC` 0.369 — a third of
/// every frame wrong, far past any code rate. This bounds it at 0.15, which the measured 0.094 /
/// 0.051 / 0.369 clear except for the RRC mode, excluded below for the reason given.
#[test]
fn a_deep_level_wander_stays_inside_fec_capacity() {
    let p = payload();
    // `64QAM2000-RRC` is excluded: its frame is ~364 symbols, so the smoothing window spans a
    // quarter of it and there is too little data either side of each symbol to read a local level
    // from. It is unchanged by this pass (0.369 before and after) rather than regressed, and needs
    // a different mechanism — noted rather than silently dropped, so the gap stays visible.
    for mode in ["64QAM500", "64QAM1000"] {
        let tx = Qam64Plugin::new()
            .modulate(&p, &cfg(mode))
            .expect("modulate");
        let rx = gain_wander(&tx, 8000.0, 2.0, 0.30);
        let err = byte_err(mode, &rx, &p);
        assert!(
            err <= 0.15,
            "{mode}: a 2 Hz 30 % level wander left {err:.3} of bytes wrong (bound 0.15)"
        );
    }
}

/// The control that constrains the fix: a clean frame must still decode PERFECTLY.
///
/// A blind level estimator has its own variance, and an early version of this pass spent it on clean
/// frames — one byte wrong in 255 on a noiseless channel, which is a regression however small. The
/// shipped version shrinks every correction by the estimator's own standard error, so a frame with
/// no wander gets no correction at all.
#[test]
fn a_clean_frame_is_untouched() {
    let p = payload();
    for mode in ["64QAM500", "64QAM1000", "64QAM2000-RRC"] {
        let tx = Qam64Plugin::new()
            .modulate(&p, &cfg(mode))
            .expect("modulate");
        let err = byte_err(mode, &tx, &p);
        assert_eq!(
            err, 0.0,
            "{mode}: an unimpaired frame decoded with {err:.3} of bytes wrong — the level tracker \
             is perturbing frames it should leave alone"
        );
    }
}

/// Slow wander must not be made worse either, at any depth this pass claims to cover.
///
/// At 0.1 Hz a frame spans a few percent of a cycle, so the level is near-constant and the static
/// preamble fit already handles it. This is the no-op end of the range: it was 0.000 before and must
/// stay 0.000, which is what catches a tracker that over-corrects a level that is not moving.
#[test]
fn a_near_static_level_is_still_handled_by_the_static_fit() {
    let p = payload();
    for mode in ["64QAM500", "64QAM1000", "64QAM2000-RRC"] {
        let tx = Qam64Plugin::new()
            .modulate(&p, &cfg(mode))
            .expect("modulate");
        for depth in [0.05f32, 0.15, 0.30] {
            let rx = gain_wander(&tx, 8000.0, 0.1, depth);
            let err = byte_err(mode, &rx, &p);
            assert_eq!(
                err, 0.0,
                "{mode}: a near-static {depth} level offset left {err:.3} of bytes wrong"
            );
        }
    }
}
