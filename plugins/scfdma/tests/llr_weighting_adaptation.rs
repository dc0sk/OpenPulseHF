use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use scfdma_plugin::demodulate::{combine_llrs_weighted, scfdma_demodulate_soft_with_metrics};
use scfdma_plugin::ScFdmaPlugin;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        sample_rate: 8000,
        center_frequency: 1500.0,
        ..ModulationConfig::default()
    }
}

fn bytes_to_bits(bytes: &[u8]) -> Vec<bool> {
    bytes
        .iter()
        .flat_map(|b| (0..8).map(move |i| (b >> i) & 1 == 1))
        .collect()
}

fn bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8)
        .map(|c| {
            c.iter()
                .enumerate()
                .fold(0u8, |acc, (i, &b)| acc | ((b as u8) << i))
        })
        .collect()
}

fn llrs_to_payload_bytes(llrs: &[f32], payload_len: usize) -> Option<Vec<u8>> {
    let need = payload_len.saturating_mul(8);
    if llrs.len() < need {
        return None;
    }
    let bits: Vec<bool> = llrs[..need].iter().map(|v| v.is_sign_negative()).collect();
    Some(bits_to_bytes(&bits)[..payload_len].to_vec())
}

fn gaussian_noise_iter(seed: u64, count: usize) -> impl Iterator<Item = f32> {
    let mut state = seed;
    std::iter::from_fn(move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let u1 = ((state >> 11) as f64) * (1.0 / ((1u64 << 53) as f64));
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let u2 = ((state >> 11) as f64) * (1.0 / ((1u64 << 53) as f64));
        let u1 = u1.clamp(1e-12, 1.0 - 1e-12);
        Some(((-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()) as f32)
    })
    .take(count)
}

fn add_awgn(samples: &[f32], snr_db: f32, seed: u64) -> Vec<f32> {
    let signal_power = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    let snr_linear = 10.0f32.powf(snr_db / 10.0);
    let noise_std = (signal_power / snr_linear).sqrt();
    gaussian_noise_iter(seed, samples.len())
        .zip(samples.iter())
        .map(|(n, &s)| s + noise_std * n)
        .collect()
}

/// Mean decision-residual σ̂², mean pilot σ̂², and the mean noise power actually injected, over
/// `frames` AWGN realisations at `snr_db`.
///
/// The realised noise power is measured rather than assumed: `add_awgn`'s Box–Muller draws its two
/// uniforms from consecutive states of a plain LCG, so a realisation's power — and its spectrum, which
/// is what a pilot-bin estimator sees — departs from the nominal by several percent. Callers must also
/// pass the *same* `seed_base` at both SNRs, so the two runs share one noise shape and differ only in
/// scale; otherwise the shape difference alone moves the ratio by ~0.5 dB.
fn mean_noise_metrics(
    tx: &[f32],
    mode: &str,
    snr_db: f32,
    seed_base: u64,
    frames: usize,
) -> (f32, f32, f32) {
    let (mut decision, mut pilot, mut injected) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..frames {
        let rx = add_awgn(tx, snr_db, seed_base + i as u64);
        injected += rx
            .iter()
            .zip(tx.iter())
            .map(|(r, t)| (r - t) * (r - t))
            .sum::<f32>()
            / tx.len() as f32;
        let m = scfdma_demodulate_soft_with_metrics(&rx, mode)
            .unwrap_or_else(|e| panic!("soft demod at {snr_db} dB: {e}"));
        decision += m.metrics.mean_noise_var;
        pilot += m.metrics.mean_pilot_noise_var;
    }
    let n = frames as f32;
    (decision / n, pilot / n, injected / n)
}

