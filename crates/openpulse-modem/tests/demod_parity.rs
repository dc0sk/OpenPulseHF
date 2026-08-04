//! #1062 vetting column 4 — demod parity for a candidate preamble sequence.
//!
//! #1062 proposes replacing the BPSK preamble (32 bits alternating; NRZI makes
//! the symbols `--++` repeating, period 4) with an aperiodic sync word. Three
//! vetting columns already exist — noise ceiling, decode edge, grid half-width.
//! This is the fourth: **does a candidate still serve the preamble's other
//! jobs?** It gates every candidate, and it decides whether BPSK31/63 change at
//! all, since those rungs gain nothing on the acquisition path (they publish no
//! template) and are justified only by uniformity or by a parity gain measured
//! here.
//!
//! ## What is instrumented, and what is deliberately not
//!
//! | column | instrument | note |
//! |---|---|---|
//! | A timing | the real `find_timing_offset_with_expected` | error **rate**, not a point error |
//! | B coarse AFC | the shipped `afc_estimate_hz` (stage 1 is expectation-blind) | lock-error rate |
//! | C fine AFC | `afc_estimate_hz_with_expected` | stage 2 locks timing, so it needs the matched expectation |
//! | D self-ambiguity | `IqMatchedFilter::search_normalized` over whole-symbol lags | the property #1062 exists to fix |
//! | E end-to-end | `bpsk_modulate_with_preamble` → channel → `bpsk_demodulate_with_expected` | the deciding column |
//!
//! Two things are **not** measured and must not be inferred from a pass here:
//! the `-RRC` path (its timing is Gardner+LMS, so a candidate reaches it through
//! training, which this seam does not cover), and any **fixed-duration**
//! candidate at the slow rungs — a 110 ms sync word is 3.4 symbols at 31.25 baud,
//! so it cannot be a symbol-rate sequence swap at all and the shipped estimators
//! would be restructured rather than re-parameterised. For those, this harness
//! reports "not representable" rather than a number.
//!
//! Rung floors come from `SessionProfile::hpx_hf` by reference, never
//! transcribed (verification rule 5).

use bpsk_plugin::demodulate::{
    afc_estimate_hz, afc_estimate_hz_with_expected, bpsk_demodulate_with_expected,
    expected_preamble_symbols, expected_symbols_for, find_timing_offset_with_expected,
};
use bpsk_plugin::modulate::{bpsk_modulate_with_preamble, preamble_bits, PREAMBLE_SYMS};
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::plugin::{ModulationConfig, PulseShape};
use openpulse_dsp::acquisition::IqMatchedFilter;

const FS: f32 = 8000.0;
const FC: f32 = 1500.0;

fn config(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.to_string(),
        sample_rate: FS as u32,
        center_frequency: FC,
        pulse_shape: PulseShape::Hann,
        ..Default::default()
    }
}

fn baud_of(mode: &str) -> f32 {
    match mode {
        "BPSK31" => 31.25,
        "BPSK63" => 62.5,
        "BPSK100" => 100.0,
        "BPSK250" => 250.0,
        _ => panic!("unhandled mode {mode}"),
    }
}

fn sps(mode: &str) -> usize {
    (FS / baud_of(mode)).round() as usize
}

// ── Candidate sequences ───────────────────────────────────────────────────────

/// Convert a target ±1 symbol sequence into the preamble *bits* that NRZI
/// encodes into it.
///
/// The modulator NRZI-encodes its preamble bits, so a candidate expressed as
/// symbols has to be inverted through NRZI before it can be transmitted. Pinned
/// by `nrzi_inversion_round_trips`.
fn symbols_to_bits(symbols: &[f32]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(symbols.len());
    let mut prev = 1.0f32; // nrzi_encode starts from a non-negated state
    for &s in symbols {
        bits.push(s != prev);
        prev = s;
    }
    bits
}

