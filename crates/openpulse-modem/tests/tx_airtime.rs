//! `ModemEngine::tx_airtime_seconds` must equal what the engine actually emits (#1299).
//!
//! A caller that keys the transmitter itself has to know a frame's airtime BEFORE it keys, because
//! a single legitimate frame can outlast the PTT watchdog (BPSK31 + `Concatenated` is of order
//! 265 s against a 180 s `DEFAULT_PTT_MAX`) and being force-released mid-frame leaves the engine
//! writing audio into an unkeyed rig. The CLI refuses such an emission instead of starting it.
//!
//! The predictor mirrors `transmit_with_fec_mode`'s dispatch arm for arm. A doc comment cannot keep
//! two `match`es in step — and this repo bans that construct — so this asserts the equality against
//! the REAL transmit path, for every `FecMode`, in the default test run.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_modem::ModemEngine;

const MODE: &str = "BPSK250";

/// Every `FecMode` the transmit dispatch accepts. Listed so a new variant fails to compile here
/// rather than silently going unmeasured — the arm-for-arm mirror is the whole point.
fn every_fec_mode() -> Vec<FecMode> {
    vec![
        FecMode::None,
        FecMode::Rs,
        FecMode::RsStrong,
        FecMode::RsInterleaved,
        FecMode::Concatenated,
        FecMode::SoftConcatenated,
        FecMode::ShortRs,
        FecMode::Ldpc,
        FecMode::LdpcHighRate,
        FecMode::Turbo,
    ]
}

fn engine_on(lb: &LoopbackBackend) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    e
}

#[test]
fn tx_airtime_matches_the_emitted_frame() {
    // Small enough for Turbo's and ShortRs's one-block limits, so every arm is exercised rather
    // than skipped — a mode list that quietly drops arms would make this gate partly vacuous.
    let payload = vec![0x5Au8; 32];

    for fec in every_fec_mode() {
        let lb = LoopbackBackend::new();
        let mut tx = engine_on(&lb);

        // Predict FIRST: the prediction is taken at the current sequence, which is the one the
        // transmit below will use.
        let predicted = tx
            .tx_airtime_seconds(&payload, MODE, fec)
            .unwrap_or_else(|e| panic!("{fec:?}: airtime prediction failed: {e}"));

        tx.transmit_with_fec_mode(&payload, MODE, fec, None)
            .unwrap_or_else(|e| panic!("{fec:?}: transmit failed: {e}"));
        let emitted = lb.drain_samples().len() as f64 / 8000.0;

        assert!(
            (predicted - emitted).abs() < 1e-9,
            "{fec:?}: predicted {predicted:.6} s but the engine emitted {emitted:.6} s — the \
             airtime predictor has drifted from transmit_with_fec_mode's dispatch"
        );
        assert!(
            emitted > 0.0,
            "{fec:?}: emitted nothing, so the check is vacuous"
        );
    }
}

#[test]
fn the_prediction_does_not_consume_a_sequence_number() {
    // `stage_encode_frame` bumps `self.sequence`; a measurement that did the same would desync the
    // wire. Proven by predicting repeatedly and requiring the next real frame to be byte-identical
    // to one sent with no prediction at all.
    let payload = vec![0x11u8; 16];

    let lb_a = LoopbackBackend::new();
    let mut a = engine_on(&lb_a);
    for _ in 0..5 {
        a.tx_airtime_seconds(&payload, MODE, FecMode::Rs).unwrap();
    }
    a.transmit_with_fec_mode(&payload, MODE, FecMode::Rs, None)
        .unwrap();
    let after_predictions = lb_a.drain_samples();

    let lb_b = LoopbackBackend::new();
    let mut b = engine_on(&lb_b);
    b.transmit_with_fec_mode(&payload, MODE, FecMode::Rs, None)
        .unwrap();
    let without_predictions = lb_b.drain_samples();

    assert_eq!(
        after_predictions, without_predictions,
        "five airtime predictions changed the next transmitted frame — the predictor is mutating \
         engine TX state (sequence number, scrambler, or similar)"
    );
}

#[test]
fn a_slow_rung_with_heavy_fec_really_does_outlast_the_ptt_watchdog() {
    // The premise the CLI's refuse-before-keying guard rests on. Asserted rather than quoted: the
    // figure it replaces (~265 s) was derived from `max_frame_samples * fec_slice_factor`, which is
    // the RECEIVE slice reserve and NOT the mechanism. Measured here, the real cause is the RS
    // block boundary — a 223 B payload plus `Frame::WIRE_OVERHEAD` needs a SECOND 255-byte block,
    // which doubles the airtime. At 200 B the same combination is 134 s and fits comfortably; the
    // cliff, not the FEC factor, is what puts it over.
    let lb = LoopbackBackend::new();
    let tx = engine_on(&lb);
    let watchdog = openpulse_radio::DEFAULT_PTT_MAX.as_secs_f64();

    // Measured 2026-09-14 on BPSK31 at the payload that maximises each: Concatenated 264.7 s and
    // SoftConcatenated 265.0 s at 223 B, Ldpc 197.9 s and Turbo 296.2 s at 250 B.
    for (fec, len) in [
        (FecMode::Concatenated, 223usize),
        (FecMode::SoftConcatenated, 223),
        (FecMode::Ldpc, 250),
        (FecMode::Turbo, 250),
    ] {
        let seconds = tx
            .tx_airtime_seconds(&vec![0x5Au8; len], "BPSK31", fec)
            .unwrap_or_else(|e| panic!("BPSK31+{fec:?} at {len} B: {e}"));
        assert!(
            seconds > watchdog,
            "BPSK31+{fec:?} at {len} B is {seconds:.1} s, no longer over the {watchdog:.0} s \
             watchdog — the CLI's refusal would be unreachable for it and its test vacuous"
        );
    }

    // Controls, both directions.
    //
    // (a) The FEC the ladder actually assigns this rung fits, so the refusal is specific to the
    //     heavy-FEC combinations rather than to slow modes in general.
    let with_rs = tx
        .tx_airtime_seconds(&vec![0x5Au8; 223], "BPSK31", FecMode::Rs)
        .expect("BPSK31+Rs airtime");
    assert!(
        with_rs < watchdog,
        "BPSK31+Rs at 223 B is {with_rs:.1} s, over the {watchdog:.0} s watchdog — the ladder's own \
         slowest rung would be refused, which is not what this guard is for"
    );

    // (b) The SAME heavy FEC one block lower is under the deadline, which pins the mechanism as the
    //     RS block boundary. If this ever goes over too, the story above is wrong.
    let below_cliff = tx
        .tx_airtime_seconds(&[0x5Au8; 200], "BPSK31", FecMode::Concatenated)
        .expect("BPSK31+Concatenated at 200 B");
    assert!(
        below_cliff < watchdog,
        "BPSK31+Concatenated at 200 B is {below_cliff:.1} s, already over the watchdog — then the \
         second-RS-block explanation for the 223 B figure is not the mechanism"
    );
}
