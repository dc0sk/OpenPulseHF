//! RESEARCH HARNESS — the two measurements that falsified the first QPSK threshold table. No asserts.
//!
//! Kept because both results are reusable instruments, not one-off checks:
//!
//! * `f1_fade_decode_cliff` — the decode column on the channel a rung actually exists for. QPSK250-D
//!   on `moderate_f1` at its own 7 dB floor decodes frames down to ρ = 0.276 (seed 127 of 150),
//!   which is *below* that mode's recorded idle-noise ceiling of 0.291. The decodable-frame and
//!   noise distributions overlap, so no threshold separates them — a fact an AWGN-only decode column
//!   cannot show.
//! * `f2_noise_colour` — the noise ceiling is set by the overlap of the noise spectrum with the
//!   TEMPLATE's spectrum, not by template length alone. A 500 Hz receive filter (an ordinary rig
//!   setting for these modes) lifts idle ρ above every threshold measured so far, including BPSK250's
//!   shipped 0.40. Note the band-limiting here is a brick-wall FFT mask, sharper than any real
//!   filter, so its numbers are a worst case.
//!
//! Cross-check on the harness itself: synthetic SSB-shaped noise (300–2700 Hz) gives BPSK250
//! ρ = 0.196 against 0.205 measured on the real recorded captures — so this reproduces the corpus,
//! and the corpus is SSB-bandwidth reception.

use openpulse_channel::watterson::WattersonChannel;
use openpulse_channel::{ChannelModel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_dsp::acquisition::IqMatchedFilter;
use openpulse_modem::channel_sim::ChannelSimHarness;
use std::time::Duration;

const FS: f32 = 8_000.0;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        sample_rate: 8_000,
        center_frequency: 1_500.0,
        ..Default::default()
    }
}

/// Peak rho with the engine's exact template, window and grid.
fn rho_engine(mode: &str, window: &[f32]) -> Option<f32> {
    let t = plugin_template(mode)?;
    let step = (0.25 * FS / t.0.len() as f32).max(0.5);
    let n = (t.2 / step).round() as i32;
    let grid: Vec<f32> = (-n..=n).map(|k| k as f32 * step).collect();
    let mf = IqMatchedFilter::new(t.0);
    if window.len() <= mf.len() {
        return None;
    }
    mf.search_normalized_over_frequency(window, window.len() - mf.len(), 0.05, FS, &grid)
        .map(|(r, _)| r.rho)
}

/// (samples, threshold, grid_hz) for either plugin.
fn plugin_template(mode: &str) -> Option<(Vec<f32>, f32, f32)> {
    if mode.starts_with("BPSK") {
        let t = bpsk_plugin::BpskPlugin::new().preamble_template(&cfg(mode))?;
        Some((t.samples, t.rho_threshold, t.rho_grid_hz))
    } else {
        let t = qpsk_plugin::QpskPlugin::new().preamble_template(&cfg(mode))?;
        Some((t.samples, t.rho_threshold, t.rho_grid_hz))
    }
}

fn win_len(mode: &str) -> usize {
    let t = plugin_template(mode).unwrap();
    let syms = if mode.starts_with("BPSK") { 31 } else { 15 };
    t.0.len() / syms * (syms + 2)
}

// ── F2: does noise COLOUR move the rho ceiling? ───────────────────────────────

/// Band-limited Gaussian noise via FFT bin masking.
fn band_noise(n: usize, lo_hz: f32, hi_hz: f32, seed: u64) -> Vec<f32> {
    use rustfft::{num_complex::Complex, FftPlanner};
    let mut state = seed | 1;
    let mut rnd = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        ((state.wrapping_mul(0x2545F4914F6CDD1D) >> 33) as f32 / (1u64 << 31) as f32) - 1.0
    };
    let mut buf: Vec<Complex<f32>> = (0..n).map(|_| Complex::new(rnd(), 0.0)).collect();
    let mut planner = FftPlanner::new();
    planner.plan_fft_forward(n).process(&mut buf);
    let bin_hz = FS / n as f32;
    for (k, v) in buf.iter_mut().enumerate() {
        let f = if k <= n / 2 {
            k as f32 * bin_hz
        } else {
            (n - k) as f32 * bin_hz
        };
        if f < lo_hz || f > hi_hz {
            *v = Complex::new(0.0, 0.0);
        }
    }
    planner.plan_fft_inverse(n).process(&mut buf);
    let out: Vec<f32> = buf.iter().map(|c| c.re).collect();
    let rms = (out.iter().map(|s| s * s).sum::<f32>() / n as f32)
        .sqrt()
        .max(1e-12);
    out.iter().map(|s| s / rms * 0.05).collect()
}

