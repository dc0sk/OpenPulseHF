//! Real-waveform net measurement for the weak-signal frequency-diversity rung (#864).
//!
//! The `diversity_upper_bound` sweep established the ρ=0 *ideal* (perfect decorrelation, no PAPR) clears
//! the kill-gate (~4 dB on good_f1). This measures the **real dual-carrier waveform**: the same FEC-framed
//! bits on two carriers at `FC ± SEP/2` (`SEP = 750 Hz`, the minimax two-ray decorrelation spacing),
//! through **one** Watterson channel — so the two carriers see the *real*, only-partially-decorrelated
//! fading `ρ(S) = cos(π·S·τ)` plus real cross-carrier interference — and separately the **PAPR delta**,
//! the on-air cost a PEP-limited transmitter pays for the two-tone beat envelope.
//!
//! Two facts make this clean:
//!   * the Watterson channel keys its noise to the input RMS, so the frame-success bake-off is
//!     matched-average-power **by construction** (the 1/√2 power split is auto-normalised away) — each
//!     carrier ends up ~3 dB down, exactly the split;
//!   * PAPR is scale-invariant, so it is measured on the raw `low + high` sum.
//!
//! **Net on-air gain ≈ (matched-power frame-success gain) − ΔPAPR.**
//!
//! Both bits are encoded ONCE (`Frame` + `FecCodec`) and modulated onto each carrier, so the two carriers
//! carry identical bits (a plain `transmit_with_fec` would increment the sequence number between calls).
//! Decode reuses the engine's audio-free union seam `combine_and_decode_llrs` — the exact path
//! `receive_with_llr_combining` uses, and the seam a production diversity plugin would use.
//!
//! Sweep: `cargo test -p openpulse-modem --no-default-features --test diversity_real_waveform -- --ignored --nocapture`

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::fec::FecCodec;
use openpulse_core::frame::Frame;
use openpulse_core::iq::hilbert_iq;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_modem::engine::ModemEngine;

const PAYLOAD: &[u8] = b"Weak-signal frequency-diversity rung bake-off, sixty-four byte payload AA";
const FC: f32 = 1500.0;
const FS: u32 = 8000;
/// Carrier separation: the minimax two-ray decorrelation spacing (ρ² ≈ 0.15/0.50/0.00 on good/mod/poor).
const SEP_HZ: f32 = 750.0;

fn cfg(mode: &str, center: f32) -> ModulationConfig {
    ModulationConfig {
        mode: mode.to_string(),
        center_frequency: center,
        sample_rate: FS,
        ..Default::default()
    }
}

/// FEC-framed wire bytes, encoded once so both carriers carry identical bits.
fn fec_bytes() -> Vec<u8> {
    let frame = Frame::new(0, PAYLOAD.to_vec()).expect("frame").encode();
    FecCodec::new().encode(&frame)
}

fn single_tx(mode: &str, bytes: &[u8]) -> Vec<f32> {
    BpskPlugin::new()
        .modulate(bytes, &cfg(mode, FC))
        .expect("modulate")
}

/// Same bits on `FC ± SEP/2`, summed. Scale-invariant for PAPR; the channel re-normalises the power.
fn dual_tx(mode: &str, bytes: &[u8]) -> Vec<f32> {
    let lo = BpskPlugin::new()
        .modulate(bytes, &cfg(mode, FC - SEP_HZ / 2.0))
        .expect("mod lo");
    let hi = BpskPlugin::new()
        .modulate(bytes, &cfg(mode, FC + SEP_HZ / 2.0))
        .expect("mod hi");
    let n = lo.len().max(hi.len());
    (0..n)
        .map(|i| lo.get(i).copied().unwrap_or(0.0) + hi.get(i).copied().unwrap_or(0.0))
        .collect()
}

/// Peak-to-average power ratio (dB) of the analytic envelope.
fn papr_db(x: &[f32]) -> f32 {
    let (i, q) = hilbert_iq(x, FC, FS as f32);
    let p: Vec<f32> = i.iter().zip(&q).map(|(a, b)| a * a + b * b).collect();
    let peak = p.iter().cloned().fold(0.0f32, f32::max);
    let mean = p.iter().sum::<f32>() / p.len().max(1) as f32;
    10.0 * (peak / mean.max(1e-12)).log10()
}

fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    e
}

