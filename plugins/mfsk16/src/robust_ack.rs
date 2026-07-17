//! REQ-WSIG-01 PR-C — robust-ACK diversity measurement (measure-first; ship only if it clears).
//!
//! The short `MFSK16-ACK` (40 sym ≈ 1.28 s, ShortFec t=4) decodes only ~0.6 at the data rung's ~0 dB
//! floor — the binding constraint on an ARQ return channel. The verified mechanism (poor_f1 = 2 Hz Doppler,
//! so a 1.28 s frame spans ~5 coherence times — it does NOT sit inside one fade) is a **fade burst
//! exceeding the tiny t=4 code**, not a lack of fade-averaging. A prior naive fix (energy-sum 3× copies)
//! was rejected — energy-summing a faded copy adds its noise and dilutes the clean copies (#694).
//!
//! Two candidate fixes are measured head-to-head at matched airtime, both at ≥400 trials, on moderate_f1
//! AND poor_f1 at the ~0 dB floor. Pre-registered bar: **decode ≥ 0.9 at 0 dB on BOTH**.
//!
//! * **Arm B — longer contiguous frame** (Fable guardrail #1, the likely winner): one frame, a stronger
//!   ShortFec code (larger `t`) so a fade burst stays inside the byte-correction budget, **one** acquisition
//!   on more sync energy. (Interleaving is inert here — a single RS block is position-agnostic within the
//!   block — so code strength `t`, not interleaving, is the lever; interleaving would only matter across
//!   multiple blocks.) The 17 s data frame decoding 0.85 at the same SNR is the existence proof that
//!   "longer contiguous" already beats "short repeated".
//! * **Arm C — K-copy per-copy-LLR diversity**: K time-spaced, frequency-hopped copies; demodulate each to
//!   calibrated soft LLRs; **union-decode** (decode each copy alone, MAP-sum only as a fallback — #694, NOT
//!   sum-then-decode) — plus a genie-sync arm and a wrong-lock counter to tell an acquisition bottleneck
//!   from a combining bottleneck, and a hop ablation so a two-tap-model overfit can't masquerade as a win.

use super::*;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::fec::{combine_llrs_map, Interleaver, ShortFecCodec};

/// A Watterson profile constructor (`moderate_f1`, `poor_f1`, …) taking an optional seed.
type ChannelCfgFn = fn(Option<u64>) -> WattersonConfig;

const FS: u32 = 8000;
const FC: f32 = 1500.0;
const ACK_DATA_LEN: usize = 5; // AckFrame wire length

/// The 5-byte ACK we transmit; success = an exact RS-corrected match (and it re-parses + CRC-validates).
fn ack_bytes() -> [u8; 5] {
    AckFrame::new(AckType::AckOk, "robust-ack").encode()
}

/// LLR → bytes, exactly the engine convention (`hard_decide`): LSB-first, negative LLR = bit 1.
fn hard_bytes(llrs: &[f32]) -> Vec<u8> {
    llrs.chunks(8)
        .map(|c| {
            c.iter()
                .enumerate()
                .fold(0u8, |a, (i, &l)| a | ((l.is_sign_negative() as u8) << i))
        })
        .collect()
}

/// A runtime frame geometry: `n_sync` Costas blocks each followed by an even data-tone gap (ends on data,
/// like `ACK_LAYOUT`). Leaks the sync-start slice — called once per config, never per trial.
fn make_layout(frame_bytes: usize, n_sync: usize) -> Layout {
    let data_tones = frame_bytes * 2;
    let onair = data_tones + COSTAS16.len() * n_sync;
    let base_gap = data_tones / n_sync;
    let rem = data_tones % n_sync;
    let mut starts = Vec::with_capacity(n_sync);
    let mut pos = 0usize;
    for i in 0..n_sync {
        starts.push(pos);
        pos += COSTAS16.len() + base_gap + usize::from(i < rem);
    }
    debug_assert_eq!(pos, onair);
    Layout {
        frame_bytes,
        sync_starts: Box::leak(starts.into_boxed_slice()),
        onair_tones: onair,
    }
}

/// Modulate an already-coded byte block (len == `lay.frame_bytes`) to audio at `fc`.
fn modulate_block(coded: &[u8], lay: &Layout, fc: f32) -> Vec<f32> {
    let tones = insert_sync(&bytes_to_data_tones(coded, lay), lay);
    modulate_tones(&tones, base_freq(fc), &gfsk_params(FS))
}

