//! Every plugin's `demodulate_variants` obeys the union contract — swept, not listed (#1428).
//!
//! The engine's hard-decode chains try each variant in turn and keep the first whose FEC and frame
//! decode succeed. Three things must hold for that to be sound, and all three are checked for
//! **every mode every registered plugin claims**, because a hand-written list of (plugin, mode)
//! pairs is the inventory-rot shape this repo has paid for repeatedly.
//!
//! - **I1 — variant 0 IS `demodulate`.** The rest of the trait contract hangs off `demodulate`
//!   (`soft_demod_conformance` checks the soft arm against it), so variant 0 must keep meaning it.
//!   Checked on a clean AND a noisy input, because a plugin could agree on one and not the other.
//! - **I2 — no duplicate variants, under noise.** A duplicate costs a wasted FEC trial on every
//!   onset of the scanning receive and can be counted as a second arm contributing when it cannot.
//!   **Under noise specifically**: BPSK's two arms are byte-identical on a clean fixture — the
//!   crossfade bias flips no decision without noise — so a clean-input version of this assertion
//!   fails for a reason that is not a defect. That is the useful form of the fact: *an invariant
//!   about two arms differing is only meaningful where they are meant to differ.*
//! - **I3 — a single-variant plugin really has one arm.** For a plugin declaring one variant,
//!   `hard_decide(demodulate_soft)` must reproduce `demodulate()` on a NOISY input. This is the
//!   measurement, not an assumption: OFDM and SC-FDMA carry separate soft and hard implementations,
//!   and nothing previously pinned them sign-equivalent under noise — `soft_demod_conformance`'s
//!   implication (B) is noiseless by its own doc. **A failure here is a finding — an undeclared
//!   second arm — not a bug in this test.**
//!
//! What this does NOT cover: the GPU variants paths. `Plugin::new()` is the CPU constructor while
//! the daemon builds `with_gpu` by default, and this gate runs `--no-default-features`. That gap is
//! exactly how #1433 survived 71 days, and `plugins/bpsk/tests/gpu_cpu_equivalence.rs` is the
//! only thing covering it.

use openpulse_core::fec::hard_decide;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

/// Modes that cannot be driven at 8 kHz; mirrored from `soft_demod_conformance`'s own list so the
/// two sweeps agree about what is undrivable rather than each maintaining a private opinion.
fn undrivable(mode: &str) -> bool {
    let src = include_str!("soft_demod_conformance.rs");
    let list = src
        .split("const UNDRIVABLE_AT_8K")
        .nth(1)
        .and_then(|s| s.split('[').nth(1))
        .and_then(|s| s.split(']').next())
        .unwrap_or("");
    list.split(',')
        .map(|s| s.trim().trim_matches('"'))
        .any(|m| !m.is_empty() && m == mode)
}

fn config_for(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.to_string(),
        ..ModulationConfig::default()
    }
}

/// Deterministic AWGN at a given total-power SNR. Box-Muller over an LCG.
fn awgn(signal: &[f32], snr_db: f32, seed: u64) -> Vec<f32> {
    let p: f32 = signal.iter().map(|s| s * s).sum::<f32>() / signal.len().max(1) as f32;
    let sigma = (p / 10f32.powf(snr_db / 10.0)).sqrt();
    let mut st = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    let mut u = || -> f32 {
        st = st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((st >> 11) as f32 / (1u64 << 53) as f32).clamp(1e-9, 1.0 - 1e-9)
    };
    signal
        .iter()
        .map(|&s| {
            let (a, b) = (u(), u());
            s + sigma * (-2.0 * a.ln()).sqrt() * (std::f32::consts::TAU * b).cos()
        })
        .collect()
}

fn plugins() -> Vec<Box<dyn ModulationPlugin>> {
    vec![
        Box::new(bpsk_plugin::BpskPlugin::new()),
        Box::new(fsk4_plugin::Fsk4Plugin::new()),
        Box::new(mfsk16_plugin::Mfsk16Plugin::new()),
        Box::new(ofdm_plugin::OfdmPlugin::new()),
        Box::new(psk8_plugin::Psk8Plugin::new()),
        Box::new(qam64_plugin::Qam64Plugin::new()),
        Box::new(qpsk_plugin::QpskPlugin::new()),
        Box::new(scfdma_plugin::ScFdmaPlugin::new()),
        Box::new(pilot_plugin::PilotPlugin::new()),
    ]
}

