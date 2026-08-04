//! Deriving BPSK31's OWN rho constants — the work that would let the slow rungs have a veto.
//!
//! Phase 0 of #1062 removed the *cost* barrier to correlating BPSK31's 7936-sample template. It did
//! not remove the correctness barrier: `PREAMBLE_RHO_THRESHOLD` and `PREAMBLE_RHO_GRID_HZ` were both
//! derived on BPSK250 and both are wrong here — the threshold because rho is normalised and this
//! template is 8x longer, the grid because BPSK31's spectral lines are 15.6 Hz apart against a
//! +/-20 Hz default that reaches every one of them.
//!
//! This measures the two columns that a grid derivation needs. The threshold's decode column is
//! separate and expensive (BPSK31 frames run ~12 s), so it is only worth running if a usable gap
//! appears here.
//!
//! **Grid first, because it bounds everything else.** A tone landing exactly ON a line scores
//! rho ~= sqrt(0.5) whatever the grid — narrowing the grid narrows the vulnerable *bands* around
//! each line, it cannot remove them. So the question is not "is there a safe grid" but "how much of
//! the frequency axis stays vulnerable", and the answer is set by the settle residual: the grid need
//! only be wide enough to cover what the settle leaves behind.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_dsp::acquisition::DdcMatchedFilter;
use std::f32::consts::PI;

const FS: f32 = 8_000.0;
const FC: f32 = 1_500.0;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        sample_rate: 8_000,
        center_frequency: FC,
        ..Default::default()
    }
}

/// The engine's derivation, mirrored so this measures what would ship.
fn cutoff_and_decim(occupied_bw: f32, grid_hz: f32, tmpl_len: usize) -> (f32, usize) {
    let cutoff = occupied_bw / 2.0 + grid_hz + occupied_bw * 0.15;
    let by_budget = tmpl_len.div_ceil(2_048);
    let by_bandwidth = ((FS / 2.0) / (cutoff * 1.25)).floor().max(1.0) as usize;
    (cutoff, by_budget.min(by_bandwidth).max(1))
}

fn grid_for(tmpl_len: usize, decim: usize, grid_hz: f32) -> Vec<f32> {
    // Grid step from the DECIMATED template's coherent bandwidth, which is the resolution the
    // correlator actually has after the DDC.
    let eff = (tmpl_len / decim).max(1);
    let step = (0.25 * (FS / decim as f32) / eff as f32).max(0.05);
    let n = (grid_hz / step).round() as i32;
    (-n..=n).map(|k| k as f32 * step).collect()
}

#[test]
#[ignore = "verification"]
fn r1_how_narrow_can_bpsk31_grid_be() {
    let template = bpsk_plugin::modulate::bpsk_preamble_template(&cfg("BPSK31")).expect("template");
    let baud = 31.25f32;
    let occ = 2.0 * baud;
    let first_line = baud / 4.0;

    println!(
        "\nR1: BPSK31 grid derivation. template {} samples, lines at odd multiples of {:.2} Hz",
        template.len(),
        first_line
    );
    println!("    spacing between adjacent lines {:.2} Hz\n", baud / 2.0);
    println!(
        "{:>8} {:>8} {:>6} {:>12} {:>14} {:>18}",
        "grid", "cutoff", "D", "worst tone", "vulnerable %", "verdict"
    );

    for grid_hz in [20.0f32, 6.0, 4.0, 2.0, 1.0, 0.5] {
        let (cutoff, d) = cutoff_and_decim(occ, grid_hz, template.len());
        let mf = DdcMatchedFilter::new(&template, FC, FS, cutoff, d);
        let grid = grid_for(template.len(), d, grid_hz);

        // Sweep one full line spacing; the pattern repeats in position.
        let mut worst = 0.0f32;
        let mut over = 0usize;
        let mut pts = 0usize;
        let mut f = 0.0f32;
        while f <= baud / 2.0 {
            let tone: Vec<f32> = (0..template.len() + 400)
                .map(|k| 0.25 * (2.0 * PI * (FC + f) * k as f32 / FS).cos())
                .collect();
            if let Some((r, _)) = mf.search_normalized_over_frequency(&tone, 0.05, &grid) {
                worst = worst.max(r.rho);
                if r.rho >= 0.40 {
                    over += 1;
                }
                pts += 1;
            }
            f += (baud / 2.0) / 60.0;
        }
        let vulnerable = 100.0 * over as f32 / pts.max(1) as f32;
        let verdict = if grid_hz >= first_line {
            "grid reaches the line"
        } else if vulnerable > 40.0 {
            "usable, wide bands"
        } else {
            "usable"
        };
        println!(
            "{:>7.1}  {:>7.1} {:>6} {:>12.3} {:>13.0}% {:>18}",
            grid_hz, cutoff, d, worst, vulnerable, verdict
        );
    }
    println!("\n  'vulnerable %' is the fraction of the frequency axis where a steady tone would");
    println!("  clear 0.40. It cannot reach zero -- a tone ON a line scores ~0.70 at any grid --");
    println!("  so the grid is sized from the settle residual, and the residual bands are the");
    println!("  irreducible cost of a two-line sync word. BPSK250 at +/-20 Hz sits at ~32%.");
}

