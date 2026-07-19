//! Sample-rate-offset (clock-drift) confirmation experiment.
//!
//! The virtual loopback (single clock) decodes SCFDMA52 / 64QAM fine, but the
//! dual-clock hardware rig fails 0/8. This sweeps a controlled sample-rate offset
//! (the one effect a single-clock rig cannot reproduce) to test whether SRO alone
//! reproduces the failure — i.e. whether the hardware fault is clock offset vs.
//! analog phase distortion.
//!
//! Run with output:
//!   cargo test -p openpulse-modem --no-default-features --test sro_confirmation -- --nocapture

use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use ofdm_plugin::OfdmPlugin;
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;
use psk8_plugin::Psk8Plugin;
use qam64_plugin::Qam64Plugin;
use qpsk_plugin::QpskPlugin;
use scfdma_plugin::ScFdmaPlugin;

fn harness_all() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        eng.register_plugin(Box::new(QpskPlugin::new())).unwrap();
        eng.register_plugin(Box::new(Psk8Plugin::new())).unwrap();
        eng.register_plugin(Box::new(Qam64Plugin::new())).unwrap();
        eng.register_plugin(Box::new(OfdmPlugin::new())).unwrap();
        eng.register_plugin(Box::new(ScFdmaPlugin::new())).unwrap();
    }
    h
}

/// Returns true if `mode` decodes the payload after a `ppm` sample-rate offset.
fn decodes_at(mode: &str, ppm: f32, payload: &[u8]) -> bool {
    let mut h = harness_all();
    if h.tx_engine.transmit(payload, mode, None).is_err() {
        return false;
    }
    if ppm == 0.0 {
        h.route_clean();
    } else {
        h.route_with_sro(ppm);
    }
    matches!(h.rx_engine.receive(mode, None), Ok(rx) if rx == payload)
}

/// As [`decodes_at`] but with a FEC codec, reflecting how the dense modes actually
/// run (they ride a FEC-protected profile). Uses the timeout-scanning FEC receive.
fn decodes_at_fec(mode: &str, ppm: f32, payload: &[u8], fec: FecMode) -> bool {
    let mut h = harness_all();
    if h.tx_engine
        .transmit_with_fec_mode(payload, mode, fec, None)
        .is_err()
    {
        return false;
    }
    if ppm == 0.0 {
        h.route_clean();
    } else {
        h.route_with_sro(ppm);
    }
    matches!(
        h.rx_engine
            .receive_with_fec_mode_timeout(mode, fec, None, Duration::from_millis(4000)),
        Ok(rx) if rx == payload
    )
}

#[test]
fn sro_sweep_matrix() {
    let payload: Vec<u8> = (0..64)
        .map(|i| b"OpenPulseHF-SRO-confirm-"[i % 24])
        .collect();
    // 0 is the control; a few-hundred ppm is the realistic two-soundcard range.
    let ppms = [0.0f32, 25.0, 50.0, 100.0, 200.0, 500.0];
    let modes = [
        "BPSK250",  // single-carrier control
        "QPSK500",  // single-carrier control
        "SCFDMA16", // narrowband multicarrier (passes hardware)
        "SCFDMA52", // wideband multicarrier (fails hardware) — the suspect
        "OFDM52",   // wideband multicarrier
        "64QAM500", // dense single-carrier (fails hardware)
    ];

    println!("\nSRO confirmation matrix (decode ok = '.', fail = 'X'):");
    print!("{:<12}", "mode \\ ppm");
    for p in ppms {
        print!("{:>7}", p as i32);
    }
    println!();
    for mode in modes {
        print!("{mode:<12}");
        for &ppm in &ppms {
            print!(
                "{:>7}",
                if decodes_at(mode, ppm, &payload) {
                    "."
                } else {
                    "X"
                }
            );
        }
        println!();
    }
    println!();

    // Sanity: every mode must decode on the clean (0 ppm) control path.
    for mode in modes {
        assert!(
            decodes_at(mode, 0.0, &payload),
            "{mode} failed the clean control case"
        );
    }

    // SRO-robust modes (single-carrier + cyclic-prefix OFDM) must tolerate 100 ppm,
    // a realistic two-soundcard offset. Guards against regressions in modes that work.
    for mode in ["BPSK250", "QPSK500", "OFDM52"] {
        assert!(
            decodes_at(mode, 100.0, &payload),
            "{mode} should tolerate 100 ppm SRO"
        );
    }
}