/// Per-symbol data LLRs from a known window offset + base (genie sync: skips `acquire`).
fn soft_at(audio: &[f32], lay: &Layout, base: f32, offset: usize) -> Option<Vec<f32>> {
    let mut energies = Vec::with_capacity(lay.onair_tones);
    for &p in &data_positions(lay) {
        let start = offset + p * SPS;
        energies.push(sym_energies(
            audio.get(start..start + SPS)?,
            base,
            FS as f32,
        ));
    }
    let inv = 1.0 / frame_noise(&energies);
    let mut llrs = Vec::with_capacity(energies.len() * BITS_PER_SYM);
    for e in &energies {
        llrs.extend_from_slice(&bit_llrs(e, inv));
    }
    Some(llrs)
}

/// Real acquisition then soft demod; returns the LLRs and the acquired sample offset (for wrong-lock).
fn soft_real(audio: &[f32], lay: &Layout, fc: f32) -> Option<(Vec<f32>, usize)> {
    let (offset, base) = acquire(audio, base_freq(fc), FS as f32, lay)?;
    soft_at(audio, lay, base, offset).map(|l| (l, offset))
}

/// Decode one copy's LLRs to an accept/reject against the sent ACK (RS-correct → deinterleave → exact match
/// → CRC re-parse). The CRC re-parse is the false-decode guard: a wrong-lock RS-mis-correction is rejected.
fn accept(llrs: &[f32], fec: &ShortFecCodec, il: &Interleaver, frame_bytes: usize) -> bool {
    let bytes = hard_bytes(llrs);
    if bytes.len() != frame_bytes {
        return false;
    }
    let Ok(data) = fec.decode(&il.deinterleave(&bytes)) else {
        return false;
    };
    let Ok(arr) = <[u8; 5]>::try_from(&data[..]) else {
        return false;
    };
    arr == ack_bytes() && AckFrame::decode(&arr).is_ok()
}

/// #694 union: each copy decoded standalone first; MAP-sum only as a fallback. Success is a strict superset
/// of both, so a clean copy is never diluted by a faded/wrong-locked one.
fn union_accept(
    copies: &[Vec<f32>],
    fec: &ShortFecCodec,
    il: &Interleaver,
    frame_bytes: usize,
) -> bool {
    if copies.iter().any(|c| accept(c, fec, il, frame_bytes)) {
        return true;
    }
    let refs: Vec<&[f32]> = copies.iter().map(|c| c.as_slice()).collect();
    accept(&combine_llrs_map(&refs), fec, il, frame_bytes)
}

fn faded(cfg_fn: ChannelCfgFn, snr: f32, seed: u64, sig: &[f32]) -> Vec<f32> {
    let mut cfg = cfg_fn(Some(seed));
    cfg.snr_db = snr;
    WattersonChannel::new(cfg).expect("watterson").apply(sig)
}

// ── Arm A: baseline single short ACK (frame_bytes=13, t=4, no interleave) ───────────────────────────────

fn arm_a_rate(cfg_fn: ChannelCfgFn, snr: f32, trials: u32) -> f32 {
    let fec = ShortFecCodec::new();
    let il = Interleaver::new(1); // identity
    let coded = fec.encode(&ack_bytes()).expect("encode"); // 13 bytes
    let lay = ACK_LAYOUT;
    let mut ok = 0;
    for t in 0..trials {
        let seed = 7000 + t as u64;
        let df = (seed % 51) as f32 - 25.0;
        let lead = (seed / 51 % 200) as usize;
        let mut sig = vec![0.0; lead];
        sig.extend(modulate_block(&coded, &lay, FC + df));
        let rx = faded(cfg_fn, snr, seed, &sig);
        if let Some((llrs, _)) = soft_real(&rx, &lay, FC) {
            if accept(&llrs, &fec, &il, 13) {
                ok += 1;
            }
        }
    }
    ok as f32 / trials as f32
}

// ── Arm B: one longer contiguous frame, stronger code + interleave, one acquisition ─────────────────────

/// `ecc` ShortFec ECC bytes (t = ecc/2), `n_sync` Costas blocks, full-depth byte interleave.
fn arm_b_rate(
    cfg_fn: ChannelCfgFn,
    snr: f32,
    ecc: usize,
    n_sync: usize,
    trials: u32,
) -> (f32, usize) {
    let fec = ShortFecCodec::with_ecc_len(ecc);
    let frame_bytes = ACK_DATA_LEN + ecc;
    let il = Interleaver::new(1); // identity — single RS block, so t (not interleaving) is the lever
    let coded = fec.encode(&ack_bytes()).expect("encode");
    let lay = make_layout(frame_bytes, n_sync);
    let mut ok = 0;
    for t in 0..trials {
        let seed = 11_000 + t as u64;
        let df = (seed % 51) as f32 - 25.0;
        let lead = (seed / 51 % 200) as usize;
        let mut sig = vec![0.0; lead];
        sig.extend(modulate_block(&coded, &lay, FC + df));
        let rx = faded(cfg_fn, snr, seed, &sig);
        if let Some((llrs, _)) = soft_real(&rx, &lay, FC) {
            if accept(&llrs, &fec, &il, frame_bytes) {
                ok += 1;
            }
        }
    }
    (ok as f32 / trials as f32, lay.onair_tones)
}

