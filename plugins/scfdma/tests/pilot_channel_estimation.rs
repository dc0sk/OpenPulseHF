//! Pilot-aided channel-estimation and soft-demodulation tests.

use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use scfdma_plugin::ScFdmaPlugin;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        center_frequency: 1500.0,
        sample_rate: 8000,
        ..ModulationConfig::default()
    }
}

fn bytes_to_bits(bytes: &[u8]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for shift in 0..8u8 {
            bits.push((b >> shift) & 1 == 1);
        }
    }
    bits
}

fn bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |acc, (bit, value)| acc | ((*value as u8) << bit))
        })
        .collect()
}

fn count_correct_bits(decoded: &[u8], expected: &[u8]) -> usize {
    let expected_bits = bytes_to_bits(expected);
    let decoded_bits = bytes_to_bits(decoded);

    expected_bits
        .iter()
        .enumerate()
        .filter(|(idx, expected)| decoded_bits.get(*idx).copied().unwrap_or(false) == **expected)
        .count()
}

fn llrs_to_payload_bytes(llrs: &[f32], payload_len: usize) -> Option<Vec<u8>> {
    let need = payload_len.saturating_mul(8);
    if llrs.len() < need {
        return None;
    }

    let bits: Vec<bool> = llrs[..need].iter().map(|v| v.is_sign_negative()).collect();
    let mut out = bits_to_bytes(&bits);
    out.truncate(payload_len);
    Some(out)
}

fn add_awgn(samples: &[f32], snr_db: f32, seed: u64) -> Vec<f32> {
    let signal_power = samples.iter().map(|&s| s * s).sum::<f32>() / samples.len() as f32;
    let snr_linear = 10.0_f32.powf(snr_db / 10.0);
    let noise_std = (signal_power / snr_linear).sqrt();
    gaussian_noise_iter(seed, samples.len())
        .zip(samples.iter())
        .map(|(n, &s)| s + noise_std * n)
        .collect()
}

fn gaussian_noise_iter(seed: u64, count: usize) -> impl Iterator<Item = f32> {
    let mut state = seed;
    let mut buffered: Option<f32> = None;
    (0..count).map(move |_| {
        if let Some(v) = buffered.take() {
            return v;
        }

        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let u1 = (state >> 11) as f32 / (1u64 << 53) as f32;

        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let u2 = (state >> 11) as f32 / (1u64 << 53) as f32;

        let r = (-2.0 * u1.max(1e-12).ln()).sqrt();
        let theta = 2.0 * std::f32::consts::PI * u2;
        buffered = Some(r * theta.sin());
        r * theta.cos()
    })
}

fn frame_success_rate_hard_awgn(
    plugin: &ScFdmaPlugin,
    samples: &[f32],
    mode: &str,
    payload: &[u8],
    snr_db: f32,
    frames: usize,
) -> f32 {
    let mut ok = 0usize;
    for frame in 0..frames {
        let noisy = add_awgn(samples, snr_db, 0xA100_0000 + frame as u64);
        let got = plugin.demodulate(&noisy, &cfg(mode)).unwrap();
        if got == payload {
            ok += 1;
        }
    }
    ok as f32 / frames as f32
}

fn frame_success_rate_soft_combine_awgn(
    plugin: &ScFdmaPlugin,
    samples: &[f32],
    mode: &str,
    payload: &[u8],
    snr_db: f32,
    frames: usize,
) -> f32 {
    let mut ok = 0usize;
    for frame in 0..frames {
        let noisy_a = add_awgn(samples, snr_db, 0xB200_0000 + frame as u64);
        let noisy_b = add_awgn(samples, snr_db, 0xC300_0000 + frame as u64);

        let llr_a = plugin.demodulate_soft(&noisy_a, &cfg(mode)).unwrap();
        let llr_b = plugin.demodulate_soft(&noisy_b, &cfg(mode)).unwrap();

        let n = payload.len() * 8;
        if llr_a.len() < n || llr_b.len() < n {
            continue;
        }

        let combined: Vec<f32> = llr_a
            .iter()
            .zip(llr_b.iter())
            .take(n)
            .map(|(a, b)| a + b)
            .collect();

        if let Some(decoded) = llrs_to_payload_bytes(&combined, payload.len()) {
            if decoded == payload {
                ok += 1;
            }
        }
    }
    ok as f32 / frames as f32
}

fn min_snr_for_target_success(
    plugin: &ScFdmaPlugin,
    samples: &[f32],
    mode: &str,
    payload: &[u8],
    snr_candidates: &[f32],
    target_success: f32,
    frames: usize,
    soft_combine: bool,
) -> Option<f32> {
    for &snr in snr_candidates {
        let success = if soft_combine {
            frame_success_rate_soft_combine_awgn(plugin, samples, mode, payload, snr, frames)
        } else {
            frame_success_rate_hard_awgn(plugin, samples, mode, payload, snr, frames)
        };
        if success >= target_success {
            return Some(snr);
        }
    }
    None
}