/// The pilot-derived σ̂² is a *direct* noise-power measurement — no constellation, no channel estimate —
/// so `σ̂² / (injected noise power)` must be the same constant at every SNR. This is the invariant the
/// whole LLR scale rests on: `symbol_llrs` divides by σ̂².
///
/// The constant itself is not 1: σ̂² is the smaller of two estimators that fail in opposite directions,
/// and taking the minimum of two unbiased estimates biases the result low by a fixed fraction.
#[test]
fn pilot_noise_variance_is_proportional_to_injected_noise_power() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0..96).map(|v| v as u8).collect();
    let mode = "SCFDMA52-64QAM-P4";
    let tx = plugin.modulate(&payload, &cfg(mode)).unwrap();
    let frames = 20usize;

    let (_, pilot20, injected20) = mean_noise_metrics(&tx, mode, 20.0, 0x1000, frames);
    let (_, pilot28, injected28) = mean_noise_metrics(&tx, mode, 28.0, 0x1000, frames);

    let scale20 = pilot20 / injected20;
    let scale28 = pilot28 / injected28;
    let drift_db = 10.0 * (scale20 / scale28).log10();
    assert!(
        drift_db.abs() <= 0.3,
        "pilot σ̂² must be linear in noise power across 8 dB: σ̂²/σ² was {scale20:.4} at 20 dB and {scale28:.4} at 28 dB ({drift_db:+.2} dB drift)"
    );
}

/// `mean_noise_var` is a distance-to-nearest-symbol residual, so it can only ever *under*-report a
/// change in noise power: once symbol errors are common the residual is clipped by the Voronoi cell
/// (d²min/6 = 0.0159 for 64QAM). And a Wiener channel estimate's MSE is sub-linear in σ² by
/// construction — its ridge shrinks with the noise. Both effects bound the measurement strictly below
/// the 8 dB of applied noise change; only the upper bound and monotonicity are receiver properties.
///
/// An earlier `8.0 ± 0.75 dB` assertion here was passing only because the DFT-CE it was calibrated
/// against had an MSE strictly proportional to σ² (a fixed tap truncation, no SNR-dependent shrinkage).
#[test]
fn decision_noise_variance_tracks_awgn_monotonically_without_over_reporting() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0..96).map(|v| v as u8).collect();
    let mode = "SCFDMA52-64QAM-P4";
    let tx = plugin.modulate(&payload, &cfg(mode)).unwrap();
    let frames = 20usize;

    let (decision20, _, injected20) = mean_noise_metrics(&tx, mode, 20.0, 0x1000, frames);
    let (decision28, _, injected28) = mean_noise_metrics(&tx, mode, 28.0, 0x1000, frames);

    // Compare against the noise actually injected, not the nominal 8 dB (see `mean_noise_metrics`).
    let applied_db = 10.0 * (injected20 / injected28).log10();
    let measured_delta_db = 10.0 * (decision20 / decision28).log10();
    assert!(
        measured_delta_db > 5.0 && measured_delta_db <= applied_db + 0.3,
        "decision-residual noise must track {applied_db:.2} dB of AWGN monotonically and never over-report it: measured={measured_delta_db:.2} dB"
    );
}

#[test]
fn rician_k_estimator_tracks_watterson_f1_in_typical_range() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0..96).map(|v| v as u8).collect();
    let tx = plugin
        .modulate(&payload, &cfg("SCFDMA52-64QAM-P4"))
        .unwrap();

    let mut k_sum_db = 0.0f32;
    let frames = 16usize;

    for i in 0..frames {
        let mut ch =
            WattersonChannel::new(WattersonConfig::good_f1(Some(0x3000 + i as u64))).unwrap();
        let faded = ch.apply(&tx);
        let out = scfdma_demodulate_soft_with_metrics(&faded, "SCFDMA52-64QAM-P4")
            .expect("soft demod through fading");
        k_sum_db += out.metrics.mean_rician_k_db;
    }

    let mean_k_db = k_sum_db / frames as f32;
    // Upper bound 11 dB: Good F1 is mild fading (high K plausible).  The exact
    // mean depends on which segment of the deterministic fade the frame spans —
    // lengthening the frame (e.g. the 6-byte protected length prefix) shifts it
    // by a few tenths of a dB.
    assert!(
        (-2.0..=11.0).contains(&mean_k_db),
        "Watterson F1 estimated K should remain in a typical HF range, got {mean_k_db:.2} dB"
    );
}

