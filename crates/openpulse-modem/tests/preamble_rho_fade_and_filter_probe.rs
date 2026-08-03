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
    let t = plugin_template(mode).expect("mode publishes a preamble template");
    win_len_for(&t.0, mode)
}

/// Window length for a template: its own span plus two symbols of slack.
fn win_len_for(samples: &[f32], mode: &str) -> usize {
    let syms = if mode.starts_with("BPSK") { 31 } else { 15 };
    samples.len() / syms * (syms + 2)
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
            // QPSK published templates only on the withdrawn #1053 branch; at HEAD it returns the
            // trait default `None`. Skipping keeps the BPSK column runnable instead of panicking
            // the whole probe on the first QPSK cell.
            let Some((_, thr, _)) = plugin_template(mode) else {
                row.push("no tmpl".to_string());
                continue;
            };
            let w = win_len(mode);
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
        match plugin_template(mode) {
            Some((_, thr, _)) => println!("  {mode} threshold {thr:.2}"),
            None => println!("  {mode} publishes no template at HEAD"),
        }
    }
}

// ── F3: is the SEQUENCE the variable, or the bandwidth? (#1062) ───────────────

/// A maximal-length LFSR sequence of length `2^bits - 1`, as ±1.
fn m_sequence(bits: u32, taps: &[u32]) -> Vec<f32> {
    let mut reg = vec![true; bits as usize];
    let mut out = Vec::with_capacity((1 << bits) - 1);
    for _ in 0..((1u32 << bits) - 1) {
        out.push(if reg[reg.len() - 1] { 1.0f32 } else { -1.0 });
        let fb = taps.iter().fold(false, |a, &t| a ^ reg[t as usize - 1]);
        reg.rotate_right(1);
        reg[0] = fb;
    }
    out
}

/// Build a PN-chip template through the REAL BPSK modulator.
///
/// The modulator's preamble bits are hardcoded, so the PN chips are placed in the *payload* span
/// instead and that span is returned. Everything downstream — NRZI, half-Hann crossfade, carrier —
/// is the shipped code path, so this differs from `bpsk_preamble_template` in the symbol sequence
/// and nothing else. The NRZI state entering the payload is `+1` (the 32 preamble bits contain 16
/// ones, an even number of flips), so `bit[k] = chip[k] != chip[k-1]` with `chip[-1] = +1` inverts
/// the encoder exactly. The final chip is dropped for the same crossfade reason the shipped
/// template drops its last symbol.
fn pn_template(mode: &str, chips: &[f32]) -> Option<Vec<f32>> {
    let mut bits = Vec::with_capacity(chips.len());
    let mut prev = 1.0f32;
    for &c in chips {
        bits.push(c != prev);
        prev = c;
    }
    let mut bytes = vec![0u8; bits.len().div_ceil(8)];
    for (k, &b) in bits.iter().enumerate() {
        if b {
            bytes[k / 8] |= 1 << (k % 8);
        }
    }
    let c = cfg(mode);
    let full = bpsk_plugin::BpskPlugin::new().modulate(&bytes, &c).ok()?;
    let baud: f32 = mode.trim_start_matches("BPSK").parse().ok()?;
    let n = (FS / baud).round() as usize;
    let start = n * 32; // PREAMBLE_SYMS
    let span = n * (chips.len() - 1);
    (full.len() >= start + span).then(|| full[start..start + span].to_vec())
}

/// Peak normalised correlation of `template` against `window`, engine-style.
fn rho_of(template: &[f32], window: &[f32], grid_hz: f32) -> Option<f32> {
    let step = (0.25 * FS / template.len() as f32).max(0.5);
    let n = (grid_hz / step).round() as i32;
    let grid: Vec<f32> = (-n..=n).map(|k| k as f32 * step).collect();
    let mf = IqMatchedFilter::new(template.to_vec());
    if window.len() <= mf.len() {
        return None;
    }
    mf.search_normalized_over_frequency(window, window.len() - mf.len(), 0.05, FS, &grid)
        .map(|(r, _)| r.rho)
}

/// Fraction of the DFT bins that actually carry the template's energy (participation ratio /
/// bin count). A two-line spectrum scores near zero however long it runs; flat noise scores 1.
fn band_occupancy(x: &[f32]) -> f32 {
    use rustfft::{num_complex::Complex, FftPlanner};
    let n = 4096;
    let mut buf: Vec<Complex<f32>> = x
        .iter()
        .map(|&v| Complex::new(v, 0.0))
        .chain(std::iter::repeat_n(
            Complex::new(0.0, 0.0),
            n - x.len().min(n),
        ))
        .take(n)
        .collect();
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);
    let p: Vec<f32> = buf.iter().map(|c| c.norm_sqr()).collect();
    let tot: f32 = p.iter().sum();
    if tot <= 0.0 {
        return 0.0;
    }
    let s2: f32 = p.iter().map(|v| (v / tot) * (v / tot)).sum();
    1.0 / s2 / n as f32
}

