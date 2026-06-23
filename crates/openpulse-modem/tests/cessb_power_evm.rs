//! Measurement: Controlled-Envelope SSB (CE-SSB) envelope conditioning — average
//! power gain (PAPR reduction) vs the EVM/BER cost it adds to digital waveforms.
//!
//! For each mode: modulate a payload → real passband; compute the analytic
//! envelope (`hilbert_iq`); derive the CE-SSB peak-stretch limiting **gain** at a
//! few clip ratios and apply it to the passband. Then report, for each ratio:
//!   - average-power gain at fixed peak  = PAPR(baseline) − PAPR(conditioned) [dB]
//!   - clean-channel raw BER             = the self-distortion (EVM) the clip adds
//!   - net BER through fixed-noise AWGN  = power gain vs self-distortion combined
//!     (both signals peak-normalised, same noise floor, so the conditioned signal's
//!     extra average power shows up as a higher effective SNR).
//!
//! Run: `cargo test -p openpulse-modem --no-default-features --test cessb_power_evm -- --nocapture`

use openpulse_core::iq::hilbert_iq;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin, PulseShape};
use openpulse_dsp::cessb;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

const FS: f32 = 8000.0;
const FC: f32 = 1500.0;
const LOOKAHEAD: usize = 16;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.to_string(),
        sample_rate: FS as u32,
        center_frequency: FC,
        pulse_shape: if mode.ends_with("-RRC") {
            PulseShape::Rrc { alpha: 0.35 }
        } else {
            PulseShape::Hann
        },
        ..ModulationConfig::default()
    }
}

fn payload() -> Vec<u8> {
    (0u16..192)
        .map(|i| (i.wrapping_mul(37).wrapping_add(11)) as u8)
        .collect()
}