/// Combining independent HARQ attempts must beat the best single attempt — that is the diversity gain
/// the whole soft-combining path exists for.
///
/// It also pins down *how* to combine. `symbol_llrs` already divides by σ̂², so each attempt's LLRs are
/// inverse-noise scaled and their plain **sum** is the MAP combine. Re-weighting that sum by
/// `1/mean_noise_var` (what [`combine_llrs_weighted`] does) applies σ⁻² a second time, so it cannot
/// beat the equal-weight sum; the two agree to within the residual mis-calibration of `llr_noise_var`
/// (which models the post-MMSE additive noise but not the channel-estimate error). An earlier
/// assertion here demanded `weighted >= equal` and only passed because the pre-Wiener per-symbol σ²
/// left the LLR scale wrong enough for a second weighting to help.
#[test]
fn soft_combining_beats_best_single_attempt_and_double_weighting_is_a_wash() {
    let plugin = ScFdmaPlugin::new();
    let mode = "SCFDMA52-64QAM-P4";
    let payload: Vec<u8> = (0u8..96).collect();
    let tx = plugin.modulate(&payload, &cfg(mode)).unwrap();
    let payload_bits = bytes_to_bits(&payload);

    let mut eq_correct = 0usize;
    let mut wt_correct = 0usize;
    let mut best_single_correct = 0usize;
    let mut total_bits = 0usize;

    let correct_bits = |llrs: &[f32]| -> usize {
        let Some(bytes) = llrs_to_payload_bytes(llrs, payload.len()) else {
            return 0;
        };
        let bits = bytes_to_bits(&bytes);
        payload_bits
            .iter()
            .enumerate()
            .filter(|(idx, b)| bits.get(*idx) == Some(*b))
            .count()
    };

    for frame in 0..30usize {
        let snrs = [12.0f32, 16.0f32, 20.0f32];
        let mut attempts: Vec<(Vec<f32>, f32)> = Vec::new();

        for (idx, snr) in snrs.iter().enumerate() {
            let rx = add_awgn(&tx, *snr, 0x5000 + frame as u64 * 17 + idx as u64);
            let Ok(out) = scfdma_demodulate_soft_with_metrics(&rx, mode) else {
                continue;
            };
            attempts.push((out.llrs, out.metrics.mean_noise_var));
        }

        let min_len = attempts.iter().map(|(l, _)| l.len()).min().unwrap_or(0);
        if min_len < payload.len() * 8 {
            continue;
        }

        // Equal weight == a plain LLR sum up to a positive constant, so it is sign-identical to it.
        let mut eq = vec![0.0f32; min_len];
        for (llr, _) in &attempts {
            for (dst, src) in eq.iter_mut().zip(llr.iter().take(min_len)) {
                *dst += *src / attempts.len() as f32;
            }
        }

        let refs: Vec<(&[f32], f32)> = attempts
            .iter()
            .map(|(llr, n)| (llr.as_slice(), *n))
            .collect();
        let wt = combine_llrs_weighted(&refs);

        eq_correct += correct_bits(&eq);
        wt_correct += correct_bits(&wt);
        best_single_correct += attempts
            .iter()
            .map(|(llr, _)| correct_bits(&llr[..min_len]))
            .max()
            .unwrap_or(0);
        total_bits += payload.len() * 8;
    }

    assert!(total_bits > 0);
    assert!(
        eq_correct > best_single_correct,
        "the MAP (equal-weight) combine must beat the best single attempt: combined={eq_correct} best_single={best_single_correct}"
    );
    let rel = (wt_correct as f32 - eq_correct as f32) / total_bits as f32;
    assert!(
        rel.abs() < 2e-3,
        "double inverse-noise weighting should be a wash against the MAP combine, not a win or a loss: {rel:+.5} (weighted={wt_correct} equal={eq_correct})"
    );
}
