//! SNR-floor calibration harness — the "quick simulation run" that derives the working SNR/step
//! pairs the OTA fast-downshift jumps to (`SessionProfile::snr_floor_for_level`).
//!
//! Sweeps SNR for each (mode, FEC) rung and finds the lowest SNR at which it decodes reliably —
//! the empirical floor. Prints a table so the constants in `profile.rs` can be validated or retuned
//! from data instead of guesswork. Four `#[ignore]` sweeps:
//!   * `calibrate_snr_floors_hpx_hf` — every hpx_hf rung over AWGN with its assigned FEC.
//!   * `calibrate_candidate_fec_rungs` — candidate (mode, FEC) pairs under consideration (AWGN).
//!   * `calibrate_snr_floors_watterson` — the same rungs over Watterson **fading** (good_f1 +
//!     moderate_f1). AWGN floors are a lower bound; fading raises them.
//!   * `calibrate_pilot_gap_candidate` — the SL7 (11→16 dB) gap-filler reassessment: 8PSK500+RS
//!     vs the pilot-aided PILOT-8PSK500 across AWGN + both fading profiles (2026-07-05).
//!
//! **Fading-calibration interpretation (read before using the Watterson numbers):**
//!   1. Fading is seed-sensitive — a fraction of realizations deep-fade the whole frame and can't
//!      decode at ANY SNR (irreducible outage), so the fading target is 50 % (majority), not 90 %.
//!   2. The SCFDMA-QAM rungs do not reach the 90 % HF fading gate, so they have no fading floor to
//!      calibrate — only AWGN. They are high-throughput top rungs for *good* HF conditions and the
//!      adaptive ladder downshifts off them on a fading path.
//!      **This was overstated before 2026-07-08**: those rungs used to decode ~0 % of Watterson frames
//!      at *any* SNR, which was read as "correct and by design". It was a channel-estimator bug — the
//!      DFT-CE mis-reconstructed every frequency-selective channel (see `plugins/scfdma/src/channel.rs`
//!      → `DelayCe`). With the delay-basis estimator they decode ~12–32 % of good_f1 frames under soft
//!      FEC, still short of the gate. A floor of "no SNR works" is a bug signature, not a design fact.
//!   3. The single-carrier no-FEC/light-FEC rungs (QPSK250/500, 8PSK500+RS) are slow-fading-only:
//!      good_f1 ~6–9 dB, but moderate_f1 (1 Hz Doppler) fails at any SNR (no equalizer / weak FEC).
//!
//! Run on demand (full modulate→channel→demodulate sweeps, so ignored by default):
//!
//! ```text
//! cargo test -p openpulse-modem --no-default-features --test snr_floor_calibration -- --ignored --nocapture
//! ```

use bpsk_plugin::BpskPlugin;
use openpulse_channel::{
    awgn::AwgnChannel, watterson::WattersonChannel, AwgnConfig, WattersonConfig,
};
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_modem::channel_sim::ChannelSimHarness;
use pilot_plugin::PilotPlugin;
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
        eng.register_plugin(Box::new(PilotPlugin::new())).unwrap();
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

