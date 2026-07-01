//! SNR-floor calibration harness — the "quick simulation run" that derives the working SNR/step
//! pairs the OTA fast-downshift jumps to (`SessionProfile::snr_floor_for_level`).
//!
//! Sweeps AWGN SNR for each (mode, FEC) rung and finds the lowest SNR at which it decodes reliably —
//! the empirical floor. Prints a table so the constants in `profile.rs` can be validated or retuned
//! from data instead of guesswork. Two `#[ignore]` sweeps:
//!   * `calibrate_snr_floors_hpx_hf` — every hpx_hf rung with the FEC the profile assigns it.
//!   * `calibrate_candidate_fec_rungs` — candidate (mode, FEC) pairs under consideration.
//!
//! Run on demand (full modulate→AWGN→demodulate sweeps, so ignored by default):
//!
//! ```text
//! cargo test -p openpulse-modem --no-default-features --test snr_floor_calibration -- --ignored --nocapture
//! ```

use bpsk_plugin::BpskPlugin;
use openpulse_channel::{awgn::AwgnChannel, AwgnConfig};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_modem::channel_sim::ChannelSimHarness;
use psk8_plugin::Psk8Plugin;
use qpsk_plugin::QpskPlugin;
use scfdma_plugin::ScFdmaPlugin;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        eng.register_plugin(Box::new(QpskPlugin::new())).unwrap();
        eng.register_plugin(Box::new(Psk8Plugin::new())).unwrap();
        eng.register_plugin(Box::new(ScFdmaPlugin::new())).unwrap();
    }
    h
}

/// Success rate of `frames` FEC-protected round-trips through AWGN at `snr_db`.
fn decode_rate(mode: &str, fec: FecMode, snr_db: f32, frames: u32) -> f32 {
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

/// Lowest SNR in `[lo, hi]` dB (1 dB grid) at which `mode`+`fec` decodes ≥ `target`.
fn min_decodable_snr(
    mode: &str,
    fec: FecMode,
    lo: f32,
    hi: f32,
    frames: u32,
    target: f32,
) -> Option<f32> {
    let mut snr = lo;
    while snr <= hi {
        if decode_rate(mode, fec, snr, frames) >= target {
            return Some(snr);
        }
        snr += 1.0;
    }
    None
}

fn print_row(label: &str, mode: &str, fec: FecMode, cfg: Option<f32>, meas: Option<f32>) {
    let verdict = match (cfg, meas) {
        (Some(c), Some(m)) if m > c + 1.5 => "⚠ OPTIMISTIC (config below measured)",
        (Some(c), Some(m)) if m < c - 3.0 => "… conservative (headroom to lower)",
        (Some(_), Some(_)) => "ok",
        (None, Some(_)) => "(no configured floor)",
        (_, None) => "did not decode in range",
    };
    println!(
        "{label:<5} {mode:<16} {:<16?} {:>7} {:>8}  {verdict}",
        fec,
        cfg.map(|c| format!("{c:.0}")).unwrap_or_else(|| "-".into()),
        meas.map(|m| format!("{m:.0}"))
            .unwrap_or_else(|| "none".into()),
    );
}

#[test]
#[ignore = "calibration sweep; run manually with --ignored --nocapture"]
fn calibrate_snr_floors_hpx_hf() {
    const FRAMES: u32 = 16;
    const TARGET: f32 = 0.90;
    let profile = SessionProfile::hpx_hf();
    println!(
        "\n=== hpx_hf SNR-floor calibration (AWGN, {FRAMES} frames, target {:.0}%) ===",
        TARGET * 100.0
    );
    println!("level mode             fec               cfg_dB  meas_dB  verdict");
    for level in profile.defined_levels() {
        let Some(mode) = profile.mode_for(level) else {
            continue;
        };
        let fec = profile.fec_for(level);
        let meas = min_decodable_snr(mode, fec, -2.0, 34.0, FRAMES, TARGET);
        print_row(
            &format!("{level:?}"),
            mode,
            fec,
            profile.snr_floor_for_level(level),
            meas,
        );
    }
    println!("=== end hpx_hf calibration ===\n");
}

#[test]
#[ignore = "calibration sweep; run manually with --ignored --nocapture"]
fn calibrate_candidate_fec_rungs() {
    const FRAMES: u32 = 20;
    const TARGET: f32 = 0.90;
    // The FEC-protected hpx_hf upper ladder (mode, FEC) as assigned in `SessionProfile::hpx_hf`:
    // 8PSK500 gets *light* RS (keeps it a faster rung than QPSK500 while filling the 11→16 gap); the
    // dense SCFDMA rungs get soft-concatenated FEC (they only run FEC-protected). Re-run to re-derive
    // the floors if the DSP changes; cross-32QAM (SL10) AWGN-measures harder than 64QAM (SL11).
    let candidates = [
        ("8PSK500", FecMode::Rs),
        ("SCFDMA52-8PSK", FecMode::SoftConcatenated),
        ("SCFDMA52-16QAM", FecMode::SoftConcatenated),
        ("SCFDMA52-32QAM", FecMode::SoftConcatenated),
        ("SCFDMA52-64QAM", FecMode::SoftConcatenated),
    ];
    println!(
        "\n=== candidate FEC-rung calibration (AWGN, {FRAMES} frames, target {:.0}%) ===",
        TARGET * 100.0
    );
    println!("      mode             fec               cfg_dB  meas_dB  verdict");
    for (mode, fec) in candidates {
        let meas = min_decodable_snr(mode, fec, 4.0, 40.0, FRAMES, TARGET);
        print_row("cand", mode, fec, None, meas);
    }
    println!("=== end candidate calibration — set fec_modes + snr_floors from meas_dB ===\n");
}