fn rms(s: &[f32]) -> f32 {
    (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt()
}

fn peak(s: &[f32]) -> f32 {
    s.iter().fold(0.0f32, |m, &x| m.max(x.abs()))
}

fn peak_normalised(s: &[f32]) -> Vec<f32> {
    let p = peak(s).max(f32::MIN_POSITIVE);
    s.iter().map(|&x| x / p).collect()
}

/// Box–Muller AWGN at a fixed sigma (a noise floor independent of signal power,
/// the right model when the PA peak is the constraint).
fn add_awgn(s: &[f32], sigma: f32, rng: &mut StdRng) -> Vec<f32> {
    s.iter()
        .map(|&x| {
            let u1: f32 = rng.gen::<f32>().max(1e-12);
            let u2: f32 = rng.gen::<f32>();
            let n = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos();
            x + sigma * n
        })
        .collect()
}

/// Bit-error rate of a demodulated frame against the payload (0.5 = total failure).
fn ber(recovered: &Result<Vec<u8>, openpulse_core::error::ModemError>, payload: &[u8]) -> f64 {
    match recovered {
        Ok(r) if r.len() >= payload.len() => {
            let errs: u32 = payload
                .iter()
                .zip(r.iter())
                .map(|(a, b)| (a ^ b).count_ones())
                .sum();
            errs as f64 / (payload.len() * 8) as f64
        }
        _ => 0.5,
    }
}

struct Row {
    ratio: f32,
    power_gain_db: f32,
    ber_clean: f64,
    ber_awgn_base: f64,
    ber_awgn_cessb: f64,
}

fn measure(mode: &str, plugin: &dyn ModulationPlugin, awgn_snr_db: f32) -> Vec<Row> {
    let c = cfg(mode);
    let payload = payload();
    let s = plugin.modulate(&payload, &c).expect("modulate");
    let (i, q) = hilbert_iq(&s, FC, FS);
    let env = cessb::envelope(&i, &q);
    let rms_env = rms(&env);
    let papr_base = cessb::papr_db(&s);

    // Fixed AWGN sigma from the peak-normalised BASELINE at the target SNR; the
    // same sigma is applied to the conditioned signal so its extra average power
    // shows up as a higher effective SNR.
    let s_norm = peak_normalised(&s);
    let sigma = rms(&s_norm) / 10f32.powf(awgn_snr_db / 20.0);
    let mut rng = StdRng::seed_from_u64(0xCE55B);
    let ber_awgn_base = ber(
        &plugin.demodulate(&add_awgn(&s_norm, sigma, &mut rng), &c),
        &payload,
    );

    [2.5f32, 2.0, 1.5]
        .iter()
        .map(|&ratio| {
            let level = ratio * rms_env;
            let gain = cessb::peak_stretch_gain(&env, level, LOOKAHEAD);
            let s_cessb = cessb::apply_gain(&s, &gain);
            let papr_cessb = cessb::papr_db(&s_cessb);

            let ber_clean = ber(&plugin.demodulate(&s_cessb, &c), &payload);
            let cessb_norm = peak_normalised(&s_cessb);
            let mut rng = StdRng::seed_from_u64(0xCE55B);
            let ber_awgn_cessb = ber(
                &plugin.demodulate(&add_awgn(&cessb_norm, sigma, &mut rng), &c),
                &payload,
            );

            Row {
                ratio,
                power_gain_db: papr_base - papr_cessb,
                ber_clean,
                ber_awgn_base,
                ber_awgn_cessb,
            }
        })
        .collect()
}

fn report(mode: &str, plugin: &dyn ModulationPlugin, snr: f32) -> Vec<Row> {
    let rows = measure(mode, plugin, snr);
    println!("\n=== {mode}  (AWGN {snr:.0} dB, peak-constrained) ===");
    println!("  clip×rms | avg-power gain | raw BER (clean) | BER base→cessb (AWGN)");
    for r in &rows {
        println!(
            "    {:>4.1}   |   {:>5.2} dB     |   {:>7.4}       |   {:.4} → {:.4}",
            r.ratio, r.power_gain_db, r.ber_clean, r.ber_awgn_base, r.ber_awgn_cessb
        );
    }
    rows
}

#[test]
fn cessb_power_vs_evm_across_modes() {
    use bpsk_plugin::BpskPlugin;
    use ofdm_plugin::OfdmPlugin;
    use qam64_plugin::Qam64Plugin;

    // Amplitude-sensitive single-carrier (Hann, modest PAPR) — shows the EVM cost.
    let qam = report("64QAM500", &Qam64Plugin::new(), 26.0);
    // Multicarrier — the highest PAPR, where CE-SSB pays off most.
    let ofdm = report("OFDM52", &OfdmPlugin::new(), 18.0);
    // Low-PAPR control: a near-constant envelope has little for CE-SSB to clip.
    let bpsk = report("BPSK250", &BpskPlugin::new(), 6.0);

    // Invariant: harder clipping never yields less average-power gain (monotone).
    for rows in [&qam, &ofdm, &bpsk] {
        for w in rows.windows(2) {
            assert!(
                w[1].power_gain_db >= w[0].power_gain_db - 0.01,
                "harder clip must not give less power gain"
            );
        }
    }
    let gain_at = |rows: &[Row], ratio: f32| {
        rows.iter()
            .find(|r| (r.ratio - ratio).abs() < 1e-3)
            .unwrap()
            .power_gain_db
    };
    // The high-PAPR multicarrier signal gains meaningful average power…
    assert!(
        gain_at(&ofdm, 1.5) > 1.0,
        "CE-SSB should recover >1 dB average power on high-PAPR OFDM"
    );
    // …more than a near-constant-envelope BPSK signal (the control).
    assert!(
        gain_at(&ofdm, 1.5) > gain_at(&bpsk, 1.5),
        "the high-PAPR OFDM signal must gain more average power than BPSK"
    );
    assert!(
        gain_at(&bpsk, 1.5) < 0.3,
        "a near-constant-envelope signal has little for CE-SSB to clip"
    );
}
