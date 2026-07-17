//! The ladder must climb on evidence, not only on an SNR estimate — issue #934.
//!
//! The controller used to have exactly one upward path: `snr_db >= ceiling(rx_confirmed)`. That
//! makes the SNR estimate the *sole permission* to climb, which fails whenever the estimate is
//! uninformative — and on a fading channel it can be uninformative **in principle**: at 31 baud a
//! 1 Hz Doppler fade decorrelates in ~6 symbols, so no window is both short enough to track the
//! fade and long enough to average the noise, and the estimate reads a constant. `hpx_hf` on
//! Watterson `moderate_f1` therefore sat pinned on its entry rung at ~5 bps **while delivering
//! 20/20 frames** — the rungs it could have reached carry ~300–1200.
//!
//! These tests pin the contract: a rung that keeps decoding earns the next step even with a useless
//! SNR reading; a frame that decoded is never answered with a demotion (the observation beats the
//! model); and SNR keeps its fast-downshift on actual failures, where it explains something.

use openpulse_core::ota_rate::{OtaRateController, RxOutcome, ACK_CLIMB_THRESHOLD};
use openpulse_core::profile::SessionProfile;

/// An SNR reading that is *adequate for the entry rung but never clears its ceiling* — exactly the
/// flat, uninformative number a fading channel produces at low baud.
const FLAT_UNINFORMATIVE_SNR: f32 = 4.4;

fn controller() -> OtaRateController {
    OtaRateController::new(SessionProfile::hpx_hf())
}

#[test]
fn a_rung_that_keeps_decoding_climbs_without_snr_evidence() {
    let p = SessionProfile::hpx_hf();
    let start = p.initial_level;
    // The premise: this SNR must NOT clear the entry rung's ceiling, or the test proves nothing.
    let ceiling = p.snr_ceiling_for_level(start).expect("entry rung ceiling");
    assert!(
        FLAT_UNINFORMATIVE_SNR < ceiling,
        "premise broken: {FLAT_UNINFORMATIVE_SNR} already clears the SL ceiling {ceiling}, so an \
         SNR-only controller would climb anyway and this test would pass vacuously"
    );

    let mut c = controller();
    let mut recommended = start;
    for _ in 0..ACK_CLIMB_THRESHOLD {
        recommended = c
            .on_rx_frame(RxOutcome::Decoded(start), FLAT_UNINFORMATIVE_SNR)
            .recommended_level;
    }
    assert!(
        recommended > start,
        "after {ACK_CLIMB_THRESHOLD} clean decodes the rung has proven itself and must earn the \
         next step, even though the SNR estimate never cleared its ceiling ({FLAT_UNINFORMATIVE_SNR} \
         < {ceiling}). Staying at {start:?} is what pinned a fading link at ~5 bps while every frame \
         decoded (#934)."
    );
}

#[test]
fn one_clean_decode_is_not_enough_to_climb() {
    let start = SessionProfile::hpx_hf().initial_level;
    let mut c = controller();
    let ack = c.on_rx_frame(RxOutcome::Decoded(start), FLAT_UNINFORMATIVE_SNR);
    assert_eq!(
        ack.recommended_level, start,
        "a single decode is not evidence; the climb must need a streak or it is just optimism"
    );
}

#[test]
fn a_failure_restarts_the_streak() {
    let start = SessionProfile::hpx_hf().initial_level;
    let mut c = controller();
    // Alternating pass/fail must never accumulate into a climb.
    for _ in 0..6 {
        c.on_rx_frame(RxOutcome::Decoded(start), FLAT_UNINFORMATIVE_SNR);
        let ack = c.on_rx_frame(RxOutcome::Failed, FLAT_UNINFORMATIVE_SNR);
        assert!(
            ack.recommended_level <= start,
            "a flapping rung must not be promoted; got {:?}",
            ack.recommended_level
        );
    }
}

/// **A decode is an observation; the SNR is a model. The observation must win.**
///
/// The controller used to answer a frame that had *just decoded at a rung* with "drop below that
/// rung" whenever the SNR estimate disagreed. That is the other half of #934: on a fade BPSK31's
/// estimate reads far below every floor at any true SNR, so every successful frame was met with a
/// demotion, and the link oscillated on its bottom two rungs while delivering 20/20 frames.
#[test]
fn a_decoded_frame_is_never_answered_with_a_demotion() {
    let p = SessionProfile::hpx_hf();
    let mut c = controller();
    // Climb to a mid rung on good SNR.
    let mut level = p.initial_level;
    for _ in 0..40 {
        level = c
            .on_rx_frame(RxOutcome::Decoded(level), 30.0)
            .recommended_level;
    }
    assert!(
        level > p.initial_level,
        "setup: should have climbed on good SNR"
    );

    // Now feed an absurdly pessimistic SNR alongside frames that keep decoding at `level`.
    // The decodes are proof the rung works; the estimate must not override them.
    for _ in 0..(ACK_CLIMB_THRESHOLD as usize + 3) {
        let got = c
            .on_rx_frame(RxOutcome::Decoded(level), -20.0)
            .recommended_level;
        assert!(
            got >= level,
            "a frame decoded at {level:?} proves that rung works, but the controller recommended \
             {got:?} on a -20 dB estimate. Demotion belongs on the Failed path, where the SNR \
             actually explains something."
        );
    }
}

/// The counterpart: SNR keeps its fast-downshift where it earns its keep — on an actual failure,
/// where it explains what went wrong and can skip several rungs at once.
#[test]
fn a_failed_frame_still_fast_downshifts_on_snr() {
    let p = SessionProfile::hpx_hf();
    let mut c = controller();
    let mut level = p.initial_level;
    for _ in 0..40 {
        level = c
            .on_rx_frame(RxOutcome::Decoded(level), 30.0)
            .recommended_level;
    }
    assert!(
        level > p.initial_level,
        "setup: should have climbed on good SNR"
    );

    let after = c.on_rx_frame(RxOutcome::Failed, -5.0).recommended_level;
    assert!(
        after < level,
        "a failure WITH a sub-floor SNR must fast-downshift (the estimate explains the failure): \
         was {level:?}, recommended {after:?}"
    );
}

/// The lockstep invariant: the recommendation may never exceed `rx_confirmed` by more than one
/// mapped step, whatever the climb trigger. Violating it desyncs the two ends on a lost ACK.
#[test]
fn evidence_climb_advances_at_most_one_mapped_step() {
    let p = SessionProfile::hpx_hf();
    let levels = p.defined_levels();
    let start = p.initial_level;
    let mut c = controller();

    // Feed a long run of clean decodes that never advance `rx_confirmed` (the sender never adopts
    // the recommendation — e.g. every ACK is lost). The recommendation must stall one step up.
    let mut seen = start;
    for _ in 0..(ACK_CLIMB_THRESHOLD as usize * 8) {
        seen = c
            .on_rx_frame(RxOutcome::Decoded(start), FLAT_UNINFORMATIVE_SNR)
            .recommended_level;
        let confirmed_idx = levels
            .iter()
            .position(|&l| l == start)
            .expect("start mapped");
        let seen_idx = levels.iter().position(|&l| l == seen).expect("rec mapped");
        assert!(
            seen_idx <= confirmed_idx + 1,
            "lockstep broken: recommended {seen:?} is more than one mapped step above confirmed \
             {start:?} — a lost ACK could then desync the ends"
        );
    }
    assert!(
        seen > start,
        "sanity: the streak should still have proposed the next step"
    );
}
