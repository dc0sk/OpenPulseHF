//! RESEARCH HARNESS for deriving preamble-correlation constants. Asserts nothing.
//!
//! Measures the two columns a ρ threshold must sit between: the noise ceiling (recorded idle) and
//! the weakest ρ that still decodes. **Both columns as written here are too narrow to derive a
//! threshold from** — the noise column is two SSB-bandwidth captures and the decode column is AWGN.
//! Measurement showed each of those omissions is load-bearing; see
//! `preamble_rho_fade_and_filter_probe.rs` and the issue it is cited from. Widen both before
//! trusting any number this prints.
//!
//! Run with:
//! `cargo test -p openpulse-modem --no-default-features --test preamble_rho_corpus_survey -- --ignored --nocapture`

use openpulse_core::fec::FecMode;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_dsp::acquisition::IqMatchedFilter;
use openpulse_modem::capture_replay::load_corpus;

const FS: f32 = 8_000.0;
const FC: f32 = 1_500.0;
const PREAMBLE_SYMS: usize = 16;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        sample_rate: 8_000,
        center_frequency: FC,
        ..Default::default()
    }
}

fn sps(baud: f32) -> usize {
    (FS / baud).round() as usize
}

/// The preamble span of a modulated QPSK frame, last symbol dropped (crossfade carries a third of
/// the first data symbol into it).
fn template(mode: &str, baud: f32) -> Vec<f32> {
    let p = qpsk_plugin::QpskPlugin::new();
    let full = p.modulate(&[], &cfg(mode)).expect("modulate");
    let n = sps(baud);
    full[..n * (PREAMBLE_SYMS - 1)].to_vec()
}

/// Replicates the engine: window = `max(preamble, min_frame)` = 17 symbols, search bound = the
/// slack past the template, grid = +-20 Hz at 0.25*fs/tlen.
fn engine_grid(tlen: usize, center: f32, half_width: f32) -> Vec<f32> {
    let step = (0.25 * FS / tlen as f32).max(0.5);
    let n = (half_width / step).round() as i32;
    (-n..=n).map(|k| center + k as f32 * step).collect()
}

fn rho_of(mf: &IqMatchedFilter, window: &[f32], center: f32, half_width: f32) -> Option<f32> {
    let grid = engine_grid(mf.len(), center, half_width);
    mf.search_normalized_over_frequency(window, window.len() - mf.len(), 0.05, FS, &grid)
        .map(|(r, _)| r.rho)
}

fn pct(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return f32::NAN;
    }
    let idx = ((sorted.len() - 1) as f32 * p).round() as usize;
    sorted[idx]
}

const MODES: [(&str, f32); 4] = [
    ("QPSK125", 125.0),
    ("QPSK250", 250.0),
    ("QPSK500", 500.0),
    ("QPSK1000", 1000.0),
];

/// Noise rho over every window of both recorded idle captures.
#[test]
#[ignore = "research harness"]
fn survey_noise_rho() {
    println!("\n== noise rho (recorded idle, engine window/grid) ==");
    println!("mode      tlen  capture             p50    p99    p99.9   max");
    for (mode, baud) in MODES {
        let n = sps(baud);
        let t = template(mode, baud);
        let win = n * (PREAMBLE_SYMS + 1);
        let mf = IqMatchedFilter::new(t.clone());
        for name in ["ic9700-idle-hot.wav", "ft991a-idle.wav"] {
            let cap = load_corpus(name).expect("corpus");
            let mut rhos: Vec<f32> = Vec::new();
            let mut start = 0usize;
            while start + win <= cap.samples.len() {
                if let Some(r) = rho_of(&mf, &cap.samples[start..start + win], 0.0, 20.0) {
                    rhos.push(r);
                }
                start += n;
            }
            rhos.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "{mode:<9} {:<5} {name:<19} {:.3}  {:.3}  {:.3}   {:.3}   (n={})",
                t.len(),
                pct(&rhos, 0.50),
                pct(&rhos, 0.99),
                pct(&rhos, 0.999),
                rhos.last().copied().unwrap_or(f32::NAN),
                rhos.len()
            );
        }
    }
}