/// A candidate preamble, named for reporting.
struct Candidate {
    name: &'static str,
    symbols: Vec<f32>,
}

/// Cycle a base chip sequence up to `len`, the same way `PreambleSpec::chips` does.
fn cycled(base: &[f32], len: usize) -> Vec<f32> {
    base.iter().copied().cycle().take(len).collect()
}

fn candidates(len: usize) -> Vec<Candidate> {
    use openpulse_dsp::preamble::{PreambleConstellation, PreambleSpec, PreambleType};

    let chips = |t: PreambleType| -> Vec<f32> {
        PreambleSpec::new(t, len, PreambleConstellation::Bpsk)
            .chips()
            .to_vec()
    };

    // A balanced pseudorandom control: equal +1/-1 counts, so its transition
    // density matches the shipped sequence's and the squared-tone energy that
    // coarse AFC reads is held constant (measured: transition count drives that
    // energy with r = -0.997, and the shipped sequence sits at the 54th
    // percentile of random sequences — it is not advantaged).
    let balanced = {
        let mut v: Vec<f32> = (0..len)
            .map(|i| if i < len / 2 { 1.0 } else { -1.0 })
            .collect();
        // Deterministic Fisher-Yates with a fixed multiplier; no Date/rand.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        for i in (1..v.len()).rev() {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let j = (state >> 33) as usize % (i + 1);
            v.swap(i, j);
        }
        v
    };

    vec![
        Candidate {
            name: "shipped(--++)",
            symbols: expected_preamble_symbols(len),
        },
        Candidate {
            name: "pn31",
            symbols: cycled(&chips(PreambleType::Pn31), len),
        },
        Candidate {
            name: "barker13",
            symbols: cycled(&chips(PreambleType::Barker13), len),
        },
        Candidate {
            name: "balanced-rand",
            symbols: balanced,
        },
    ]
}

// ── Channel helpers ───────────────────────────────────────────────────────────

/// Deterministic Gaussian noise at a given SNR relative to `signal`.
///
/// Box-Muller rather than a uniform difference: an earlier probe in this series
/// shipped a uniform generator with a DC bias that shifted every measured ρ.
fn add_awgn(signal: &[f32], snr_db: f32, seed: u64) -> Vec<f32> {
    let sig_pow: f32 = signal.iter().map(|s| s * s).sum::<f32>() / signal.len().max(1) as f32;
    let noise_pow = sig_pow / 10f32.powf(snr_db / 10.0);
    let sigma = noise_pow.sqrt();
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    let mut next_unit = || -> f32 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 11) as f32 / (1u64 << 53) as f32).clamp(1e-9, 1.0 - 1e-9)
    };
    signal
        .iter()
        .map(|&s| {
            let (u1, u2) = (next_unit(), next_unit());
            let g = (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos();
            s + sigma * g
        })
        .collect()
}

// ── Column A: timing ──────────────────────────────────────────────────────────

/// Timing-lock error rate: fraction of trials whose recovered sub-symbol offset
/// is more than `tol` samples from the truth.
fn timing_error_rate(mode: &str, cand: &Candidate, snr_db: f32, trials: u32) -> f32 {
    let cfg = config(mode);
    let n = sps(mode);
    let bits = symbols_to_bits(&cand.symbols);
    let expected = expected_symbols_for(&bits);
    let tol = (n as f32 * 0.10).max(1.0) as usize;
    let mut bad = 0u32;
    for t in 0..trials {
        let Ok(tx) = bpsk_modulate_with_preamble(b"parity", &cfg, &bits) else {
            return f32::NAN;
        };
        let rx = add_awgn(&tx, snr_db, 700 + t as u64);
        let off = find_timing_offset_with_expected(&rx, n, FC, FS, &expected);
        // The frame starts at sample 0, so the correct sub-symbol offset is 0;
        // wrap-around means n-1 is one sample early, not n-1 samples late.
        let err = off.min(n - off);
        if err > tol {
            bad += 1;
        }
    }
    bad as f32 / trials as f32
}

