//! Kill-first measurement for the weak-signal frequency-diversity rung (#864).
//!
//! Before building a dual-carrier plugin we measure its **ρ=0 upper bound**: the ideal dual-carrier
//! receiver has *perfect* fade decorrelation between branches, no cross-carrier ISI, no per-branch
//! acquisition divergence, and no PAPR penalty — a strict upper bound on any real implementation. If the
//! ideal bound does not clearly beat same-baud single-carrier on a *slow-fading* channel, the real mode
//! cannot, and the rung should not ship (#864's "honest 'no gain → don't ship the rung'" clause).
//!
//! ## How the bound reuses existing machinery
//! An RMS-keyed Watterson channel (`noise_sigma = rms / 10^(snr/20)`) normalises noise to the input
//! power, so a two-carrier split that halves each branch's power is modelled exactly by feeding each
//! branch at **snr − 3 dB**. Two *independent* Watterson draws (different seeds) = perfectly decorrelated
//! branches (ρ=0). `receive_with_llr_combining` already demodulates each look's calibrated LLRs, decodes
//! each alone, and MAP-sums them (the union) — which is exactly MRC for calibrated LLRs. So:
//!   * **single-carrier baseline** = one look at `snr`, `receive_with_fec`;
//!   * **ρ=0 ideal dual** = two independent looks at `snr − 3`, `receive_with_llr_combining(_, _, 2)`;
//!   * **ρ=1 control** = two *same-seed* looks at `snr − 3` (correlated branches → no diversity) — proves
//!     the harness hands no free gain from combining alone.
//!
//! Run: `cargo test -p openpulse-modem --no-default-features --test diversity_upper_bound -- --ignored --nocapture`

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_modem::engine::ModemEngine;

const PAYLOAD: &[u8] = b"Weak-signal frequency-diversity rung bake-off, sixty-four byte payload AA";
const TRIALS: u32 = 48;

/// Which Watterson preset a run uses. `good_f1` (0.1 Hz Doppler / 0.5 ms delay) is slow fading — the
/// physically-expected win region; `moderate_f1` (1 Hz / 1 ms) fades fast enough that the interleaver +
/// soft FEC already harvest time diversity, so it is the redundancy check.
#[derive(Clone, Copy)]
enum Preset {
    GoodF1,
    ModerateF1,
}

impl Preset {
    fn config(self, seed: u64) -> WattersonConfig {
        match self {
            Preset::GoodF1 => WattersonConfig::good_f1(Some(seed)),
            Preset::ModerateF1 => WattersonConfig::moderate_f1(Some(seed)),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Preset::GoodF1 => "good_f1",
            Preset::ModerateF1 => "moderate_f1",
        }
    }
}

fn make() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    (engine, backend)
}

fn tx_samples(mode: &str) -> Vec<f32> {
    let (mut engine, backend) = make();
    engine
        .transmit_with_fec(PAYLOAD, mode, None)
        .expect("transmit");
    backend.drain_samples()
}

fn faded(tx: &[f32], preset: Preset, snr_db: f32, seed: u64) -> Vec<f32> {
    let mut cfg = preset.config(seed);
    cfg.snr_db = snr_db;
    WattersonChannel::new(cfg).expect("watterson").apply(tx)
}

fn seed(trial: u32, branch: u64) -> u64 {
    9000 + (trial as u64) * 10 + branch
}

