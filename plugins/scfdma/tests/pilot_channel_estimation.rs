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
        .filter(|(idx, expected_bit)| decoded_bits.get(*idx) == Some(expected_bit))
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

fn frame_success_rate_hard_diversity_awgn(
    plugin: &ScFdmaPlugin,
    samples: &[f32],
    mode: &str,
    payload: &[u8],
    snr_db: f32,
    frames: usize,
) -> f32 {
    let mut ok = 0usize;
    for frame in 0..frames {
        let noisy_a = add_awgn(samples, snr_db, 0xD400_0000 + frame as u64);
        let noisy_b = add_awgn(samples, snr_db, 0xE500_0000 + frame as u64);

        let got_a = plugin.demodulate(&noisy_a, &cfg(mode)).unwrap();
        let got_b = plugin.demodulate(&noisy_b, &cfg(mode)).unwrap();

        if got_a == payload || got_b == payload {
            ok += 1;
        }
    }
    ok as f32 / frames as f32
}

fn frame_success_rate_watterson_hard(
    plugin: &ScFdmaPlugin,
    samples: &[f32],
    mode: &str,
    payload: &[u8],
    frames: usize,
    mut config_for_seed: impl FnMut(u64) -> WattersonConfig,
) -> f32 {
    let mut ok = 0usize;
    for frame in 0..frames {
        let seed = 0x7100_0000 + frame as u64;
        let channel_cfg = config_for_seed(seed);
        let mut ch = WattersonChannel::new(channel_cfg)
            .expect("failed to construct Watterson channel profile");
        let faded = ch.apply(samples);
        let got = plugin.demodulate(&faded, &cfg(mode)).unwrap();
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
    path: DecodePath,
) -> Option<f32> {
    for &snr in snr_candidates {
        let success = match path {
            DecodePath::HardDiversityTwo => {
                frame_success_rate_hard_diversity_awgn(plugin, samples, mode, payload, snr, frames)
            }
            DecodePath::SoftCombineTwo => {
                frame_success_rate_soft_combine_awgn(plugin, samples, mode, payload, snr, frames)
            }
        };
        if success >= target_success {
            return Some(snr);
        }
    }
    None
}

#[derive(Clone, Copy)]
enum DecodePath {
    HardDiversityTwo,
    SoftCombineTwo,
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
        DecodePath::HardDiversityTwo,
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
        DecodePath::SoftCombineTwo,
    )
    .expect("soft-combine path never reached target success in tested SNR range");

    let gain_db = hard_min - soft_min;
    assert!(
        gain_db >= 1.5,
        "soft-symbol path should provide at least 1.5 dB gain over equivalent two-shot hard baseline: hard_min={hard_min:.1} dB, soft_min={soft_min:.1} dB, gain={gain_db:.2} dB"
    );
}

