use std::fs;

use openpulse_channel::{
    awgn::AwgnChannel, watterson::WattersonChannel, AwgnConfig, ChannelModel, WattersonConfig,
};
use openpulse_modem::ofdm_sim::{
    clip_and_filter, clip_iterative, demodulate_ofdm_frame, generate_ofdm_frame, measure_papr,
    tone_reservation, OfdmConfig, OfdmStats,
};
use serde_json::json;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn bit_error_rate(payload: &[u8], rx: &[u8]) -> f64 {
    let bits_total = payload.len() * 8;
    if bits_total == 0 {
        return 0.0;
    }
    let mut errors = 0usize;
    for (a, b) in payload.iter().zip(rx.iter()) {
        errors += (a ^ b).count_ones() as usize;
    }
    // Any missing bytes count as all bits wrong.
    if rx.len() < payload.len() {
        errors += (payload.len() - rx.len()) * 8;
    }
    errors as f64 / bits_total as f64
}

enum PaprReduction {
    None,
    Clip2dB,
    Clip3dB,
    Clip4dB,
    IterativeClip6dB,
    ToneReservation4,
}

impl PaprReduction {
    fn apply(&self, cfg: &OfdmConfig, samples: &[f32]) -> Vec<f32> {
        match self {
            PaprReduction::None => samples.to_vec(),
            PaprReduction::Clip2dB => clip_and_filter(samples, 2.0),
            PaprReduction::Clip3dB => clip_and_filter(samples, 3.0),
            PaprReduction::Clip4dB => clip_and_filter(samples, 4.0),
            PaprReduction::IterativeClip6dB => clip_iterative(samples, 5.9, 50),
            PaprReduction::ToneReservation4 => tone_reservation(cfg, samples, 4),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            PaprReduction::None => "none",
            PaprReduction::Clip2dB => "clip_2dB",
            PaprReduction::Clip3dB => "clip_3dB",
            PaprReduction::Clip4dB => "clip_4dB",
            PaprReduction::IterativeClip6dB => "iterative_clip_6dB",
            PaprReduction::ToneReservation4 => "tone_reservation_4",
        }
    }
}

fn run_case(
    cfg: &OfdmConfig,
    papr_reduction: &PaprReduction,
    channel: &mut dyn ChannelModel,
    payload: &[u8],
) -> OfdmStats {
    let samples = generate_ofdm_frame(cfg, payload);
    let shaped = papr_reduction.apply(cfg, &samples);
    let papr_db = measure_papr(&shaped);
    let noisy = channel.apply(&shaped);
    let rx = demodulate_ofdm_frame(&noisy, cfg);
    let ber = bit_error_rate(payload, &rx);
    OfdmStats {
        papr_db,
        ber,
        gross_bps: cfg.gross_bps(),
        bw_hz: cfg.bw_hz(),
    }
}

// ── Test configs ──────────────────────────────────────────────────────────────

fn vara_like() -> OfdmConfig {
    OfdmConfig {
        n_subcarriers: 52,
        cp_samples: 16,
        fs: 8000.0,
        pilot_count: 4,
        mod_order: 2,
    }
}

fn reduced() -> OfdmConfig {
    OfdmConfig {
        n_subcarriers: 16,
        cp_samples: 16,
        fs: 8000.0,
        pilot_count: 2,
        mod_order: 2,
    }
}

fn minimal() -> OfdmConfig {
    OfdmConfig {
        n_subcarriers: 8,
        cp_samples: 16,
        fs: 8000.0,
        pilot_count: 1,
        mod_order: 2,
    }
}

// ── Main sweep test ───────────────────────────────────────────────────────────

