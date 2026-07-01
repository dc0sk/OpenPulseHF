//! SNR-floor calibration harness — the "quick simulation run" that derives the working SNR/step
//! pairs the OTA fast-downshift jumps to (`SessionProfile::snr_floor_for_level`).
//!
//! For each single-carrier rung of `hpx_hf` it sweeps AWGN SNR and finds the lowest SNR at which the
//! FEC-protected mode decodes reliably — the empirical floor. Prints a table comparing the measured
//! floor against the profile's configured floor, so the constants in `profile.rs` can be validated
//! or retuned from data instead of guesswork.
//!
//! Ignored by default (it runs a full modulate→AWGN→demodulate sweep); run on demand:
//!
//! ```text
//! cargo test -p openpulse-modem --no-default-features --test snr_floor_calibration -- --ignored --nocapture
//! ```

use bpsk_plugin::BpskPlugin;
use openpulse_channel::{awgn::AwgnChannel, AwgnConfig};
use openpulse_core::profile::SessionProfile;
use openpulse_modem::channel_sim::ChannelSimHarness;
use psk8_plugin::Psk8Plugin;
use qpsk_plugin::QpskPlugin;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        eng.register_plugin(Box::new(QpskPlugin::new())).unwrap();
        eng.register_plugin(Box::new(Psk8Plugin::new())).unwrap();
    }
    h
}

/// Success rate of `frames` FEC-protected round-trips through AWGN at `snr_db`.
fn decode_rate(mode: &str, fec: openpulse_core::fec::FecMode, snr_db: f32, frames: u32) -> f32 {
    let payload = b"OTA SNR floor calibration payload, sixty-four bytes total AAAA";
    let mut ok = 0u32;
    for f in 0..frames {
        let mut h = harness();
        if h.tx_engine
            .transmit_with_fec_mode(payload, mode, fec, None)
            .is_err()
        {
            continue;
        }
        let mut ch = AwgnChannel::new(AwgnConfig {
            snr_db,
            seed: Some(1000 + f as u64),
        })
        .unwrap();
        h.route(&mut ch);
        if h.rx_engine
            .receive_with_fec_mode(mode, fec, None)
            .map(|d| d == payload)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    ok as f32 / frames as f32
}

#[test]
#[ignore = "calibration sweep; run manually with --ignored --nocapture"]
fn calibrate_snr_floors_hpx_hf_single_carrier() {
    const FRAMES: u32 = 16;
    const TARGET: f32 = 0.90; // "reliable" = 90% of frames decode
    let profile = SessionProfile::hpx_hf();

    println!(
        "\n=== hpx_hf SNR-floor calibration (AWGN, FEC, {FRAMES} frames, target {:.0}%) ===",
        TARGET * 100.0
    );
    println!("level mode           fec                 cfg_dB   meas_dB  verdict");

    for level in profile.defined_levels() {
        let Some(mode) = profile.mode_for(level) else {
            continue;
        };
        // Single-carrier ladder only — fast, and where the demo's low-SNR stepping matters.
        if mode.starts_with("SCFDMA") || mode.starts_with("OFDM") || mode.starts_with("PILOT") {
            continue;
        }
        let fec = profile.fec_for(level);
        // Sweep from well below to well above the configured floor.
        let mut measured: Option<f32> = None;
        let mut snr = -2.0f32;
        while snr <= 26.0 {
            if decode_rate(mode, fec, snr, FRAMES) >= TARGET {
                measured = Some(snr);
                break;
            }
            snr += 1.0;
        }
        let cfg = profile.snr_floor_for_level(level);
        let verdict = match (cfg, measured) {
            (Some(c), Some(m)) if m > c + 1.5 => "⚠ floor OPTIMISTIC (config below measured)",
            (Some(c), Some(m)) if m < c - 3.0 => "… floor conservative (headroom to lower)",
            (Some(_), Some(_)) => "ok",
            (None, Some(_)) => "no configured floor",
            (_, None) => "did not decode ≤26 dB",
        };
        println!(
            "{:<5?} {:<14} {:<16?} {:>9} {:>9}  {}",
            level,
            mode,
            fec,
            cfg.map(|c| format!("{c:.0}")).unwrap_or_else(|| "-".into()),
            measured
                .map(|m| format!("{m:.0}"))
                .unwrap_or_else(|| ">26".into()),
            verdict,
        );
    }
    println!("=== end calibration — copy meas_dB into SessionProfile::hpx_hf snr_floors if it drifts ===\n");
}