// ── Column B/C: AFC ───────────────────────────────────────────────────────────

/// AFC lock-error rate over true offsets × noise seeds.
///
/// `matched = false` routes through the shipped `afc_estimate_hz`, whose stage 2
/// locks timing against the *shipped* expectation — so on candidate audio it
/// measures a TX/RX mismatch. `matched = true` supplies the candidate's own
/// expectation, which is what a post-change receiver would have.
fn afc_error_rate(mode: &str, cand: &Candidate, snr_db: f32, matched: bool) -> (f32, f32) {
    let n = sps(mode);
    let bits = symbols_to_bits(&cand.symbols);
    let expected = expected_symbols_for(&bits);
    // Tolerance: the coarse stage quantises to 12.5 Hz, so anything inside one
    // step is a successful lock for a stage-1 measurement.
    let tol_hz = 12.5f32;
    let mut bad = 0u32;
    let mut total = 0u32;
    let mut worst = 0.0f32;
    for &true_off in &[-300.0f32, -120.0, -40.0, 0.0, 40.0, 120.0, 300.0] {
        for seed in 0..6u64 {
            let mut cfg = config(mode);
            cfg.center_frequency = FC + true_off;
            let Ok(tx) = bpsk_modulate_with_preamble(b"parity", &cfg, &bits) else {
                continue;
            };
            let rx = add_awgn(&tx, snr_db, 4200 + seed);
            // The engine hands the settle `preamble + 1 symbol`, not a whole
            // frame — a full frame would dilute the preamble to ~1/4 of the
            // scan window and understate any sequence effect ~4x.
            let win = (n * (expected.len() + 1)).min(rx.len());
            let probe = config(mode); // estimator is told the nominal centre
            let est = if matched {
                afc_estimate_hz_with_expected(&rx[..win], &probe, &expected)
            } else {
                afc_estimate_hz(&rx[..win], &probe)
            };
            total += 1;
            match est {
                Some(e) => {
                    let err = (e - true_off).abs();
                    worst = worst.max(err);
                    if err > tol_hz {
                        bad += 1;
                    }
                }
                None => bad += 1,
            }
        }
    }
    (bad as f32 / total.max(1) as f32, worst)
}

// ── Column D: self-ambiguity ──────────────────────────────────────────────────

/// Peak-to-sidelobe of the candidate's own template, over **whole-symbol lags**.
///
/// This is the property #1062 exists to fix. The shipped sequence is periodic
/// with period 4, so its autocorrelation has near-full peaks at every 4-symbol
/// lag — which is what made onset-snapping pick an offset two symbol periods
/// late and decode to "invalid magic" (#1049 point 3). `find_timing_offset`
/// never experiences this, because it only searches sub-symbol offsets; the
/// engine's onset placement does.
fn self_ambiguity(mode: &str, cand: &Candidate) -> f32 {
    let cfg = config(mode);
    let n = sps(mode);
    let bits = symbols_to_bits(&cand.symbols);
    // A payload long enough that the search has somewhere else to land. The
    // first version of this correlated the template against shifted copies of
    // ITSELF, so every shifted slice was shorter than the template and
    // `search_normalized` returned None for all of them — it reported 0.000 for
    // the period-4 shipped sequence, which is the one case that must be high.
    let Ok(frame) = bpsk_modulate_with_preamble(b"AAAAAAAAAAAAAAAA", &cfg, &bits) else {
        return f32::NAN;
    };
    let span = n * (cand.symbols.len() - 1);
    if frame.len() < span * 2 {
        return f32::NAN;
    }
    let template = frame[..span].to_vec();
    let mf = IqMatchedFilter::new(template);
    let peak = match mf.search_normalized(&frame, 0, 0.0) {
        Some(r) => r.rho,
        None => return f32::NAN,
    };
    // Worst ρ at any offset at least one symbol away from the true onset: the
    // self-alias that made onset-snapping choose a whole-symbol-late offset and
    // decode to "invalid magic" (#1049 point 3).
    let mut worst = 0.0f32;
    for lag in n..(frame.len().saturating_sub(span)) {
        if let Some(r) = mf.search_normalized(&frame[lag..], 0, 0.0) {
            worst = worst.max(r.rho);
        }
    }
    if peak.is_finite() && peak > 0.0 {
        worst / peak
    } else {
        f32::NAN
    }
}