fn faded(tx: &[f32], snr_db: f32, seed: u64) -> Vec<f32> {
    let mut c = WattersonConfig::good_f1(Some(seed));
    c.snr_db = snr_db;
    WattersonChannel::new(c).expect("watterson").apply(tx)
}

fn seed(trial: u32) -> u64 {
    9000 + trial as u64 * 3
}

/// Single-carrier: demod at FC, decode one look through the same union seam as the dual path.
fn single_success(mode: &str, tx: &[f32], snr: f32, n: u32) -> f32 {
    let mut e = engine();
    let mut ok = 0u32;
    for t in 0..n {
        let f = faded(tx, snr, seed(t));
        let llr = BpskPlugin::new()
            .demodulate_soft(&f, &cfg(mode, FC))
            .unwrap_or_default();
        if e.combine_and_decode_llrs(mode, &[llr])
            .map(|d| d == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    ok as f32 / n as f32
}

/// Dual-carrier: demod each carrier at its exact center, union-combine the two calibrated LLR vectors.
fn dual_success(mode: &str, tx: &[f32], snr: f32, n: u32) -> f32 {
    let mut e = engine();
    let mut ok = 0u32;
    for t in 0..n {
        let f = faded(tx, snr, seed(t));
        let lo = BpskPlugin::new()
            .demodulate_soft(&f, &cfg(mode, FC - SEP_HZ / 2.0))
            .unwrap_or_default();
        let hi = BpskPlugin::new()
            .demodulate_soft(&f, &cfg(mode, FC + SEP_HZ / 2.0))
            .unwrap_or_default();
        if e.combine_and_decode_llrs(mode, &[lo, hi])
            .map(|d| d == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    ok as f32 / n as f32
}

/// Plumbing guard (not ignored): the real dual-carrier waveform + the `combine_and_decode_llrs` union
/// must round-trip on a clean channel. BPSK31's 62 Hz carriers are 750 Hz apart → negligible cross-ISI.
#[test]
fn dual_carrier_round_trips_on_a_clean_channel() {
    let mode = "BPSK31";
    let bytes = fec_bytes();
    let tx = dual_tx(mode, &bytes);
    let lo = BpskPlugin::new()
        .demodulate_soft(&tx, &cfg(mode, FC - SEP_HZ / 2.0))
        .expect("demod lo");
    let hi = BpskPlugin::new()
        .demodulate_soft(&tx, &cfg(mode, FC + SEP_HZ / 2.0))
        .expect("demod hi");
    let mut e = engine();
    let out = e
        .combine_and_decode_llrs(mode, &[lo, hi])
        .expect("decode combined");
    assert_eq!(out, PAYLOAD, "dual-carrier must round-trip clean");
}

fn sweep(mode: &str, n: u32, snrs: &[f32]) {
    let bytes = fec_bytes();
    let single = single_tx(mode, &bytes);
    let dual = dual_tx(mode, &bytes);
    let dpapr = papr_db(&dual) - papr_db(&single);
    println!(
        "\n=== {mode} on good_f1 ({n} trials, PAYLOAD {} B, SEP {SEP_HZ} Hz) ===",
        PAYLOAD.len()
    );
    println!(
        "  PAPR single {:.2} dB, dual {:.2} dB, ΔPAPR {:+.2} dB (on-air avg-power cost)",
        papr_db(&single),
        papr_db(&dual),
        dpapr
    );
    println!("  snr_db   single   dual(real)   gain_note");
    for &snr in snrs {
        let s = single_success(mode, &single, snr, n);
        let d = dual_success(mode, &dual, snr, n);
        println!("  {snr:6.1}   {s:6.2}   {d:9.2}");
    }
    println!("  → net on-air ≈ (frame-success gain at matched avg power) − {dpapr:+.2} dB PAPR");
}

/// The real-waveform net measurement. Read the frame-success gain (dB shift at the 0.5 crossing) and
/// subtract the ΔPAPR to get the expected on-air net.
#[test]
#[ignore = "research measurement for #864; run with --ignored --nocapture"]
fn diversity_real_waveform_sweep() {
    sweep("BPSK250", 48, &[0.0, 3.0, 6.0, 9.0, 12.0]);
    sweep("BPSK31", 24, &[-6.0, -3.0, 0.0, 3.0]);
}