#[test]
#[ignore = "verification"]
fn f2_noise_colour() {
    let bands = [
        ("white 0-4k", 0.0f32, 4_000.0),
        ("ssb 300-2700", 300.0, 2_700.0),
        ("filter 1250-1750", 1_250.0, 1_750.0),
        ("filter 1400-1600", 1_400.0, 1_600.0),
    ];
    println!("\nF2: peak rho over 45 s of synthetic noise, engine window/grid");
    println!(
        "{:<18} {:>10} {:>10} {:>10} {:>10}",
        "band", "QPSK125", "QPSK250", "QPSK500", "BPSK250"
    );
    let n = 360_000; // 45 s
    for (name, lo, hi) in bands {
        let noise = band_noise(n, lo, hi, 12345);
        let mut row = vec![];
        for mode in ["QPSK125", "QPSK250", "QPSK500", "BPSK250"] {
            let w = win_len(mode);
            let thr = plugin_template(mode).unwrap().1;
            let mut peak = 0.0f32;
            let mut s = 0usize;
            while s + w <= noise.len() {
                if let Some(r) = rho_engine(mode, &noise[s..s + w]) {
                    peak = peak.max(r);
                }
                s += w / 4;
            }
            row.push(format!("{peak:.3}{}", if peak >= thr { "*" } else { " " }));
        }
        println!(
            "{name:<18} {:>10} {:>10} {:>10} {:>10}",
            row[0], row[1], row[2], row[3]
        );
    }
    println!("(* = at or above that mode's published threshold)");
    for mode in ["QPSK125", "QPSK250", "QPSK500", "BPSK250"] {
        println!("  {mode} threshold {:.2}", plugin_template(mode).unwrap().1);
    }
}

// ── F1: does a DECODABLE fade frame ever score below the threshold? ───────────

#[test]
#[ignore = "verification"]
fn f1_fade_decode_cliff() {
    let mode = std::env::var("F1_MODE").unwrap_or_else(|_| "QPSK250-D".into());
    let snr: f32 = std::env::var("F1_SNR")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7.0);
    let seeds: u64 = std::env::var("F1_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);
    let thr = plugin_template(&mode).unwrap().1;
    let w = win_len(&mode);

    println!("\nF1: {mode} on moderate_f1 @ {snr} dB, threshold {thr:.2}, {seeds} seeds");
    let mut decodable_rhos: Vec<f32> = vec![];
    let mut below = 0usize;
    for seed in 0..seeds {
        // rho at the true onset, on the same realization the receiver will see
        let clean = qpsk_plugin::QpskPlugin::new()
            .modulate(b"fade cliff probe", &cfg(&mode))
            .unwrap();
        let mut c = WattersonConfig::moderate_f1(Some(seed));
        c.snr_db = snr;
        let faded = WattersonChannel::new(c).unwrap().apply(&clean);
        let r = match rho_engine(&mode, &faded[..w.min(faded.len())]) {
            Some(r) => r,
            None => continue,
        };

        let mut h = ChannelSimHarness::new();
        for eng in [&mut h.tx_engine, &mut h.rx_engine] {
            eng.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
                .unwrap();
        }
        h.tx_engine
            .transmit_with_fec_mode(b"fade cliff probe", &mode, FecMode::Rs, None)
            .expect("tx");
        let mut c2 = WattersonConfig::moderate_f1(Some(seed));
        c2.snr_db = snr;
        h.route(&mut WattersonChannel::new(c2).unwrap());
        let ok = h
            .rx_engine
            .receive_with_fec_mode_timeout(&mode, FecMode::Rs, None, Duration::from_millis(8_000))
            .is_ok();
        if ok {
            decodable_rhos.push(r);
            if r < thr {
                below += 1;
                println!("  DECODES at rho {r:.3} < threshold {thr:.2}  (seed {seed})");
            }
        }
    }
    decodable_rhos.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "  decodable frames: {} / {seeds}; min rho {:.3}; below threshold: {below}",
        decodable_rhos.len(),
        decodable_rhos.first().copied().unwrap_or(f32::NAN)
    );
    if decodable_rhos.len() > 4 {
        println!("  five lowest decodable rho: {:?}", &decodable_rhos[..5]);
    }
}