/// SC-FDMA sync tracking under sample-rate offset (per-symbol pilot deramp).
/// Before the fix SCFDMA52 failed at ≥200 ppm; it now matches OFDM's robustness.
#[test]
fn scfdma52_tolerates_realistic_sro() {
    let payload: Vec<u8> = (0..64)
        .map(|i| b"OpenPulseHF-SRO-target--"[i % 24])
        .collect();
    assert!(
        decodes_at("SCFDMA52", 200.0, &payload),
        "SCFDMA52 @ 200 ppm"
    );
    assert!(
        decodes_at("SCFDMA52", 500.0, &payload),
        "SCFDMA52 @ 500 ppm"
    );
}

/// 64QAM (dense single-carrier) under a realistic sample-rate offset.
///
/// Two-pass DD carrier tracking cuts the raw byte-error rate at 100 ppm from ~6.2 %
/// to ~2.1 % — within soft-FEC capacity — so with the soft code these dense modes
/// run under, 64QAM decodes at a realistic two-clock offset. (Bare 64QAM is still
/// SNR/eye-marginal at 100 ppm no-FEC, expected for a 64-point grid; full no-FEC
/// closure is deferred — it is sim-only validatable and not a v1.0 blocker.)
#[test]
fn qam64_tolerates_realistic_sro() {
    let payload: Vec<u8> = (0..64)
        .map(|i| b"OpenPulseHF-SRO-target--64QAM-fc"[i % 31])
        .collect();
    assert!(
        decodes_at_fec("64QAM500", 100.0, &payload, FecMode::SoftConcatenated),
        "64QAM500 @ 100 ppm with soft-concatenated FEC"
    );
}

/// Does SRO alone reproduce the hardware failure of `QPSK250 + rs`?
///
/// The dual-card rig fails `QPSK250 + rs` (and `QPSK250-D + rs`) while passing uncoded `QPSK250` on
/// the same cable. "Clock offset over a long frame" was the leading hypothesis. This tests it: the
/// coded frame is a padded 255-byte block (4.08 s at QPSK250), the uncoded one is 74 B (1.18 s).
///
/// Arithmetic says the hypothesis is weak — at 100 ppm the drift across the whole 4.08 s frame is
/// 0.10 symbol periods, and a full symbol slip needs ~980 ppm. Run it rather than argue.
#[test]
#[ignore = "diagnostic experiment for the 2026-07-19 hardware failure; run with --ignored --nocapture"]
fn does_sro_alone_break_a_long_coded_qpsk_frame() {
    let payload: Vec<u8> = (0..64u8).collect();
    println!("\nQPSK250 + rs (255 B wire, 4.08 s) vs sample-rate offset:");
    for ppm in [0.0f32, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0] {
        let ok = decodes_at_fec("QPSK250", ppm, &payload, FecMode::Rs);
        println!("  {ppm:7.0} ppm : {}", if ok { "decode" } else { "FAIL" });
    }
    println!("\nControl — uncoded QPSK250 (74 B wire, 1.18 s), which PASSES on the rig:");
    for ppm in [0.0f32, 100.0, 500.0, 1000.0, 2000.0] {
        let ok = decodes_at("QPSK250", ppm, &payload);
        println!("  {ppm:7.0} ppm : {}", if ok { "decode" } else { "FAIL" });
    }

    // QPSK250-D was named in this file's original doc comment but never actually exercised — the
    // body only ever passed "QPSK250". A test that claims coverage it does not have is worse than
    // no test, so the differential rung is swept here explicitly. It is the mode that still fails
    // on hardware even with a tight capture window.
    println!("\nQPSK250-D + rs — the differential rung that fails on hardware:");
    for ppm in [0.0f32, 100.0, 500.0, 1000.0, 2000.0] {
        let ok = decodes_at_fec("QPSK250-D", ppm, &payload, FecMode::Rs);
        println!("  {ppm:7.0} ppm : {}", if ok { "decode" } else { "FAIL" });
    }
}
