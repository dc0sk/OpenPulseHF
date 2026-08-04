//! The preamble parameterisation must not move a single shipped bit.
//!
//! Before #1062's vetting work the preamble sequence was written out three times
//! — the Hann/RRC modulator, the GPU modulator, and the demodulator's expected
//! table — with no shared constant, so a wire-format edit could silently
//! desynchronise one end from the other (the playbook's one-sided rebuild, which
//! fails with "invalid magic" rather than a compile error).
//!
//! These tests pin that the parameterised seam, fed the shipped sequence,
//! reproduces the shipped path exactly. That is what makes the vetting harness
//! fidelity-by-construction instead of fidelity-by-comment (verification rule 5).

use bpsk_plugin::demodulate::{
    bpsk_demodulate, bpsk_demodulate_with_expected, expected_preamble_symbols, expected_symbols_for,
};
use bpsk_plugin::modulate::{
    bpsk_modulate, bpsk_modulate_with_preamble, preamble_bits, PREAMBLE_SYMS,
};
use openpulse_core::plugin::{ModulationConfig, PulseShape};

fn config(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.to_string(),
        sample_rate: 8000,
        center_frequency: 1500.0,
        pulse_shape: PulseShape::Hann,
        ..Default::default()
    }
}

const MODES: [&str; 4] = ["BPSK31", "BPSK63", "BPSK100", "BPSK250"];

#[test]
fn the_shipped_preamble_bits_are_pinned_against_an_independent_transcription() {
    // A golden value, deliberately written out rather than derived: comparing
    // `preamble_bits` against itself proves nothing, and every ρ threshold, grid
    // half-width and line-spacing claim in #1049/#1053/#1062 is scoped to *this*
    // sequence. If it changes, those constants are invalidated and this fails.
    let pinned: Vec<bool> = [
        true, false, true, false, true, false, true, false, true, false, true, false, true, false,
        true, false, true, false, true, false, true, false, true, false, true, false, true, false,
        true, false, true, false,
    ]
    .to_vec();
    assert_eq!(pinned.len(), PREAMBLE_SYMS);
    assert_eq!(
        preamble_bits(PREAMBLE_SYMS),
        pinned,
        "the shipped preamble sequence changed — every ρ constant derived for it must be re-derived"
    );
}

#[test]
fn the_preamble_parameter_actually_reaches_the_wire() {
    // Anti-vacuity tripwire. A seam that accepted the parameter and ignored it
    // would satisfy every other test in this file, because they all feed it the
    // shipped sequence. This one feeds a different sequence and requires the
    // transmitted samples to differ.
    let data = b"OPENPULSE parity seam";
    let mut altered = preamble_bits(PREAMBLE_SYMS);
    altered[1] = !altered[1];
    for mode in MODES {
        let cfg = config(mode);
        let shipped = bpsk_modulate(data, &cfg).expect("shipped modulate");
        let changed = bpsk_modulate_with_preamble(data, &cfg, &altered).expect("altered modulate");
        assert_eq!(
            shipped.len(),
            changed.len(),
            "{mode}: a same-length preamble changed the frame length"
        );
        assert_ne!(
            shipped, changed,
            "{mode}: the preamble parameter is being ignored — the seam is inert"
        );
    }
}

#[test]
fn the_parameterised_demodulator_reproduces_the_shipped_decode_exactly() {
    let data = b"OPENPULSE parity seam";
    for mode in MODES {
        let cfg = config(mode);
        let tx = bpsk_modulate(data, &cfg).expect("modulate");
        let shipped = bpsk_demodulate(&tx, &cfg).expect("shipped demodulate");
        let seamed =
            bpsk_demodulate_with_expected(&tx, &cfg, &expected_preamble_symbols(PREAMBLE_SYMS))
                .expect("seamed demodulate");
        assert_eq!(
            shipped, seamed,
            "{mode}: parameterised demodulator changed the decoded bytes"
        );
        assert_eq!(
            &shipped[..data.len()],
            data,
            "{mode}: round-trip did not recover the payload"
        );
    }
}

#[test]
fn the_expectation_parameter_actually_reaches_the_timing_lock() {
    // The RX-side anti-vacuity tripwire, and the reason the AFC column had to be
    // redesigned: stage-2 AFC and the decoder both lock timing by correlating
    // against the expectation, so feeding candidate audio through the *shipped*
    // entry point measures a TX/RX mismatch rather than the candidate. If a
    // deliberately wrong expectation decoded identically, that coupling would be
    // absent and the redesign unnecessary.
    let data = b"OPENPULSE parity seam";
    // The corruption has to be near-orthogonal to the shipped sequence. Measured
    // while writing this: flipping every third symbol (correlation ~1/3) still
    // locks to the correct offset, and a full inversion is invariant by design
    // (the metric is magnitude, handling the 180 degree polarity ambiguity). Only
    // an uncorrelated expectation moves the lock — which is itself evidence that
    // the sub-symbol timing metric tolerates partial mismatch.
    let wrong: Vec<f32> = (0..PREAMBLE_SYMS)
        .map(|i| {
            if i.wrapping_mul(2_654_435_761) % 2 == 0 {
                1.0
            } else {
                -1.0
            }
        })
        .collect();
    let cfg = config("BPSK250");
    let tx = bpsk_modulate(data, &cfg).expect("modulate");
    let right = bpsk_demodulate(&tx, &cfg).expect("shipped demodulate");
    let under_wrong = bpsk_demodulate_with_expected(&tx, &cfg, &wrong);
    let differs = match under_wrong {
        Ok(bytes) => bytes != right,
        Err(_) => true,
    };
    assert!(
        differs,
        "a corrupted expectation decoded identically — the expectation parameter is inert"
    );
}

#[test]
fn the_demodulator_expectation_derives_from_the_modulator_sequence() {
    // The former third copy. If someone edits `preamble_bits` and the RX table
    // does not follow, this fails at compile-time-adjacent speed rather than as
    // an on-air "invalid magic".
    for len in [8usize, 16, PREAMBLE_SYMS, 64] {
        assert_eq!(
            expected_preamble_symbols(len),
            expected_symbols_for(&preamble_bits(len)),
            "expected-symbol table diverged from the modulator's preamble at len {len}"
        );
    }
}

#[test]
fn the_shipped_preamble_is_still_the_period_four_alternation() {
    // Guards the record every #1049/#1053/#1062 constant rests on: the bits
    // alternate, and NRZI turns them into `--++` repeating (period 4), which is
    // why the lines sit at baud/4 rather than baud/2.
    let syms = expected_preamble_symbols(PREAMBLE_SYMS);
    assert_eq!(syms.len(), PREAMBLE_SYMS);
    for (i, w) in syms.chunks_exact(4).enumerate() {
        assert_eq!(
            w,
            &[-1.0, -1.0, 1.0, 1.0],
            "preamble period-4 structure broken at symbol group {i}"
        );
    }
}