#[test]
fn watterson_f1_pilot_density_throughput_improves_at_least_2_percent() {
    let plugin = ScFdmaPlugin::new();
    let baseline_mode = "SCFDMA52-64QAM";
    let improved_mode = "SCFDMA52-64QAM-P4";
    let payload: Vec<u8> = (0u8..96).collect();
    let baseline_samples = plugin.modulate(&payload, &cfg(baseline_mode)).unwrap();
    let improved_samples = plugin.modulate(&payload, &cfg(improved_mode)).unwrap();

    let frames = 40usize;
    let seed_windows = [10_000u64, 20_000, 30_000, 40_000, 50_000];

    let mut improvements = Vec::with_capacity(seed_windows.len());
    let mut avg_hard = 0.0f32;
    let mut avg_soft = 0.0f32;

    for &seed_base in &seed_windows {
        let mut baseline_correct_bits = 0usize;
        let mut improved_correct_bits = 0usize;

        for frame in 0..frames {
            let hard_seed = seed_base + frame as u64;
            let mut ch_hard = WattersonChannel::new(WattersonConfig::good_f1(Some(hard_seed)))
                .expect("failed to create hard-path Watterson channel");
            let faded_baseline = ch_hard.apply(&baseline_samples);
            let baseline_rx = plugin
                .demodulate(&faded_baseline, &cfg(baseline_mode))
                .unwrap();
            baseline_correct_bits += count_correct_bits(&baseline_rx, &payload);

            let mut ch_improved = WattersonChannel::new(WattersonConfig::good_f1(Some(hard_seed)))
                .expect("failed to create improved-path Watterson channel");
            let faded_improved = ch_improved.apply(&improved_samples);
            let improved_rx = plugin
                .demodulate(&faded_improved, &cfg(improved_mode))
                .unwrap();
            improved_correct_bits += count_correct_bits(&improved_rx, &payload);
        }

        let hard_throughput = baseline_correct_bits as f32 / frames as f32;
        let soft_throughput = improved_correct_bits as f32 / frames as f32;
        if hard_throughput <= 0.0 {
            continue;
        }

        let improvement = ((soft_throughput / hard_throughput) - 1.0) * 100.0;
        improvements.push(improvement);
        avg_hard += hard_throughput;
        avg_soft += soft_throughput;
    }

    assert!(
        !improvements.is_empty(),
        "no valid seed windows produced hard-path throughput"
    );

    improvements.sort_by(|a, b| a.total_cmp(b));
    let p80_idx = ((improvements.len() * 8).div_ceil(10)).saturating_sub(1);
    let p80_improvement = improvements[p80_idx.min(improvements.len() - 1)];

    avg_hard /= improvements.len() as f32;
    avg_soft /= improvements.len() as f32;

    assert!(
        p80_improvement >= 2.0,
        "Watterson F1 useful-bit throughput gain should be >= 2% at 20 dB (p80 across seed windows): baseline={avg_hard:.1} b/frame, improved={avg_soft:.1} b/frame, p80_gain={p80_improvement:.2}%"
    );
}

#[test]
fn scfdma_qam_modes_remain_deferred_on_watterson_profile_entry_matrix() {
    let plugin = ScFdmaPlugin::new();
    let payload: Vec<u8> = (0u8..96).collect();
    let frames = 30usize;

    let samples_16 = plugin.modulate(&payload, &cfg("SCFDMA52-16QAM")).unwrap();
    let samples_64 = plugin.modulate(&payload, &cfg("SCFDMA52-64QAM")).unwrap();

    let scenarios: [(&str, fn(u64) -> WattersonConfig); 3] = [
        ("good_f1", |seed| WattersonConfig::good_f1(Some(seed))),
        ("good_f2", |seed| WattersonConfig::good_f2(Some(seed))),
        ("moderate_f1", |seed| {
            WattersonConfig::moderate_f1(Some(seed))
        }),
    ];

    let mut worst_16 = f32::INFINITY;
    let mut worst_64 = f32::INFINITY;

    for (label, mk) in scenarios {
        let succ_16 = frame_success_rate_watterson_hard(
            &plugin,
            &samples_16,
            "SCFDMA52-16QAM",
            &payload,
            frames,
            mk,
        );
        let succ_64 = frame_success_rate_watterson_hard(
            &plugin,
            &samples_64,
            "SCFDMA52-64QAM",
            &payload,
            frames,
            mk,
        );

        worst_16 = worst_16.min(succ_16);
        worst_64 = worst_64.min(succ_64);

        println!(
            "profile-entry-matrix {label}: 16QAM_success={succ_16:.3} 64QAM_success={succ_64:.3}"
        );
    }

    // Profile-entry gate (wide-matrix): min scenario success >= 90%.
    // Current expectation: both modes remain below this gate and therefore deferred.
    const PROFILE_ENTRY_MIN_SUCCESS: f32 = 0.90;
    assert!(
        worst_16 < PROFILE_ENTRY_MIN_SUCCESS,
        "SCFDMA52-16QAM unexpectedly met profile-entry gate across matrix: worst={worst_16:.3}"
    );
    assert!(
        worst_64 < PROFILE_ENTRY_MIN_SUCCESS,
        "SCFDMA52-64QAM unexpectedly met profile-entry gate across matrix: worst={worst_64:.3}"
    );
}