/// Per-mode check. Returns the reason on failure so a sabotage test can require one.
fn check_mode(plugin: &dyn ModulationPlugin, mode: &str) -> Result<bool, String> {
    let cfg = config_for(mode);
    let payload: Vec<u8> = (0..96u32)
        .map(|i| (i.wrapping_mul(131) >> 2) as u8)
        .collect();
    let Ok(tx) = plugin.modulate(&payload, &cfg) else {
        return Ok(false);
    };
    if tx.is_empty() {
        return Ok(false);
    }

    // The SNRs at which a declared second arm must prove itself, as a LADDER rather than a point.
    //
    // A single absolute SNR cannot work for a registry sweep, because each mode's arms diverge at
    // its own operating point and processing gain spans ~9 dB across this registry alone. Measured
    // while writing this test: at 6 dB total power BPSK250 sits near 21 dB Eb/N0 and its two arms
    // are byte-identical; BPSK31, at 256 samples/symbol (~24 dB of gain), is still identical at
    // −6 dB. The ladder therefore runs down to where any mode here is unusable, and a mode that
    // fails to demodulate at the bottom rungs simply contributes nothing — `demodulate_variants`
    // returning `Err` is skipped, not counted as agreement.
    const ARM_SNRS: [f32; 5] = [0.0, -6.0, -12.0, -18.0, -24.0];

    if let Ok(v) = plugin.demodulate_variants(&tx, &cfg) {
        if v.len() > 1 {
            let mut ever_differed = false;
            for snr in ARM_SNRS {
                for seed in 0..4u64 {
                    if let Ok(vs) = plugin.demodulate_variants(&awgn(&tx, snr, seed), &cfg) {
                        if vs.len() > 1 && vs.windows(2).any(|w| w[0] != w[1]) {
                            ever_differed = true;
                        }
                    }
                }
            }
            if !ever_differed {
                return Err(format!(
                    "{mode}: declares {} arms, but they produce identical bytes at every tested \
                     SNR — a duplicate costs an FEC trial per onset and cannot rescue a frame",
                    v.len()
                ));
            }
        }
    }

    for (label, rx) in [("clean", tx.clone()), ("noisy", awgn(&tx, 6.0, 11))] {
        let Ok(variants) = plugin.demodulate_variants(&rx, &cfg) else {
            continue;
        };
        if variants.is_empty() {
            return Err(format!("{mode}: demodulate_variants returned no variants"));
        }
        let Ok(shipped) = plugin.demodulate(&rx, &cfg) else {
            continue;
        };

        // I1
        if variants[0] != shipped {
            return Err(format!(
                "{mode} ({label}): variant 0 is not demodulate()'s bytes — the trait contract \
                 hangs off demodulate, so variant 0 must reproduce it"
            ));
        }

        // I3, noisy only: a plugin claiming ONE arm must not have a second one hiding in its
        // soft path. A failure is an undeclared arm, i.e. a finding about that plugin.
        if label == "noisy" && variants.len() == 1 {
            if let Ok(llrs) = plugin.demodulate_soft(&rx, &cfg) {
                let decided = hard_decide(&llrs);
                // Compare DECISIONS over the common prefix, not `Vec` equality. The two paths can
                // legitimately return different LENGTHS at low SNR — measured, OFDM52-8PSK at 6 dB
                // returns 111 B hard against 96 B soft-decided with ZERO differing bits — and a
                // bare `!=` reports that as a decision divergence. An earlier draft of this test
                // did exactly that and produced four false "undeclared second arm" findings.
                let n = decided.len().min(shipped.len());
                let differing: u32 = decided[..n]
                    .iter()
                    .zip(&shipped[..n])
                    .map(|(a, b)| (a ^ b).count_ones())
                    .sum();
                if n >= 32 && differing > 0 {
                    return Err(format!(
                        "{mode}: declares ONE hard arm, but hard-deciding demodulate_soft \
                         disagrees with demodulate() on {differing} bit(s) of the first {n} \
                         bytes under noise — that is an undeclared second arm, and the union \
                         could be using it. Investigate the plugin, not this test."
                    ));
                }
            }
        }
    }
    Ok(true)
}

#[test]
fn every_plugin_obeys_the_hard_variant_contract() {
    let ps = plugins();
    let (mut checked, mut failures) = (0usize, Vec::new());
    for p in &ps {
        for mode in &p.info().supported_modes {
            if undrivable(mode) {
                continue;
            }
            match check_mode(p.as_ref(), mode) {
                Ok(true) => checked += 1,
                Ok(false) => {}
                Err(e) => failures.push(e),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} mode(s) break the hard-variant contract:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
    assert!(
        checked >= 40,
        "only {checked} modes were actually exercised; the sweep has gone mostly vacuous"
    );
    println!(
        "hard-variant contract: {checked} modes checked across {} plugins",
        ps.len()
    );
}

/// BPSK must actually DECLARE two arms — otherwise the sweep above passes on a build where the
/// union was never wired, which is the state this whole change exists to leave.
#[test]
fn bpsk_declares_its_second_arm() {
    let p = bpsk_plugin::BpskPlugin::new();
    let cfg = config_for("BPSK250");
    let payload: Vec<u8> = (0..96u32)
        .map(|i| (i.wrapping_mul(131) >> 2) as u8)
        .collect();
    let tx = p.modulate(&payload, &cfg).expect("modulate");
    let v = p
        .demodulate_variants(&awgn(&tx, 0.0, 3), &cfg)
        .expect("variants");
    assert_eq!(
        v.len(),
        2,
        "BPSK250 must offer the cancelled and uncancelled arms (#1428); it offered {}",
        v.len()
    );
}
