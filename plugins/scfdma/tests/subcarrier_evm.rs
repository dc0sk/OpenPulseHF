//! The per-subcarrier EVM diagnostic must actually localize a narrowband defect.
//!
//! **Why this exists.** `SCFDMA52-64QAM` and `-64QAM-P4` fail on the dual-soundcard hardware
//! loopback while decoding cleanly in-process, and while `SCFDMA52-32QAM` — one constellation order
//! down, same subcarriers, same pilots, same audio — decodes. An AWGN decode-threshold sweep put the
//! mode's floor at 14 dB against a cable measured at 71 dB SNR, so the impairment is not noise-like.
//! Every measurement taken so far was after the DFT de-spread, which averages all 52 subcarriers into
//! every output symbol: past that point one ruined subcarrier and a uniformly degraded band look
//! identical. `scfdma_subcarrier_evm_db` measures before the de-spread, where the distinction still
//! exists.
//!
//! **Why the tests below are about the instrument and not about the modem.** A diagnostic that has
//! not been checked against known inputs is not evidence — this repo has twice had a measurement
//! quietly report the wrong answer (an SRO estimator that read an injected 200 ppm as −6.9 ppm; a
//! capture that recorded a stray tone rather than the waveform under test), and both times the tell
//! was that the numbers looked too clean. So: a noiseless frame must read as near-zero residual, a
//! known SNR must read back proportionally, and — the one that matters — a notch injected at a known
//! frequency must show up on the subcarrier that sits at that frequency, not merely somewhere.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use scfdma_plugin::demodulate::scfdma_subcarrier_evm_db;
use scfdma_plugin::ScFdmaPlugin;

const MODE: &str = "SCFDMA52-64QAM";
/// Subcarrier spacing: 8 kHz / 256-point FFT.
const SC_HZ: f32 = 8000.0 / 256.0;

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        sample_rate: 8000,
        center_frequency: 1500.0,
        ..ModulationConfig::default()
    }
}

fn tx(mode: &str) -> Vec<f32> {
    let payload: Vec<u8> = (0..200u32)
        .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
        .collect();
    ScFdmaPlugin::new()
        .modulate(&payload, &cfg(mode))
        .expect("modulate")
}