/// Rho of a steady tone — the birdie falsifier, swept over grid half-width.
#[test]
#[ignore = "research harness"]
fn survey_tone_rho() {
    println!("\n== steady-tone rho vs grid half-width ==");
    println!("mode      f_tone  +-20    +-50    +-160   +-450");
    for (mode, baud) in MODES {
        let n = sps(baud);
        let t = template(mode, baud);
        let win = n * (PREAMBLE_SYMS + 1);
        let mf = IqMatchedFilter::new(t);
        for f in [1_250.0f32, 1_400.0, 1_500.0, 1_600.0, 1_750.0] {
            let tone: Vec<f32> = (0..win)
                .map(|k| (2.0 * std::f32::consts::PI * f * k as f32 / FS).cos())
                .collect();
            let vals: Vec<f32> = [20.0f32, 50.0, 160.0, 450.0]
                .iter()
                .map(|&hw| rho_of(&mf, &tone, 0.0, hw).unwrap_or(f32::NAN))
                .collect();
            println!(
                "{mode:<9} {f:<7.0} {:.3}   {:.3}   {:.3}   {:.3}",
                vals[0], vals[1], vals[2], vals[3]
            );
        }
    }
    // The real recorded birdie capture, for the modes at the shipped grid.
    let cap = load_corpus("ic9700-tone-1501hz.wav").expect("corpus");
    println!("\n-- recorded ic9700-tone-1501hz.wav, +-20 Hz grid --");
    for (mode, baud) in MODES {
        let n = sps(baud);
        let t = template(mode, baud);
        let win = n * (PREAMBLE_SYMS + 1);
        let mf = IqMatchedFilter::new(t);
        let mut best = 0.0f32;
        let mut start = 0usize;
        while start + win <= cap.samples.len() {
            if let Some(r) = rho_of(&mf, &cap.samples[start..start + win], 0.0, 20.0) {
                best = best.max(r);
            }
            start += n;
        }
        println!("{mode:<9} max rho {best:.3}");
    }
}

/// The decode cliff: rho of a real frame at its true onset, against whether it still decodes.
#[test]
#[ignore = "research harness"]
fn survey_decode_cliff() {
    use openpulse_channel::awgn::AwgnChannel;
    use openpulse_channel::{AwgnConfig, ChannelModel};
    use openpulse_modem::channel_sim::ChannelSimHarness;
    use std::time::Duration;

    let awgn = |snr: f32| {
        AwgnChannel::new(AwgnConfig {
            snr_db: snr,
            seed: Some(7),
        })
        .expect("awgn")
    };

    println!("\n== decode cliff: rho at the true onset vs decode, AWGN ==");
    println!("mode      snr_db  rho     decode");
    for (mode, baud) in [("QPSK125", 125.0f32)] {
        let n = sps(baud);
        let t = template(mode, baud);
        let win = n * (PREAMBLE_SYMS + 1);
        let mf = IqMatchedFilter::new(t);
        for snr in [
            -14.0f32, -12.0, -10.0, -9.0, -8.0, -7.0, -6.0, -5.0, -4.0, -3.0,
        ] {
            // Measure rho on the same waveform the receiver sees.
            let p = qpsk_plugin::QpskPlugin::new();
            let clean = p.modulate(b"decode cliff probe", &cfg(mode)).unwrap();
            let mut ch = awgn(snr);
            let noisy = ch.apply(&clean);
            let rho = rho_of(&mf, &noisy[..win.min(noisy.len())], 0.0, 20.0).unwrap_or(f32::NAN);

            let mut h = ChannelSimHarness::new();
            for eng in [&mut h.tx_engine, &mut h.rx_engine] {
                eng.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
                    .unwrap();
            }
            h.tx_engine
                .transmit_with_fec_mode(b"decode cliff probe", mode, FecMode::Rs, None)
                .expect("tx");
            h.route(&mut awgn(snr));
            let ok = h
                .rx_engine
                .receive_with_fec_mode_timeout(
                    mode,
                    FecMode::Rs,
                    None,
                    Duration::from_millis(8_000),
                )
                .is_ok();
            println!(
                "{mode:<9} {snr:<7.0} {rho:.3}   {}",
                if ok { "OK" } else { "fail" }
            );
        }
    }
}