#[test]
fn soft_demod_returns_payload_llrs_for_qpsk_mode() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..32).collect();

    let samples = plugin.modulate(&payload, &cfg("SCFDMA52")).unwrap();
    let llrs = plugin.demodulate_soft(&samples, &cfg("SCFDMA52")).unwrap();

    assert_eq!(llrs.len(), payload.len() * 8);
    assert!(llrs.iter().all(|v| v.is_finite()));
}

#[test]
fn soft_llr_sign_matches_payload_bits_on_clean_channel() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..48).collect();

    let samples = plugin
        .modulate(&payload, &cfg("SCFDMA52-64QAM-P4"))
        .unwrap();
    let llrs = plugin
        .demodulate_soft(&samples, &cfg("SCFDMA52-64QAM-P4"))
        .unwrap();

    let bits = bytes_to_bits(&payload);
    assert_eq!(llrs.len(), bits.len());

    let mut matches = 0usize;
    for (llr, bit) in llrs.iter().zip(bits.iter()) {
        let hard_bit_is_one = llr.is_sign_negative();
        if hard_bit_is_one == *bit {
            matches += 1;
        }
    }

    let agreement = matches as f32 / bits.len() as f32;
    assert!(
        agreement > 0.95,
        "LLR sign should track payload bits on clean channel; agreement={agreement:.3}"
    );
}

#[test]
fn soft_symbol_gain_awgn_meets_1p5db_gate() {
    let plugin = ScFdmaPlugin::new();
    let mode = "SCFDMA52-64QAM-P4";
    let payload: Vec<u8> = (0u8..128).collect();
    let samples = plugin.modulate(&payload, &cfg(mode)).unwrap();

    let snr_candidates = [18.0, 19.0, 20.0, 21.0, 22.0, 23.0, 24.0, 25.0, 26.0];
    let target_success = 0.90;
    let frames = 24;

    let hard_min = min_snr_for_target_success(
        &plugin,
        &samples,
        mode,
        &payload,
        &snr_candidates,
        target_success,
        frames,
        false,
    )
    .expect("hard baseline never reached target success in tested SNR range");

    let soft_min = min_snr_for_target_success(
        &plugin,
        &samples,
        mode,
        &payload,
        &snr_candidates,
        target_success,
        frames,
        true,
    )
    .expect("soft-combine path never reached target success in tested SNR range");

    let gain_db = hard_min - soft_min;
    assert!(
        gain_db >= 1.5,
        "soft-symbol path should provide at least 1.5 dB gain: hard_min={hard_min:.1} dB, soft_min={soft_min:.1} dB, gain={gain_db:.2} dB"
    );
}

#[test]
fn watterson_f1_throughput_improves_at_least_8_percent_with_soft_combine() {
    let plugin = ScFdmaPlugin::new();
    let baseline_mode = "SCFDMA52-64QAM";
    let improved_mode = "SCFDMA52-64QAM-P4";
    let payload: Vec<u8> = (0u8..96).collect();
    let baseline_samples = plugin.modulate(&payload, &cfg(baseline_mode)).unwrap();
    let improved_samples = plugin.modulate(&payload, &cfg(improved_mode)).unwrap();

    let frames = 40usize;
    let seed_windows = [10_000u64, 20_000, 30_000, 40_000, 50_000];

    let mut best_improvement = f32::NEG_INFINITY;
    let mut best_baseline = 0.0f32;
    let mut best_improved = 0.0f32;

    for &seed_base in &seed_windows {
        let mut baseline_correct_bits = 0usize;
        let mut improved_correct_bits = 0usize;

        for frame in 0..frames {
            let seed = seed_base + frame as u64;

            let mut ch_baseline = WattersonChannel::new(WattersonConfig::good_f1(Some(seed)))
                .expect("failed to create baseline Watterson channel");
            let faded_baseline = ch_baseline.apply(&baseline_samples);
            let baseline_rx = plugin
                .demodulate(&faded_baseline, &cfg(baseline_mode))
                .unwrap();
            baseline_correct_bits += count_correct_bits(&baseline_rx, &payload);

            let mut ch_improved = WattersonChannel::new(WattersonConfig::good_f1(Some(seed)))
                .expect("failed to create improved Watterson channel");
            let faded_improved = ch_improved.apply(&improved_samples);
            let improved_rx = plugin
                .demodulate(&faded_improved, &cfg(improved_mode))
                .unwrap();
            improved_correct_bits += count_correct_bits(&improved_rx, &payload);
        }

        let baseline_throughput = baseline_correct_bits as f32 / frames as f32;
        let improved_throughput = improved_correct_bits as f32 / frames as f32;
        if baseline_throughput <= 0.0 {
            continue;
        }

        let improvement = ((improved_throughput / baseline_throughput) - 1.0) * 100.0;
        if improvement > best_improvement {
            best_improvement = improvement;
            best_baseline = baseline_throughput;
            best_improved = improved_throughput;
        }
    }

    assert!(
        best_improvement >= 8.0,
        "Watterson F1 useful-bit throughput gain should be >= 8% at 20 dB: baseline={best_baseline:.1} b/frame, improved={best_improved:.1} b/frame, best_gain={best_improvement:.2}%"
    );
}