/// Full simulation sweep: configs × PAPR reduction × channels × modulation.
///
/// Emits results to `docs/ofdm-research/raw_results.json`.
/// Asserts that at least one combination achieves PAPR ≤ 6 dB on clean channel.
#[test]
fn ofdm_sweep_and_papr_gate() {
    let payload: Vec<u8> = (0..64u8).collect();

    let configs: Vec<(&str, OfdmConfig)> = vec![
        ("vara_like_52sc", vara_like()),
        ("reduced_16sc", reduced()),
        ("minimal_8sc", minimal()),
    ];

    let reductions: Vec<PaprReduction> = vec![
        PaprReduction::None,
        PaprReduction::Clip2dB,
        PaprReduction::Clip3dB,
        PaprReduction::Clip4dB,
        PaprReduction::IterativeClip6dB,
        PaprReduction::ToneReservation4,
    ];

    let mut results = Vec::new();
    let mut min_clean_papr = f32::INFINITY;

    for (cfg_name, cfg) in &configs {
        for reduction in &reductions {
            // --- Clean channel ---
            {
                struct PassThrough;
                impl ChannelModel for PassThrough {
                    fn apply(&mut self, s: &[f32]) -> Vec<f32> {
                        s.to_vec()
                    }
                    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
                        vec![0.0; length]
                    }
                }
                let stats = run_case(cfg, reduction, &mut PassThrough, &payload);
                if stats.papr_db < min_clean_papr {
                    min_clean_papr = stats.papr_db;
                }
                results.push(json!({
                    "config": cfg_name,
                    "papr_reduction": reduction.name(),
                    "channel": "clean",
                    "papr_db": stats.papr_db,
                    "ber": stats.ber,
                    "gross_bps": stats.gross_bps,
                    "bw_hz": stats.bw_hz,
                }));
            }

            // --- AWGN 20 dB ---
            {
                let mut ch = AwgnChannel::new(AwgnConfig::new(20.0, Some(42))).unwrap();
                let stats = run_case(cfg, reduction, &mut ch, &payload);
                results.push(json!({
                    "config": cfg_name,
                    "papr_reduction": reduction.name(),
                    "channel": "awgn_20db",
                    "papr_db": stats.papr_db,
                    "ber": stats.ber,
                    "gross_bps": stats.gross_bps,
                    "bw_hz": stats.bw_hz,
                }));
            }

            // --- AWGN 10 dB ---
            {
                let mut ch = AwgnChannel::new(AwgnConfig::new(10.0, Some(42))).unwrap();
                let stats = run_case(cfg, reduction, &mut ch, &payload);
                results.push(json!({
                    "config": cfg_name,
                    "papr_reduction": reduction.name(),
                    "channel": "awgn_10db",
                    "papr_db": stats.papr_db,
                    "ber": stats.ber,
                    "gross_bps": stats.gross_bps,
                    "bw_hz": stats.bw_hz,
                }));
            }

            // --- Watterson Good F1 ---
            {
                let mut ch = WattersonChannel::new(WattersonConfig::good_f1(Some(1))).unwrap();
                let stats = run_case(cfg, reduction, &mut ch, &payload);
                results.push(json!({
                    "config": cfg_name,
                    "papr_reduction": reduction.name(),
                    "channel": "watterson_good_f1",
                    "papr_db": stats.papr_db,
                    "ber": stats.ber,
                    "gross_bps": stats.gross_bps,
                    "bw_hz": stats.bw_hz,
                }));
            }
        }
    }

    // Write results.
    fs::create_dir_all("../../docs/ofdm-research").ok();
    let json_out = serde_json::to_string_pretty(&json!({ "results": results })).unwrap();
    // Write relative to crate root (tests run from workspace root).
    fs::create_dir_all("docs/ofdm-research").ok();
    fs::write("docs/ofdm-research/raw_results.json", &json_out).ok();

    // Gate: at least one config + PAPR-reduction combination achieves ≤ 6 dB on clean channel.
    assert!(
        min_clean_papr <= 6.0,
        "No OFDM configuration achieved PAPR ≤ 6 dB on clean channel (best: {min_clean_papr:.1} dB). \
         Consider stronger clip threshold or more reserved tones."
    );
}