// ── Arm C: K time-spaced, frequency-hopped copies, per-copy-LLR union decode ────────────────────────────

struct ArmC {
    real: f32,
    genie: f32,
    wrong_locks: u32,
}

/// `k` copies, `hop_hz` per-copy frequency step (0 = ablation), `gap_s` inter-copy silence. Copies pass
/// through ONE continuous Watterson realization (honest fade correlation), then each is sliced to its own
/// turnaround window and independently acquired (the ARQ receiver knows the schedule).
fn arm_c(cfg_fn: ChannelCfgFn, snr: f32, k: usize, hop_hz: f32, gap_s: f32, trials: u32) -> ArmC {
    let fec = ShortFecCodec::new();
    let il = Interleaver::new(1);
    let coded = fec.encode(&ack_bytes()).expect("encode");
    let lay = ACK_LAYOUT;
    let copy_len = lay.onair_tones * SPS;
    let gap = (gap_s * FS as f32) as usize;
    let mut ok_real = 0;
    let mut ok_genie = 0;
    let mut wrong = 0;
    for t in 0..trials {
        let seed = 21_000 + t as u64;
        let df = (seed % 51) as f32 - 25.0;
        let lead = (seed / 51 % 200) as usize;
        // Build the concatenated multi-copy TX; record each copy's true start + its fc.
        let mut sig = vec![0.0; lead];
        let mut starts = Vec::with_capacity(k);
        let mut fcs = Vec::with_capacity(k);
        for c in 0..k {
            let fc = FC + df + (c as f32 - (k as f32 - 1.0) / 2.0) * hop_hz;
            starts.push(sig.len());
            fcs.push(fc);
            sig.extend(modulate_block(&coded, &lay, fc));
            if c + 1 < k {
                sig.extend(std::iter::repeat_n(0.0, gap));
            }
        }
        let rx = faded(cfg_fn, snr, seed, &sig);
        // Per-copy: slice a turnaround window, real-acquire, and also genie-demod at the true offset.
        let mut real_copies = Vec::with_capacity(k);
        let mut genie_copies = Vec::with_capacity(k);
        let margin = 220usize; // ≥ max lead, so acquisition is genuinely exercised inside the slice
        for c in 0..k {
            let s0 = starts[c].saturating_sub(margin);
            let s1 = (starts[c] + copy_len + margin).min(rx.len());
            let slice = &rx[s0..s1];
            let true_off = starts[c] - s0;
            if let Some(g) = soft_at(slice, &lay, base_freq(fcs[c]), true_off) {
                genie_copies.push(g);
            }
            if let Some((llrs, off)) = soft_real(slice, &lay, fcs[c]) {
                if off.abs_diff(true_off) > SPS / 2 {
                    wrong += 1;
                }
                real_copies.push(llrs);
            }
        }
        if !real_copies.is_empty() && union_accept(&real_copies, &fec, &il, 13) {
            ok_real += 1;
        }
        if !genie_copies.is_empty() && union_accept(&genie_copies, &fec, &il, 13) {
            ok_genie += 1;
        }
    }
    ArmC {
        real: ok_real as f32 / trials as f32,
        genie: ok_genie as f32 / trials as f32,
        wrong_locks: wrong,
    }
}

// ── Gates + research sweeps ─────────────────────────────────────────────────────────────────────────────

/// Shipped-path single-ACK decode (hard argmax via the public `demodulate`), to reconcile against the prior
/// 40-trial ack_feasibility 0.60 and against the LLR-sign decoder used by the diversity arms.
fn arm_a_argmax(cfg_fn: ChannelCfgFn, snr: f32, trials: u32) -> f32 {
    let plugin = Mfsk16Plugin::new();
    let fec = ShortFecCodec::new();
    let coded = fec.encode(&ack_bytes()).expect("encode");
    let lay = ACK_LAYOUT;
    let acfg = ModulationConfig {
        mode: "MFSK16-ACK".into(),
        center_frequency: FC,
        sample_rate: FS,
        ..Default::default()
    };
    let mut ok = 0;
    for t in 0..trials {
        let seed = 7000 + t as u64;
        let df = (seed % 51) as f32 - 25.0;
        let lead = (seed / 51 % 200) as usize;
        let mut sig = vec![0.0; lead];
        sig.extend(modulate_block(&coded, &lay, FC + df));
        let rx = faded(cfg_fn, snr, seed, &sig);
        if let Ok(bytes) = plugin.demodulate(&rx, &acfg) {
            if fec.decode(&bytes).ok().as_deref() == Some(&ack_bytes()[..]) {
                ok += 1;
            }
        }
    }
    ok as f32 / trials as f32
}