// ── Column E: end-to-end ──────────────────────────────────────────────────────

/// Uncoded end-to-end decode rate through a channel.
///
/// **Limit, stated rather than hidden:** `bpsk_demodulate_with_expected` assumes
/// the frame begins at sample 0 (it searches sub-symbol offsets only), so this
/// is a buffer-is-the-frame fixture — the easiest case that exists. Frame
/// *location* is the engine's job and is not exercised here.
fn decode_rate(mode: &str, cand: &Candidate, snr_db: f32, fade: bool, trials: u32) -> f32 {
    let cfg = config(mode);
    let bits = symbols_to_bits(&cand.symbols);
    let expected = expected_symbols_for(&bits);
    let payload = b"OPENPULSE demod parity";
    let mut ok = 0u32;
    for t in 0..trials {
        let Ok(tx) = bpsk_modulate_with_preamble(payload, &cfg, &bits) else {
            continue;
        };
        let rx = if fade {
            let mut wc = WattersonConfig::moderate_f1(Some(9100 + t as u64));
            wc.snr_db = snr_db;
            match WattersonChannel::new(wc) {
                Ok(mut ch) => ch.apply(&tx),
                Err(_) => continue,
            }
        } else {
            add_awgn(&tx, snr_db, 5500 + t as u64)
        };
        if let Ok(bytes) = bpsk_demodulate_with_expected(&rx, &cfg, &expected) {
            if bytes.len() >= payload.len() && &bytes[..payload.len()] == payload {
                ok += 1;
            }
        }
    }
    ok as f32 / trials as f32
}

// ── Shipped gates ─────────────────────────────────────────────────────────────

#[test]
fn nrzi_inversion_round_trips() {
    // The harness's own apparatus check: `symbols_to_bits` must be the exact
    // inverse of the modulator's NRZI encoding, or every candidate measurement
    // is of a different sequence than the one named.
    for len in [8usize, 16, PREAMBLE_SYMS] {
        for cand in candidates(len) {
            let bits = symbols_to_bits(&cand.symbols);
            let round = expected_symbols_for(&bits);
            assert_eq!(
                round, cand.symbols,
                "{}: symbols_to_bits is not the NRZI inverse at len {len}",
                cand.name
            );
        }
    }
}

#[test]
fn the_shipped_sequence_is_recovered_as_a_candidate() {
    // Anti-vacuity: the control candidate must be byte-identical to the shipped
    // preamble, so a parity difference is attributable to the sequence and not
    // to the harness taking a different path for candidates than for the control.
    let c = candidates(PREAMBLE_SYMS);
    let shipped = c
        .iter()
        .find(|c| c.name == "shipped(--++)")
        .expect("control");
    assert_eq!(
        symbols_to_bits(&shipped.symbols),
        preamble_bits(PREAMBLE_SYMS)
    );
}

// ── Research harnesses (ignored: they print tables, they do not gate) ─────────