/// Worst off-peak normalised correlation of a template against a REAL transmission carrying it —
/// how badly a misaligned window still matches. This is the #1052 onset-placement failure as a
/// number.
///
/// Deliberately not measured against a zero-padded copy of the template: `search_normalized`
/// divides by the *window's* energy, so a window half over padding scores ρ ≈ 0.7 by construction
/// and every sequence pins to that artifact regardless of its autocorrelation. Here the template
/// is followed by real modulated data, so every candidate lag has full overlap with real signal.
fn peak_sidelobe(template: &[f32], mode: &str, guard: usize) -> f32 {
    let payload: Vec<u8> = (0..200u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    let data = bpsk_plugin::BpskPlugin::new()
        .modulate(&payload, &cfg(mode))
        .expect("modulate payload");
    let sig: Vec<f32> = template.iter().chain(data.iter()).copied().collect();

    // Lags are restricted to a shift WITHIN the template's own span. Beyond that the window would
    // be scoring against the filler frame's own alternating preamble, which is a match against
    // different signal — a real question, but not this one, and it is not symmetric across cases.
    let mf = IqMatchedFilter::new(template.to_vec());
    let mut worst = 0.0f32;
    for lag in (guard + 1)..template.len() {
        if let Some(r) = mf.search_normalized(&sig[lag..], 0, 0.05) {
            worst = worst.max(r.rho);
        }
    }
    worst
}

#[test]
#[ignore = "verification"]
fn f3_pn_vs_alternating() {
    let alt = plugin_template("BPSK250").expect("BPSK250 template").0;

    // 31 chips at 250/s == the shipped template's duration AND bandwidth: sequence is the only
    // variable. 110 chips at 1000/s == same ~110 ms duration, 4x the chip bandwidth.
    let pn31 = pn_template("BPSK250", &m_sequence(5, &[5, 3])).expect("pn31");
    let mut long = m_sequence(7, &[7, 6]);
    long.truncate(110);
    let pn110 = pn_template("BPSK1000", &long).expect("pn110");

    // The sidelobe guard must be half of the TEMPLATE'S OWN symbol period, not a shared constant:
    // BPSK1000 runs 8 samples/chip against BPSK250's 32, so one guard in samples would excise
    // real lag-1 sidelobes from the wideband case — the one whose number looks best.
    let cases: [(&str, &[f32], &str, usize); 3] = [
        ("BPSK250 alternating (shipped)", &alt, "BPSK250", 32),
        ("BPSK250 PN-31 (same BW)", &pn31, "BPSK250", 32),
        ("BPSK1000 PN-110 (4x BW)", &pn110, "BPSK1000", 8),
    ];

    // Peak sidelobe is reported at three guards because the number is guard-sensitive and the
    // sensitivity is the point: at half a symbol every sequence is still inside the pulse's own
    // mainlobe, so that column measures pulse shape, not sequence. Only past ~1 symbol does the
    // sequence's autocorrelation dominate.
    println!("\nF3: template structure, real modulator path");
    println!(
        "{:<32} {:>8} {:>8} {:>7} {:>10} {:>26}",
        "template", "samples", "ms", "sps", "occupancy", "peak sidelobe @ guard"
    );
    println!(
        "{:<32} {:>8} {:>8} {:>7} {:>10} {:>26}",
        "", "", "", "", "", "0.5 sym   1 sym   2 sym"
    );
    for (name, t, m, sps) in cases {
        println!(
            "{name:<32} {:>8} {:>8.0} {sps:>7} {:>10.3} {:>9.3} {:>7.3} {:>7.3}",
            t.len(),
            t.len() as f32 / FS * 1000.0,
            band_occupancy(t),
            peak_sidelobe(t, m, sps / 2),
            peak_sidelobe(t, m, sps),
            peak_sidelobe(t, m, sps * 2),
        );
    }

    let bands = [
        ("white 0-4k", 0.0f32, 4_000.0),
        ("ssb 300-2700", 300.0, 2_700.0),
        ("filter 1250-1750", 1_250.0, 1_750.0),
        ("filter 1400-1600", 1_400.0, 1_600.0),
    ];
    println!("\nF3: peak rho over 45 s of synthetic noise (grid +/-20 Hz, as shipped)");
    println!(
        "{:<18} {:>16} {:>16} {:>16}",
        "band", "alternating", "PN-31", "PN-110"
    );
    let noise_len = 360_000;
    for (name, lo, hi) in bands {
        let noise = band_noise(noise_len, lo, hi, 12345);
        let mut row = vec![];
        for (_, t, _, _) in cases {
            let w = win_len_for(t, "BPSK250");
            let mut peak = 0.0f32;
            let mut s = 0usize;
            while s + w <= noise.len() {
                if let Some(r) = rho_of(t, &noise[s..s + w], 20.0) {
                    peak = peak.max(r);
                }
                s += w / 4;
            }
            row.push(format!("{peak:.3}"));
        }
        println!("{name:<18} {:>16} {:>16} {:>16}", row[0], row[1], row[2]);
    }
    println!("  (shipped BPSK250 threshold is 0.40)");
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