fn awgn(x: &[f32], snr_db: f32, seed: &mut u64) -> Vec<f32> {
    let sp = x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32;
    let sd = (sp / 10f32.powf(snr_db / 10.0)).sqrt();
    x.iter()
        .map(|&s| {
            *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let u1 = (((*seed >> 40) as f32) / ((1u64 << 24) as f32)).max(1e-6);
            *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let u2 = ((*seed >> 40) as f32) / ((1u64 << 24) as f32);
            s + sd * (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
        })
        .collect()
}

/// A two-pole notch at `f0` with roughly `bw` Hz of width.
fn notch(x: &[f32], fs: f32, f0: f32, bw: f32) -> Vec<f32> {
    let r = 1.0 - std::f32::consts::PI * bw / fs;
    let w = std::f32::consts::TAU * f0 / fs;
    let (a1, a2) = (-2.0 * r * w.cos(), r * r);
    let (b1, b2) = (-2.0 * w.cos(), 1.0);
    let (mut y1, mut y2, mut x1, mut x2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    x.iter()
        .map(|&s| {
            let y = s + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            x2 = x1;
            x1 = s;
            y2 = y1;
            y1 = y;
            y
        })
        .collect()
}

fn mean_db(e: &[(usize, f32)]) -> f32 {
    let v: Vec<f32> = e.iter().map(|(_, d)| *d).filter(|d| d.is_finite()).collect();
    v.iter().sum::<f32>() / v.len() as f32
}

fn worst(e: &[(usize, f32)]) -> (usize, f32) {
    e.iter()
        .filter(|(_, d)| d.is_finite())
        .fold((0usize, f32::NEG_INFINITY), |acc, &(sc, d)| {
            if d > acc.1 {
                (sc, d)
            } else {
                acc
            }
        })
}

/// THE GATE: a notch at a known frequency must land on the subcarrier at that frequency.
///
/// This is the property the whole diagnostic exists for. If the worst subcarrier were merely
/// somewhere in the band, the measurement could not distinguish a narrowband defect from a broadband
/// one, which is exactly the ambiguity that made every post-despread measurement inconclusive.
#[test]
fn a_notch_is_localized_to_the_subcarrier_at_its_frequency() {
    let clean = tx(MODE);
    for f0 in [1000.0f32, 1500.0, 2000.0] {
        let expected_sc = (f0 / SC_HZ).round() as usize;
        let e = scfdma_subcarrier_evm_db(&notch(&clean, 8000.0, f0, 40.0), MODE)
            .expect("sync and measure a notched frame");
        let (sc, db) = worst(&e);
        assert_eq!(
            sc, expected_sc,
            "a notch at {f0} Hz should peak on subcarrier {expected_sc} ({} Hz), but the worst \
             subcarrier was {sc} ({} Hz) at {db:.1} dB — the diagnostic is not localizing, so no \
             conclusion drawn from it about which subcarriers are damaged would be trustworthy",
            expected_sc as f32 * SC_HZ,
            sc as f32 * SC_HZ
        );
        assert!(
            db > mean_db(&e) + 6.0,
            "the notched subcarrier {sc} read {db:.1} dB against a band mean of {:.1} dB — under a \
             6 dB margin the peak is not distinguishable from ordinary spread",
            mean_db(&e)
        );
    }
}

/// A noiseless frame must read as essentially no residual.
///
/// Catches the failure mode where the reconstruction is mis-scaled: any constant scale error between
/// the re-spread decisions and the equalized subcarriers would show as a large floor on every
/// subcarrier, and would then swamp whatever real structure the measurement is meant to find.
#[test]
fn a_noiseless_frame_reads_as_near_zero_residual() {
    let e = scfdma_subcarrier_evm_db(&tx(MODE), MODE).expect("measure a clean frame");
    let m = mean_db(&e);
    assert!(
        m < -40.0,
        "a noiseless frame measured {m:.1} dB mean EVM; anything above −40 dB means the diagnostic \
         has a scaling or alignment error of its own rather than a clean reference"
    );
}

/// The reading must track injected SNR, and must not blame any one subcarrier for broadband noise.
///
/// The second half is the counterpart to the notch test: broadband noise must look broadband. A
/// diagnostic that peaked somewhere under uniform noise would manufacture a narrowband explanation.
#[test]
fn broadband_noise_reads_broadband_and_tracks_snr() {
    let clean = tx(MODE);
    let mut prev = f32::NEG_INFINITY;
    for snr in [30.0f32, 20.0, 14.0] {
        let mut seed = 7u64;
        let e = scfdma_subcarrier_evm_db(&awgn(&clean, snr, &mut seed), MODE)
            .expect("measure a noisy frame");
        let m = mean_db(&e);
        assert!(
            m > prev,
            "mean EVM must worsen as SNR drops: {snr} dB gave {m:.1} dB, no worse than the previous \
             step's {prev:.1} dB"
        );
        prev = m;

        let (sc, db) = worst(&e);
        assert!(
            db < m + 12.0,
            "under uniform AWGN, subcarrier {sc} stood {:.1} dB above the {m:.1} dB band mean — a \
             broadband impairment must not concentrate on one subcarrier, or the notch test's \
             localization proves nothing",
            db - m
        );
    }
}

/// The absolute subcarrier indices must be real, in order, and skip the pilots.
///
/// Without this the vector is uninterpretable across modes: `SCFDMA52-64QAM` carries 52 data
/// subcarriers at pilot spacing 5 and `-64QAM-P4` carries 49 at spacing 4, so the k-th entry of one
/// is not the same frequency as the k-th of the other — which is precisely the comparison the
/// diagnostic was built to make.
#[test]
fn subcarrier_indices_are_absolute_ascending_and_pilot_free() {
    for (mode, expect_len) in [(MODE, 52usize), ("SCFDMA52-64QAM-P4", 49)] {
        let e = scfdma_subcarrier_evm_db(&tx(mode), mode).expect("measure");
        assert_eq!(e.len(), expect_len, "{mode}: wrong data-subcarrier count");
        assert!(
            e.windows(2).all(|w| w[0].0 < w[1].0),
            "{mode}: subcarrier indices must be strictly ascending"
        );
        for &(sc, _) in &e {
            assert!(
                (16..=80).contains(&sc),
                "{mode}: subcarrier {sc} is outside the occupied band 16..=80"
            );
        }
    }
}
