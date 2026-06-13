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

use bpsk_plugin::BpskPlugin;
use ofdm_plugin::OfdmPlugin;
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

/// Target for the upcoming SC-FDMA / 64QAM sync-tracking work: these wideband and
/// dense modes currently fail at a realistic sample-rate offset where single-carrier
/// and cyclic-prefix OFDM modes do not. Marked `#[ignore]` because it fails today;
/// remove the attribute once SRO/pilot tracking lands and it passes.
#[test]
#[ignore = "red TDD target for SC-FDMA/64QAM SRO sync tracking; passes once the fix lands"]
fn scfdma52_and_64qam_tolerate_realistic_sro() {
    let payload: Vec<u8> = (0..64)
        .map(|i| b"OpenPulseHF-SRO-target--"[i % 24])
        .collect();
    assert!(
        decodes_at("SCFDMA52", 200.0, &payload),
        "SCFDMA52 @ 200 ppm"
    );
    assert!(
        decodes_at("64QAM500", 100.0, &payload),
        "64QAM500 @ 100 ppm"
    );
}
