//! #1428 step 1 — the ENGINE-level soft-vs-hard A/B, with real RS and the scrambler.
//!
//! **SINCE #1428's UNION LANDED, THE SECOND COLUMN IS NO LONGER THE CANCELLED ARM.** Its entry,
//! `receive_with_fec_mode(Rs)` → `receive_with_fec`, now decodes BOTH decision arms and keeps the
//! first that RS accepts. So this harness now measures **uncancelled vs union**, not uncancelled vs
//! cancelled, and its "hard"/"H-only" columns are the union's. Expect the union column to be at
//! least the soft column in every cell. A soft-only frame would indicate the soft sign-slice and
//! variant 1 have diverged — nothing pins them bit-identical for BPSK (`hard_variant_conformance`'s
//! I3 skips two-variant plugins) — so treat one as a finding to check, not as noise.
//! The numbers recorded against this harness in the traceability ledger (2026-09-22) predate the
//! union and ARE the cancelled-vs-uncancelled comparison; do not re-run this and compare to them.
//! The labels below are kept as printed so those recorded tables stay readable.
//!
//! **What is new here, stated checkably.** #1363 opened with engine-level frame counts, so this is
//! not the thread's first decode rate. It is the first since that opening, and the first whose
//! apparatus is known to put BOTH arms through the same hard RS — the opening left that open, and
//! every number in between is the bad-byte proxy (bad bytes over the 200 payload bytes of a
//! 255-byte wire frame, RS never run), which is a lower bound for both arms alike and explicitly
//! not a decode rate.
//!
//! **Why this is a pure cancellation toggle.** Both arms lock timing with the same
//! `find_timing_offset_with_expected` and run the same `demodulate_iq`; the hard arm adds
//! `cancel_crossfade_isi`; the soft arm multiplies by `differential_llr_scale`, a single POSITIVE
//! scalar floored at 1e-6, so hard-decision sign-slicing reproduces `differential_decode`'s
//! `dot < 0` bit for bit. Both then run strict `rs_decode_free_strengthened` + `stage_decode_frame`.
//! `receive_with_llr_combining(n = 1)` is degenerate but not different: one capture, one
//! `demodulate_soft`, and a "combined" fallback that is `combine_llrs_map` of a single vector.
//!
//! **"Hard" is the shipped FIRST-ATTEMPT arm, not the only shipped arm.** It is what CLI
//! `receive_with_fec` and the OTA candidate pass run. The daemon's OTA path also admits `Rs` to the
//! HARQ branch, where `ota_demodulate_soft` produces UNCANCELLED LLRs that are retained and
//! MAP-combined with the next burst — so on air a retransmission is neither arm alone.
//!
//! **Three traps, each handled rather than hoped about** — all three would silently break the A/B:
//!
//! 1. *`decode_prefix` vs strict `decode`.* The soft arm RS-decodes strictly; the SCANNING hard arm
//!    uses `decode_prefix`. With any trailing audio the soft arm fails `FecCodec::decode`'s
//!    multiple-of-255 gate while hard passes — not a toggle. Hence single-shot
//!    `receive_with_fec_mode` on a buffer-IS-the-frame fixture. The table is its own evidence that
//!    this worked: soft reaching 96/96 at 0 dB is only possible if the demodulated byte count
//!    clears strict decode.
//! 2. *AFC carry-over.* Both paths build `mod_cfg` BEFORE `update_afc_estimate` and update AFTER, so
//!    a second arm on the same engine would demodulate at a different `center_frequency`. A fresh
//!    engine per arm per seed removes that BY CONSTRUCTION; the `afc_correction_hz() == 0` assert
//!    pins the constructor's initial state and is not itself the guarantee.
//! 3. *Payload length.* `free_rs_strengthening` upgrades `Rs` to t = 32 at payload <= 177 B, which
//!    would move the cliff every #1363 number is counted against. Hence 200 B.
//!
//! **Why PAIRED, and what that does and does not license.** #1428 pre-registered a MARGINAL
//! threshold — "soft - hard < ~8 frames at 8 dB" — and named no statistic and no SE. The design was
//! paired by seed from the opening onward, so the marginal difference is the SE of an analysis
//! nobody was going to run. Paired at the observed discordance the difference has
//! SE = sqrt(b + c - (b - c)^2 / n) = sqrt(17 - 121/96) = 3.97, so the threshold is ~2 sigma and the
//! observed +11 clears it by 3 frames, i.e. **less than one paired SE**. The discordant pair is the
//! statistic (14 against 3, exact McNemar p = 0.013). Note the order of events honestly: the switch
//! to the paired statistic was made AFTER seeing the data. The per-seed pattern string is printed
//! so a marginal can be audited rather than trusted.
//!
//! **This file asserts nothing about the counts.** It is an instrument, not a gate — the only
//! assertion is on the engine's initial AFC state.
//!
//! Run: `cargo test -p openpulse-modem --no-default-features --test engine_cancellation_ab -- --ignored --nocapture`
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{awgn::AwgnChannel, watterson::WattersonChannel};
use openpulse_channel::{AwgnConfig, ChannelModel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_modem::engine::ModemEngine;

/// One measurement cell: a name and the channel it applies to the transmitted audio, keyed by seed.
type Cell = (String, Box<dyn Fn(u64) -> Vec<f32>>);

const MODE: &str = "BPSK250";
const SEEDS: u64 = 96;

fn engine() -> (ModemEngine, LoopbackBackend) {
    let b = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(b.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register");
    (e, b)
}

fn ln_choose(n: u64, k: u64) -> f64 {
    let lg = |x: u64| -> f64 { (1..=x).map(|v| (v as f64).ln()).sum() };
    lg(n) - lg(k) - lg(n - k)
}

/// Exact two-sided McNemar p from the discordant counts (b = soft-only, c = hard-only).
fn mcnemar_p(b: u64, c: u64) -> f64 {
    let n = b + c;
    if n == 0 {
        return 1.0;
    }
    let m = b.min(c);
    let tail: f64 = (0..=m)
        .map(|k| (ln_choose(n, k) - (n as f64) * std::f64::consts::LN_2).exp())
        .sum();
    (2.0 * tail).min(1.0)
}

#[test]
#[ignore = "measurement instrument, run on demand (~150 s); asserts nothing about the counts"]
fn engine_soft_vs_hard_paired() {
    let payload: Vec<u8> = (0..200u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    let tx = {
        let (mut e, b) = engine();
        e.transmit_with_fec_mode(&payload, MODE, FecMode::Rs, None)
            .expect("tx");
        b.drain_samples()
    };
    let tx_rms = (tx.iter().map(|s| s * s).sum::<f32>() / tx.len() as f32).sqrt();
    let sigma09_db = 20.0 * (tx_rms / 0.9).log10();
    println!("\nPAIRED-AB-1428 {SEEDS} seeds {MODE} 200 B; tx len {} rms {tx_rms:.4}; sigma0.9 = {sigma09_db:.2} dB", tx.len());
    println!("  (since #1428: \"hard\" = the UNION of both arms, not the cancelled arm alone)");
    println!("  cell                       | soft | hard | diff | both | S-only | H-only | neither | McNemar p");

    let cells: Vec<Cell> = vec![
        ("moderate_f1 1ms @ 8 dB".into(), {
            let tx = tx.clone();
            Box::new(move |s| {
                let mut c = WattersonConfig::moderate_f1(Some(s));
                c.snr_db = 8.0;
                WattersonChannel::new(c).expect("w").apply(&tx)
            })
        }),
        ("moderate_f1 1ms @ 12 dB".into(), {
            let tx = tx.clone();
            Box::new(move |s| {
                let mut c = WattersonConfig::moderate_f1(Some(s));
                c.snr_db = 12.0;
                WattersonChannel::new(c).expect("w").apply(&tx)
            })
        }),
        ("doppler-only 0ms @ 8 dB".into(), {
            let tx = tx.clone();
            Box::new(move |s| {
                let mut c = WattersonConfig::moderate_f1(Some(s));
                c.snr_db = 8.0;
                c.delay_spread_ms = 0.0;
                WattersonChannel::new(c).expect("w").apply(&tx)
            })
        }),
        ("awgn @ -2 dB".into(), {
            let tx = tx.clone();
            Box::new(move |s| {
                AwgnChannel::new(AwgnConfig::new(-2.0, Some(s)))
                    .expect("a")
                    .apply(&tx)
            })
        }),
        ("awgn @ -1 dB".into(), {
            let tx = tx.clone();
            Box::new(move |s| {
                AwgnChannel::new(AwgnConfig::new(-1.0, Some(s)))
                    .expect("a")
                    .apply(&tx)
            })
        }),
        ("awgn @ 0 dB".into(), {
            let tx = tx.clone();
            Box::new(move |s| {
                AwgnChannel::new(AwgnConfig::new(0.0, Some(s)))
                    .expect("a")
                    .apply(&tx)
            })
        }),
        ("awgn @ 2 dB".into(), {
            let tx = tx.clone();
            Box::new(move |s| {
                AwgnChannel::new(AwgnConfig::new(2.0, Some(s)))
                    .expect("a")
                    .apply(&tx)
            })
        }),
        ("awgn @ 5 dB".into(), {
            let tx = tx.clone();
            Box::new(move |s| {
                AwgnChannel::new(AwgnConfig::new(5.0, Some(s)))
                    .expect("a")
                    .apply(&tx)
            })
        }),
        (format!("awgn sigma0.9 @ {sigma09_db:.1} dB"), {
            let tx = tx.clone();
            Box::new(move |s| {
                AwgnChannel::new(AwgnConfig::new(sigma09_db, Some(s)))
                    .expect("a")
                    .apply(&tx)
            })
        }),
    ];

    for (name, chan) in cells {
        let mut pattern = String::with_capacity(SEEDS as usize);
        let (mut both, mut s_only, mut h_only, mut neither) = (0u64, 0u64, 0u64, 0u64);
        for seed in 0..SEEDS {
            let rx = chan(seed);
            let soft = {
                let (mut e, b) = engine();
                assert_eq!(
                    e.afc_correction_hz(),
                    0.0,
                    "fresh engine must start at afc 0"
                );
                b.push_frame(&rx);
                e.receive_with_llr_combining(MODE, None, 1)
                    .map(|d| d == payload)
                    .unwrap_or(false)
            };
            let hard = {
                let (mut e, b) = engine();
                assert_eq!(
                    e.afc_correction_hz(),
                    0.0,
                    "fresh engine must start at afc 0"
                );
                b.push_frame(&rx);
                e.receive_with_fec_mode(MODE, FecMode::Rs, None)
                    .map(|d| d == payload)
                    .unwrap_or(false)
            };
            match (soft, hard) {
                (true, true) => {
                    both += 1;
                    pattern.push('B');
                }
                (true, false) => {
                    s_only += 1;
                    pattern.push('S');
                }
                (false, true) => {
                    h_only += 1;
                    pattern.push('H');
                }
                (false, false) => {
                    neither += 1;
                    pattern.push('.');
                }
            }
        }
        let soft = both + s_only;
        let hard = both + h_only;
        println!(
            "  {name:26} | {soft:4} | {hard:4} | {:+4} | {both:4} | {s_only:6} | {h_only:6} | {neither:7} | {:.4}",
            soft as i64 - hard as i64,
            mcnemar_p(s_only, h_only)
        );
        println!("      {pattern}");
    }
}