/// R2: the false-REJECT side — does a real frame still correlate at the narrow grid?
///
/// R1 picks the grid from the false-accept side alone, and that is half a derivation. The grid
/// exists to absorb whatever frequency error the settle leaves behind; make it too narrow and real
/// frames stop being corroborated, which is the failure the veto's whole placement was chosen to
/// avoid (checking correlation *before* the settle was rejected for exactly this — a matched filter
/// integrates coherently, so rho collapses with residual offset).
///
/// A pass here needs rho to stay high across the residual the settle actually leaves. Measured for
/// BPSK250 at <= 0.3 Hz over a 1056-sample window; BPSK31's settle window is far longer, so its
/// residual should be no worse — but "should" is why this is measured rather than assumed.
#[test]
#[ignore = "verification"]
fn r2_does_a_real_frame_survive_the_narrow_grid() {
    let template = bpsk_plugin::modulate::bpsk_preamble_template(&cfg("BPSK31")).expect("template");
    let frame = bpsk_plugin::BpskPlugin::new()
        .modulate(b"grid derivation", &cfg("BPSK31"))
        .expect("modulate");
    let occ = 2.0 * 31.25f32;

    println!("\nR2: BPSK31 real-frame rho vs residual carrier offset, per candidate grid");
    println!("    (the settle leaves <= 0.3 Hz on BPSK250; anything the grid must absorb)\n");
    print!("{:>10}", "residual");
    for g in [20.0f32, 4.0, 2.0, 1.0, 0.5] {
        print!("{:>12}", format!("±{g} Hz"));
    }
    println!();

    for residual in [0.0f32, 0.1, 0.3, 0.5, 1.0, 2.0] {
        print!("{:>9.1}", residual);
        for grid_hz in [20.0f32, 4.0, 2.0, 1.0, 0.5] {
            let (cutoff, d) = cutoff_and_decim(occ, grid_hz, template.len());
            let mf = DdcMatchedFilter::new(&template, FC, FS, cutoff, d);
            let grid = grid_for(template.len(), d, grid_hz);
            // Frame arriving with a residual carrier error the settle did not remove.
            let shifted: Vec<f32> = frame
                .iter()
                .enumerate()
                .map(|(k, &s)| s * (2.0 * PI * residual * k as f32 / FS).cos())
                .collect();
            let rho = mf
                .search_normalized_over_frequency(&shifted, 0.05, &grid)
                .map(|(r, _)| r.rho)
                .unwrap_or(f32::NAN);
            print!("{rho:>12.3}");
        }
        println!();
    }
    println!("\n  A grid is usable only if rho stays well above threshold across the residual the");
    println!(
        "  settle really leaves. Collapse at a residual the settle can produce means the grid"
    );
    println!("  is too narrow and real frames would stop being corroborated.");
}

