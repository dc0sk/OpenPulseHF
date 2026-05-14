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

#[test]
fn adaptive_noise_variance_estimator_awgn_relative_delta_within_point75_db() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0..96).map(|v| v as u8).collect();
    let tx = plugin
        .modulate(&payload, &cfg("SCFDMA52-64QAM-P4"))
        .unwrap();

    let mut noise20 = 0.0f32;
    let mut noise28 = 0.0f32;
    let frames = 20usize;

    for i in 0..frames {
        let rx20 = add_awgn(&tx, 20.0, 0x1000 + i as u64);
        let rx28 = add_awgn(&tx, 28.0, 0x2000 + i as u64);

        let m20 = scfdma_demodulate_soft_with_metrics(&rx20, "SCFDMA52-64QAM-P4");
        let m28 = scfdma_demodulate_soft_with_metrics(&rx28, "SCFDMA52-64QAM-P4");

        noise20 += m20.metrics.mean_noise_var;
        noise28 += m28.metrics.mean_noise_var;
    }

    noise20 /= frames as f32;
    noise28 /= frames as f32;

    let measured_delta_db = 10.0 * (noise20 / noise28).log10();
    let expected_delta_db = 8.0;
    assert!(
        (measured_delta_db - expected_delta_db).abs() <= 0.75,
        "noise-variance estimator should track AWGN deltas within ±0.75 dB: measured={measured_delta_db:.2} dB expected={expected_delta_db:.2} dB"
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
        let out = scfdma_demodulate_soft_with_metrics(&faded, "SCFDMA52-64QAM-P4");
        k_sum_db += out.metrics.mean_rician_k_db;
    }

    let mean_k_db = k_sum_db / frames as f32;
    assert!(
        (-2.0..=8.0).contains(&mean_k_db),
        "Watterson F1 estimated K should remain in a typical HF range, got {mean_k_db:.2} dB"
    );
}

#[test]
fn soft_combine_weighted_by_inverse_noise_beats_equal_weight() {
    let plugin = ScFdmaPlugin::new();
    let mode = "SCFDMA52-64QAM-P4";
    let payload: Vec<u8> = (0u8..96).collect();
    let tx = plugin.modulate(&payload, &cfg(mode)).unwrap();
    let payload_bits = bytes_to_bits(&payload);

    let mut eq_correct = 0usize;
    let mut wt_correct = 0usize;
    let mut total_bits = 0usize;

    for frame in 0..30usize {
        let snrs = [12.0f32, 16.0f32, 20.0f32];
        let mut attempts: Vec<(Vec<f32>, f32)> = Vec::new();

        for (idx, snr) in snrs.iter().enumerate() {
            let rx = add_awgn(&tx, *snr, 0x5000 + frame as u64 * 17 + idx as u64);
            let out = scfdma_demodulate_soft_with_metrics(&rx, mode);
            attempts.push((out.llrs, out.metrics.mean_noise_var));
        }

        let min_len = attempts.iter().map(|(l, _)| l.len()).min().unwrap_or(0);
        if min_len < payload.len() * 8 {
            continue;
        }

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

        let eq_bytes = llrs_to_payload_bytes(&eq, payload.len()).unwrap();
        let wt_bytes = llrs_to_payload_bytes(&wt, payload.len()).unwrap();

        let eq_bits = bytes_to_bits(&eq_bytes);
        let wt_bits = bytes_to_bits(&wt_bytes);

        eq_correct += payload_bits
            .iter()
            .enumerate()
            .filter(|(idx, b)| eq_bits.get(*idx) == Some(*b))
            .count();
        wt_correct += payload_bits
            .iter()
            .enumerate()
            .filter(|(idx, b)| wt_bits.get(*idx) == Some(*b))
            .count();
        total_bits += payload.len() * 8;
    }

    assert!(total_bits > 0);
    assert!(
        wt_correct >= eq_correct,
        "weighted LLR combine should not underperform equal-weight combine: weighted={wt_correct} equal={eq_correct}"
    );
}