/// Single-carrier baseline: one look at `snr`, decoded standalone.
fn single_success(mode: &str, tx: &[f32], preset: Preset, snr_db: f32) -> f32 {
    let mut ok = 0u32;
    for trial in 0..TRIALS {
        let (mut rx, backend) = make();
        backend.push_frame(&faded(tx, preset, snr_db, seed(trial, 0)));
        if rx
            .receive_with_fec(mode, None)
            .map(|d| d == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    ok as f32 / TRIALS as f32
}

/// Two half-power looks (`snr − 3`) combined via the engine's decode-each-alone-then-MAP-sum union.
/// `independent = true` → different seeds (ρ=0 ideal); `false` → same seed (ρ=1 control).
fn dual_success(mode: &str, tx: &[f32], preset: Preset, snr_db: f32, independent: bool) -> f32 {
    let branch_snr = snr_db - 3.0;
    let mut ok = 0u32;
    for trial in 0..TRIALS {
        let (mut rx, backend) = make();
        backend.push_frame(&faded(tx, preset, branch_snr, seed(trial, 0)));
        let second_seed = if independent { 1 } else { 0 };
        backend.push_frame(&faded(tx, preset, branch_snr, seed(trial, second_seed)));
        if rx
            .receive_with_llr_combining(mode, None, 2)
            .map(|d| d == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    ok as f32 / TRIALS as f32
}

fn sweep(mode: &str, preset: Preset, snrs: &[f32]) {
    let tx = tx_samples(mode);
    println!(
        "\n=== {mode} on {} ({} trials, PAYLOAD {} B) ===",
        preset.label(),
        TRIALS,
        PAYLOAD.len()
    );
    println!("  snr_db   single   dual_ρ0   dual_ρ1(ctrl)");
    for &snr in snrs {
        let single = single_success(mode, &tx, preset, snr);
        let ideal = dual_success(mode, &tx, preset, snr, true);
        let corr = dual_success(mode, &tx, preset, snr, false);
        println!("  {snr:6.1}   {single:6.2}   {ideal:7.2}   {corr:9.2}");
    }
}

/// Focused, faster BPSK31 point (the actual SL floor the rung sits below). BPSK31 frames are ~8× the
/// audio of BPSK250, so this trims to 24 trials × 3 SNRs on the decisive slow-fade channel.
#[test]
#[ignore = "research measurement for #864; run with --ignored --nocapture"]
fn diversity_upper_bound_bpsk31_good_f1() {
    const N: u32 = 24;
    let mode = "BPSK31";
    let preset = Preset::GoodF1;
    let tx = tx_samples(mode);
    println!(
        "\n=== {mode} on {} ({N} trials, PAYLOAD {} B) ===",
        preset.label(),
        PAYLOAD.len()
    );
    println!("  snr_db   single   dual_ρ0   dual_ρ1(ctrl)");
    for &snr in &[-6.0f32, -3.0, 0.0] {
        // Local copies with N trials (the module fns use TRIALS=48).
        let single = {
            let mut ok = 0u32;
            for trial in 0..N {
                let (mut rx, backend) = make();
                backend.push_frame(&faded(&tx, preset, snr, seed(trial, 0)));
                if rx
                    .receive_with_fec(mode, None)
                    .map(|d| d == PAYLOAD)
                    .unwrap_or(false)
                {
                    ok += 1;
                }
            }
            ok as f32 / N as f32
        };
        let (ideal, corr) = {
            let mut oki = 0u32;
            let mut okc = 0u32;
            for trial in 0..N {
                let bsnr = snr - 3.0;
                for (independent, counter) in [(true, &mut oki), (false, &mut okc)] {
                    let (mut rx, backend) = make();
                    backend.push_frame(&faded(&tx, preset, bsnr, seed(trial, 0)));
                    backend.push_frame(&faded(
                        &tx,
                        preset,
                        bsnr,
                        seed(trial, if independent { 1 } else { 0 }),
                    ));
                    if rx
                        .receive_with_llr_combining(mode, None, 2)
                        .map(|d| d == PAYLOAD)
                        .unwrap_or(false)
                    {
                        *counter += 1;
                    }
                }
            }
            (oki as f32 / N as f32, okc as f32 / N as f32)
        };
        println!("  {snr:6.1}   {single:6.2}   {ideal:7.2}   {corr:9.2}");
    }
}

/// The primary #864 kill-first sweep. Prints frame-success tables; read them against the ship bar:
/// the ρ=0 ideal dual must beat same-baud single-carrier by ≥ ~2 dB equivalent shift on the SLOW-fading
/// channel (good_f1). A win only on moderate_f1 (fast fade) but not good_f1 is backwards — the fast
/// channel's own time diversity, not frequency diversity, and a harness-artifact signal.
#[test]
#[ignore = "research measurement for #864; run with --ignored --nocapture"]
fn diversity_upper_bound_sweep() {
    // BPSK250: short frame (~5 s) → few coherence times on good_f1, so frequency diversity has the most
    // room. If the ideal bound can't win here it can't at the slower BPSK31 floor either.
    sweep("BPSK250", Preset::GoodF1, &[0.0, 3.0, 6.0, 9.0, 12.0]);
    sweep("BPSK250", Preset::ModerateF1, &[3.0, 6.0, 9.0, 12.0, 15.0]);
    // BPSK31: the actual SL floor the rung would sit below.
    sweep("BPSK31", Preset::GoodF1, &[-3.0, 0.0, 3.0, 6.0, 9.0]);
    sweep("BPSK31", Preset::ModerateF1, &[0.0, 3.0, 6.0, 9.0, 12.0]);
}