/// R3: the NOISE column — BPSK31's own rho ceiling, through the correlator it would really use.
///
/// The threshold must clear this. It cannot be inherited: rho is normalised, so its noise floor is
/// set by template length, and BPSK31's template is 8x BPSK250's. Two effects push in opposite
/// directions and only measurement settles the net — the longer template lowers the ceiling, while
/// the DDC's anti-alias stage lowers it further by removing noise outside the mode's own 60 Hz
/// passband before the correlation happens.
///
/// Reported as an exceedance RATE at a stated observation length, not a 45 s peak. A maximum over
/// many windows is an extreme value that grows with how long you look, and a threshold derived from
/// one says nothing about how often a settle is actually corroborated by noise.
#[test]
#[ignore = "verification"]
fn r3_bpsk31_noise_ceiling() {
    let template = bpsk_plugin::modulate::bpsk_preamble_template(&cfg("BPSK31")).expect("template");
    let occ = 2.0 * 31.25f32;
    let grid_hz = 2.0f32; // derived in R1/R2
    let (cutoff, d) = cutoff_and_decim(occ, grid_hz, template.len());
    let mf = DdcMatchedFilter::new(&template, FC, FS, cutoff, d);
    let grid = grid_for(template.len(), d, grid_hz);

    println!(
        "\nR3: BPSK31 noise ceiling, DDC path (cutoff {cutoff:.1} Hz, D {d}), grid ±{grid_hz} Hz"
    );
    println!("    exceedance rate over independent windows, not a peak\n");
    println!(
        "{:<20} {:>10} {:>10} {:>10} {:>12} {:>12}",
        "band", "windows", "median", "p99", "max", ">=0.40"
    );

    let win = template.len() + 400;
    for (name, lo, hi) in [
        ("white 0-4k", 0.0f32, 4_000.0),
        ("ssb 300-2700", 300.0, 2_700.0),
        ("filter 1250-1750", 1_250.0, 1_750.0),
        ("filter 1470-1530", 1_470.0, 1_530.0),
    ] {
        // 40 windows made p99 == max, i.e. the percentile was the maximum wearing a
        // percentile's name. A tail estimate needs enough samples that the quantile is interior.
        let noise = band_noise(win * 400, lo, hi, 0.05, 4242);
        let mut rhos: Vec<f32> = Vec::new();
        let mut s = 0usize;
        while s + win <= noise.len() {
            if let Some((r, _)) =
                mf.search_normalized_over_frequency(&noise[s..s + win], 0.05, &grid)
            {
                rhos.push(r.rho);
            }
            s += win;
        }
        rhos.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = rhos.len().max(1);
        let median = rhos[n / 2];
        let p99 = rhos[(n * 99 / 100).min(n - 1)];
        let max = *rhos.last().unwrap_or(&f32::NAN);
        let over = rhos.iter().filter(|&&r| r >= 0.40).count();
        println!(
            "{name:<20} {:>10} {median:>10.3} {p99:>10.3} {max:>12.3} {:>12}",
            rhos.len(),
            format!("{}/{}", over, rhos.len())
        );
    }
    println!("\n  A threshold must clear the ceiling AND stay under the weakest decodable frame.");
    println!("  The narrow-filter rows are the ones that killed the QPSK table in #1053 — a 60 Hz");
    println!("  receive filter is not exotic for a 31-baud mode, it is how the mode is operated.");
}

/// Band-limited noise, shared with the equivalence bench's method.
fn band_noise(n: usize, lo: f32, hi: f32, a: f32, seed: u64) -> Vec<f32> {
    use rustfft::{num_complex::Complex, FftPlanner};
    let mut st = seed | 1;
    let mut buf: Vec<Complex<f32>> = (0..n)
        .map(|_| {
            st ^= st >> 12;
            st ^= st << 25;
            st ^= st >> 27;
            let u = (st.wrapping_mul(0x2545F4914F6CDD1D) >> 33) as f32 / (1u64 << 31) as f32;
            Complex::new(u * 2.0 - 1.0, 0.0)
        })
        .collect();
    let mut pl = FftPlanner::new();
    pl.plan_fft_forward(n).process(&mut buf);
    let bin = FS / n as f32;
    for (k, v) in buf.iter_mut().enumerate() {
        let f = if k <= n / 2 {
            k as f32 * bin
        } else {
            (n - k) as f32 * bin
        };
        if f < lo || f > hi {
            *v = Complex::new(0.0, 0.0);
        }
    }
    pl.plan_fft_inverse(n).process(&mut buf);
    let out: Vec<f32> = buf.iter().map(|c| c.re).collect();
    let rms = (out.iter().map(|s| s * s).sum::<f32>() / n as f32)
        .sqrt()
        .max(1e-12);
    out.iter().map(|s| s / rms * a).collect()
}