#[test]
#[ignore = "research harness: prints the parity table"]
fn parity_table() {
    let modes = ["BPSK31", "BPSK250"];
    println!();
    println!("=== Column A: timing-lock error rate (10% of a symbol period tolerated) ===");
    for mode in modes {
        // -18 dB is the positive control: if timing never fails even there,
        // the metric is inert and its zeros mean nothing.
        for snr in [-18.0f32, -6.0, 0.0, 6.0, 12.0] {
            print!("{mode:8} {snr:5.1} dB |");
            for cand in candidates(PREAMBLE_SYMS) {
                let r = timing_error_rate(mode, &cand, snr, 24);
                print!(" {:>14}={:5.2}", cand.name, r);
            }
            println!();
        }
    }

    println!();
    println!("=== Column B/C: AFC lock-error rate (tol 12.5 Hz = one coarse step) ===");
    println!("    'shipped-rx' routes candidate audio through the SHIPPED expectation");
    println!("    (a TX/RX mismatch, not the candidate); 'matched-rx' is the real column.");
    for mode in modes {
        for cand in candidates(PREAMBLE_SYMS) {
            let (mis, mis_worst) = afc_error_rate(mode, &cand, 12.0, false);
            let (mat, mat_worst) = afc_error_rate(mode, &cand, 12.0, true);
            // Transition count is reported because it is the proposed mechanism:
            // the Goertzel stage squares, so any BPSK sequence collapses to a
            // tone at 2*fc whose energy is set by how often the envelope dips at
            // a symbol flip. Printing it turns "the sequence matters" into a
            // testable claim rather than a story fitted after the fact.
            let trans = cand.symbols.windows(2).filter(|w| w[0] != w[1]).count();
            println!(
                "{mode:8} {:>14} (trans {trans:2}) | shipped-rx {:5.2} (worst {:7.1} Hz)  matched-rx {:5.2} (worst {:7.1} Hz)",
                cand.name, mis, mis_worst, mat, mat_worst
            );
        }
    }

    println!();
    println!("=== Column D: self-ambiguity — worst whole-symbol-lag rho / peak ===");
    println!("    Lower is better. The shipped sequence is period-4, so it should be high.");
    for mode in modes {
        for cand in candidates(PREAMBLE_SYMS) {
            println!(
                "{mode:8} {:>14} | {:6.3}",
                cand.name,
                self_ambiguity(mode, &cand)
            );
        }
    }

    println!();
    println!("=== Column E: uncoded end-to-end decode rate ===");
    for mode in modes {
        for cand in candidates(PREAMBLE_SYMS) {
            let awgn = decode_rate(mode, &cand, 8.0, false, 32);
            let fade = decode_rate(mode, &cand, 20.0, true, 96);
            println!(
                "{mode:8} {:>14} | AWGN 8 dB {:5.2}   moderate_f1 20 dB {:5.2}",
                cand.name, awgn, fade
            );
        }
    }
    println!();
}

#[test]
fn the_shipped_preamble_self_aliases_completely_and_a_pn_sequence_does_not() {
    // The measured reason #1062 exists, pinned so it cannot quietly change.
    //
    // The shipped sequence is period-4, so its own template matches a
    // whole-symbol-shifted copy of itself just as well as it matches the truth —
    // ratio 1.000. That is the mechanism behind #1049 point 3, where snapping the
    // onset to the correlation argmax chose an offset two symbol periods late and
    // decoded to "invalid magic", and it is why correct onset placement needs a
    // sequence whose autocorrelation is not periodic.
    for mode in ["BPSK31", "BPSK250"] {
        let c = candidates(PREAMBLE_SYMS);
        let shipped = c
            .iter()
            .find(|c| c.name == "shipped(--++)")
            .expect("control");
        let pn = c.iter().find(|c| c.name == "pn31").expect("pn31");
        let s_amb = self_ambiguity(mode, shipped);
        let p_amb = self_ambiguity(mode, pn);
        assert!(
            s_amb > 0.9,
            "{mode}: shipped preamble self-ambiguity {s_amb:.3} — expected ~1.0 for a period-4 \
             sequence; if this dropped, the premise of #1062's onset-placement argument changed"
        );
        assert!(
            p_amb < 0.5,
            "{mode}: pn31 self-ambiguity {p_amb:.3} — expected well below the shipped 1.0; an \
             aperiodic candidate that self-aliases would not fix onset placement"
        );
    }
}