/// Regression gate for the corrected finding: at a proper trial count the single short `MFSK16-ACK` sits at
/// ~0.90 at the 0 dB data floor on moderate_f1 — it is NOT the 0.60 the original 40-trial ack_feasibility
/// reported (a small-sample artifact). Guards against a real regression back toward that floor.
#[test]
fn single_ack_is_near_the_floor_bar() {
    let rate = arm_a_argmax(WattersonConfig::moderate_f1, 0.0, 160);
    assert!(
        rate >= 0.80,
        "single MFSK16-ACK on moderate_f1 @0 dB decoded {rate:.2} (< 0.80) — regressed below the \
         corrected ~0.90 baseline (robust_ack::baseline_reconciliation)"
    );
}

/// Acceptance gate for the shipped robust-ACK design: K=3 time-spaced copies + per-copy-LLR union decode
/// (no frequency hop) hold ≥ 0.9 at 3 dB **below** the 0 dB data floor on moderate_f1 — where a single ACK
/// holds only ~0.66 — buying the ARQ return channel a real fade margin. The full A/B/C sweep and the
/// wrong-lock / genie diagnostics live in `robust_ack_sweep`.
#[test]
fn k3_union_holds_below_the_floor() {
    let c = arm_c(WattersonConfig::moderate_f1, -3.0, 3, 0.0, 0.5, 60);
    assert!(
        c.real >= 0.9,
        "K=3 union ACK on moderate_f1 @-3 dB decoded {:.2} (< 0.9); genie {:.2}, wrong-locks {} \
         (genie≫real ⇒ acquisition regressed; else combining)",
        c.real,
        c.genie,
        c.wrong_locks
    );
}

/// Reconcile the prior 40-trial baseline. Prints argmax vs LLR-sign at 400 trials; run with `--nocapture`.
#[test]
#[ignore = "REQ-WSIG-01 robust-ACK reconciliation; -- --ignored --nocapture"]
fn baseline_reconciliation() {
    let n = 400;
    for (name, cf) in [
        ("moderate_f1", WattersonConfig::moderate_f1 as ChannelCfgFn),
        ("poor_f1", WattersonConfig::poor_f1),
    ] {
        println!("\n── single ACK, {name}, {n} trials ──");
        for snr in [-3.0f32, 0.0, 3.0] {
            println!(
                "  {snr:+.0} dB   argmax {:.3}   llr-sign {:.3}",
                arm_a_argmax(cf, snr, n),
                arm_a_rate(cf, snr, n)
            );
        }
    }
}

/// Full head-to-head table. Research measurement — run with `--ignored --nocapture`.
#[test]
#[ignore = "REQ-WSIG-01 robust-ACK measurement; cargo test -p mfsk16-plugin robust_ack_sweep -- --ignored --nocapture"]
fn robust_ack_sweep() {
    let trials = 400;
    let channels: [(&str, ChannelCfgFn); 2] = [
        ("moderate_f1", WattersonConfig::moderate_f1),
        ("poor_f1", WattersonConfig::poor_f1),
    ];
    let single_air = ACK_LAYOUT.onair_tones; // 40 sym reference
    for (name, cf) in channels {
        println!("\n════ {name}, {trials} trials (single ACK = {single_air} sym ≈ 1.28 s) ════");
        for snr in [-3.0f32, 0.0] {
            println!("  ── {snr:+.0} dB ──");
            println!(
                "    Arm A baseline (40 sym) ............. {:.2}",
                arm_a_rate(cf, snr, trials)
            );
            for (ecc, ns) in [(16, 3), (32, 4), (48, 5)] {
                let (r, air) = arm_b_rate(cf, snr, ecc, ns, trials);
                println!(
                    "    Arm B t={:<2} {:>3} sym (×{:.1} air) ..... {:.2}",
                    ecc / 2,
                    air,
                    air as f32 / single_air as f32,
                    r
                );
            }
            for (k, hop) in [(3, 0.0f32), (3, 500.0), (2, 500.0)] {
                let c = arm_c(cf, snr, k, hop, 0.5, trials);
                println!(
                    "    Arm C K={k} hop={hop:.0}Hz .... real {:.2}  genie {:.2}  wrong-locks {}",
                    c.real, c.genie, c.wrong_locks
                );
            }
        }
    }
}