/// R4: the DECODE column — the weakest rho that still decodes, on the channel this rung exists for.
///
/// The half that #1053 got wrong, and the reason its QPSK table was withdrawn: an AWGN decode
/// column reported a comfortable margin while the fade column showed the decodable-frame and
/// noise distributions overlapping, so no threshold existed at all. BPSK31 is `hpx_hf`'s SL2 and
/// its `initial_level` — the rung every session starts on — with a 3 dB floor and `Rs`, and the
/// channel it exists for is a fade, not AWGN.
///
/// Expensive by construction: `Rs` emits a 255-byte block, so a BPSK31 frame is ~65 s of audio.
/// That cost is the reason this column gets skipped, and skipping it is what withdrew #1053.
#[test]
#[ignore = "verification"]
fn r4_bpsk31_decode_column() {
    use openpulse_channel::watterson::WattersonChannel;
    use openpulse_channel::{ChannelModel, WattersonConfig};
    use openpulse_core::fec::FecMode;
    use openpulse_modem::channel_sim::ChannelSimHarness;
    use std::time::Duration;

    let template = bpsk_plugin::modulate::bpsk_preamble_template(&cfg("BPSK31")).expect("template");
    let occ = 2.0 * 31.25f32;
    let grid_hz = 2.0f32;
    let (cutoff, d) = cutoff_and_decim(occ, grid_hz, template.len());
    let mf = DdcMatchedFilter::new(&template, FC, FS, cutoff, d);
    let grid = grid_for(template.len(), d, grid_hz);

    let snr_db: f32 = std::env::var("R4_SNR")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3.0);
    let seeds: u64 = std::env::var("R4_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6);

    println!("\nR4: BPSK31 + Rs on moderate_f1 @ {snr_db} dB — its own floor, its own channel");
    println!("    grid ±{grid_hz} Hz, DDC cutoff {cutoff:.1} Hz, {seeds} seeds\n");
    println!("{:>6} {:>10} {:>10}", "seed", "rho", "decoded");

    let mut decodable: Vec<f32> = Vec::new();
    let mut all: Vec<f32> = Vec::new();
    for seed in 0..seeds {
        let mut h = ChannelSimHarness::new();
        for e in [&mut h.tx_engine, &mut h.rx_engine] {
            e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
                .unwrap();
        }
        h.rx_engine.set_deterministic_scan_positions(Some(3_000));
        h.rx_engine.set_deterministic_max_iterations(Some(4_000));
        h.tx_engine
            .transmit_with_fec_mode(b"decode column", "BPSK31", FecMode::Rs, None)
            .expect("tx");
        let mut c = WattersonConfig::moderate_f1(Some(seed));
        c.snr_db = snr_db;

        // rho on the same realisation the receiver will see, at the true onset.
        let clean = bpsk_plugin::BpskPlugin::new()
            .modulate(b"decode column", &cfg("BPSK31"))
            .unwrap();
        let mut c2 = WattersonConfig::moderate_f1(Some(seed));
        c2.snr_db = snr_db;
        let faded = WattersonChannel::new(c2).unwrap().apply(&clean);
        let rho = mf
            .search_normalized_over_frequency(
                &faded[..(template.len() + 400).min(faded.len())],
                0.05,
                &grid,
            )
            .map(|(r, _)| r.rho)
            .unwrap_or(f32::NAN);

        h.route(&mut WattersonChannel::new(c).unwrap());
        let ok = h
            .rx_engine
            .receive_with_fec_mode_timeout(
                "BPSK31",
                FecMode::Rs,
                None,
                Duration::from_millis(180_000),
            )
            .is_ok();
        println!("{seed:>6} {rho:>10.3} {ok:>10}");
        all.push(rho);
        if ok {
            decodable.push(rho);
        }
    }
    decodable.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    println!(
        "\n  decoded {}/{}; weakest decodable rho {:?}",
        decodable.len(),
        all.len(),
        decodable.first()
    );
    println!(
        "  A threshold must sit BELOW that and ABOVE the R3 noise ceiling (0.319 wide-filter,"
    );
    println!("  0.426 at 60 Hz). If the weakest decodable rho falls into that range, the two");
    println!("  distributions overlap and BPSK31 publishes NO template — the #1053 outcome.");
}