/// Success rate of `frames` FEC-protected round-trips through a Watterson fading channel (fresh
/// fading realisation per frame) at additive `snr_db`. `make_cfg` fixes the fading profile.
fn decode_rate_watterson(
    mode: &str,
    fec: FecMode,
    snr_db: f32,
    frames: u32,
    make_cfg: fn(f32, Option<u64>) -> WattersonConfig,
) -> f32 {
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
        let mut ch = match WattersonChannel::new(make_cfg(snr_db, Some(2000 + f as u64))) {
            Ok(c) => c,
            Err(_) => continue,
        };
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

/// A Watterson config for a given additive SNR, keeping the profile's fading params.
fn watterson_good_f1(snr_db: f32, seed: Option<u64>) -> WattersonConfig {
    let mut c = WattersonConfig::good_f1(seed);
    c.snr_db = snr_db;
    c
}
fn watterson_moderate_f1(snr_db: f32, seed: Option<u64>) -> WattersonConfig {
    let mut c = WattersonConfig::moderate_f1(seed);
    c.snr_db = snr_db;
    c
}

/// Lowest fading SNR in `[lo, hi]` at which `mode`+`fec` decodes ≥ `target`.
fn min_decodable_snr_watterson(
    mode: &str,
    fec: FecMode,
    lo: f32,
    hi: f32,
    frames: u32,
    target: f32,
    make_cfg: fn(f32, Option<u64>) -> WattersonConfig,
) -> Option<f32> {
    let mut snr = lo;
    while snr <= hi {
        if decode_rate_watterson(mode, fec, snr, frames, make_cfg) >= target {
            return Some(snr);
        }
        snr += 1.0;
    }
    None
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
#[ignore = "fading calibration sweep; run manually with --ignored --nocapture"]
fn calibrate_snr_floors_watterson() {
    const FRAMES: u32 = 16;
    // Fading is seed-sensitive: a fraction of realizations deep-fade the whole frame and can't decode
    // at ANY SNR (an irreducible outage), so a 90 % target is unreachable under Watterson. The fading
    // "floor" is where a MAJORITY of fades decode — use a 50 % target (cf. the seed-window pattern in
    // `channel_loopback.rs`).
    const TARGET: f32 = 0.50;
    let profile = SessionProfile::hpx_hf();
    println!(
        "\n=== hpx_hf Watterson fading calibration (50% target — fading has an outage floor) ==="
    );
    println!("level mode             fec               cfg_dB  gF1_dB  mF1_dB  gap(mF1-cfg)");
    for level in profile.defined_levels() {
        let Some(mode) = profile.mode_for(level) else {
            continue;
        };
        // Skip the low-baud BPSK rungs (SL2–4): their multi-second frames make the per-frame fading
        // FFT prohibitively slow, and they are the trivially-robust ladder floor — the fading margin
        // that matters for rate decisions is on QPSK250 upward.
        if mode.starts_with("BPSK") {
            continue;
        }
        let fec = profile.fec_for(level);
        // Fading only *raises* the floor, so anchor the search at the AWGN floor (bounded window).
        let anchor = profile.snr_floor_for_level(level).unwrap_or(0.0);
        let (lo, hi) = (anchor - 3.0, anchor + 16.0);
        let good =
            min_decodable_snr_watterson(mode, fec, lo, hi, FRAMES, TARGET, watterson_good_f1);
        let mod1 =
            min_decodable_snr_watterson(mode, fec, lo, hi, FRAMES, TARGET, watterson_moderate_f1);
        let cfg = profile.snr_floor_for_level(level);
        let gap = match (cfg, mod1) {
            (Some(c), Some(m)) => format!("{:+.0}", m - c),
            _ => "-".into(),
        };
        let fmt = |v: Option<f32>| v.map(|x| format!("{x:.0}")).unwrap_or_else(|| ">40".into());
        println!(
            "{level:<5?} {mode:<16} {fec:<16?} {:>7} {:>7} {:>7}  {gap}",
            cfg.map(|c| format!("{c:.0}")).unwrap_or_else(|| "-".into()),
            fmt(good),
            fmt(mod1),
        );
    }
    println!("=== end Watterson calibration — set floors to cover moderate_f1 (mF1) ===\n");
}

#[test]
#[ignore = "calibration sweep; run manually with --ignored --nocapture"]
fn calibrate_candidate_fec_rungs() {
    const FRAMES: u32 = 20;
    const TARGET: f32 = 0.90;
    // The FEC-protected hpx_hf upper ladder (mode, FEC) as assigned in `SessionProfile::hpx_hf`:
    // 8PSK500 gets *light* RS (keeps it a faster rung than QPSK500 while filling the 11→16 gap); the
    // dense SCFDMA rungs get soft-concatenated FEC (they only run FEC-protected). Re-run to re-derive
    // the floors if the DSP changes. (The old cross-32QAM inversion — SL10 measuring harder than
    // SL11 — was fixed at the root by the 2D-Gray remap in #616: 32QAM dropped 17→9 dB AWGN.)
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

/// Reassessment of the SL7 (11→16 dB) gap-filler: does the cycle-slip-immune, pilot-aided
/// `PILOT-8PSK500` retain a *finite* moderate_f1 (1 Hz Doppler) floor where the decision-directed
/// `8PSK500` fails at any SNR? If so it is the more robust occupant of the slot. AWGN uses 90 %;
/// fading uses 50 % (irreducible outage — see the module notes).
#[test]
#[ignore]
fn calibrate_pilot_gap_candidate() {
    const FRAMES: u32 = 20;
    let candidates = [("8PSK500", FecMode::Rs), ("PILOT-8PSK500", FecMode::Rs)];
    println!("\n=== SL7 gap-filler reassessment (AWGN 90% / fading 50%) ===");
    println!("      mode             awgn_dB  gF1_dB  mF1_dB");
    let fmt = |v: Option<f32>| {
        v.map(|x| format!("{x:.0}"))
            .unwrap_or_else(|| ">lim".into())
    };
    for (mode, fec) in candidates {
        let awgn = min_decodable_snr(mode, fec, 4.0, 40.0, FRAMES, 0.90);
        let gf1 =
            min_decodable_snr_watterson(mode, fec, 4.0, 30.0, FRAMES, 0.50, watterson_good_f1);
        let mf1 =
            min_decodable_snr_watterson(mode, fec, 4.0, 30.0, FRAMES, 0.50, watterson_moderate_f1);
        println!(
            "cand  {mode:<16} {:>7} {:>7} {:>7}",
            fmt(awgn),
            fmt(gf1),
            fmt(mf1)
        );
    }
    println!("=== end SL7 gap-filler reassessment ===\n");
}

/// Focused AWGN floors for the finer-`hpx_hf` gap-filler candidates
/// (research #2, `docs/dev/research/ladder-granularity.md`). AWGN is a lower bound; fading raises it.
///
/// Run: `cargo test -p openpulse-modem --no-default-features --test snr_floor_calibration \
///   -- --ignored --nocapture calibrate_ladder_gap_fillers`
#[test]
#[ignore]
fn calibrate_ladder_gap_fillers() {
    println!("\n=== hpx_hf gap-filler candidates (AWGN, ≥90% of 16 frames) ===");
    let candidates: &[(&str, FecMode, f32, f32)] = &[
        ("BPSK31", FecMode::Rs, 0.0, 12.0),
        ("BPSK100", FecMode::None, 0.0, 14.0),
        ("QPSK250", FecMode::Rs, 2.0, 16.0),
        ("SCFDMA26-32QAM", FecMode::SoftConcatenated, 8.0, 24.0),
        ("SCFDMA52-64QAM-P4", FecMode::SoftConcatenated, 12.0, 32.0),
    ];
    for (mode, fec, lo, hi) in candidates {
        let meas = min_decodable_snr(mode, *fec, *lo, *hi, 16, 0.9);
        print_row("gap", mode, *fec, None, meas);
    }
}
