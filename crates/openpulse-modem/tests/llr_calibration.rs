//! Every soft demodulator must emit *calibrated* LLRs: magnitude ∝ 1/σ².
//!
//! `openpulse_dsp::constellation::symbol_llrs` divides distances by `noise_var`, so a true
//! log-likelihood ratio grows as the noise falls. Nothing that decodes a single frame notices — soft
//! Viterbi, min-sum LDPC and max-log turbo are all scale-invariant — but HARQ soft combining across
//! receive attempts does: `openpulse_core::fec::combine_llrs_map` sums the attempts, so an
//! uncalibrated attempt from a deep fade votes exactly as loudly as a clean one.
//!
//! Before this was enforced, `mean(|LLR|)` was flat in SNR for BPSK, QPSK, 8PSK and 64QAM (measured
//! 1.00× across 8→24 dB), and a three-attempt HARQ set with one −14 dB frame needed 9.0 dB more SNR on
//! BPSK250 than it does now.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

fn cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        sample_rate: 8000,
        center_frequency: 1500.0,
        ..ModulationConfig::default()
    }
}

/// Deterministic AWGN at the requested SNR relative to the signal's own power.
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

/// `mean(|LLR|)` at `snr_db`, averaged over 8 noise realisations.
fn mean_abs_llr(p: &dyn ModulationPlugin, mode: &str, snr_db: f32) -> f32 {
    let payload: Vec<u8> = (0..96u32)
        .map(|i| (i.wrapping_mul(37) & 0xff) as u8)
        .collect();
    let tx = p.modulate(&payload, &cfg(mode)).expect("modulate");
    let mut acc = 0.0f32;
    for t in 0..8u64 {
        let mut seed = 11 + t * 977;
        let rx = awgn(&tx, snr_db, &mut seed);
        let llrs = p.demodulate_soft(&rx, &cfg(mode)).expect("demodulate_soft");
        assert!(!llrs.is_empty(), "{mode}: no LLRs at {snr_db} dB");
        acc += llrs.iter().map(|v| v.abs()).sum::<f32>() / llrs.len() as f32;
    }
    acc / 8.0
}

/// A calibrated demodulator's LLR magnitude must *grow* with SNR. The ideal is 1/σ², i.e. ×6.31 over
/// 8 dB; the achievable factor is bounded below that by each receiver's own SNR-independent residual
/// (pulse-shaping ISI, equalizer misadjustment, PLL jitter), so this asserts only the direction and a
/// floor — a flat curve means the plugin forgot to divide by its noise variance.
#[test]
fn every_soft_demodulator_emits_llrs_that_grow_with_snr() {
    // (mode, plugin, minimum ×factor of mean|LLR| from 8 dB to 20 dB)
    //
    // The floors are per-plugin because the estimators differ in how much of the residual they can
    // separate from noise: BPSK's differential quadrature cancels the symbol amplitude exactly;
    // 64QAM's decision-directed variance saturates; QPSK/8PSK are dominated above ~12 dB by a
    // receiver-internal residual (see `docs/dev/research/scfdma-improvements.md`, item 10).
    let cases: [(&str, Box<dyn ModulationPlugin>, f32); 6] = [
        ("BPSK250", Box::new(bpsk_plugin::BpskPlugin::new()), 8.0),
        ("QPSK250", Box::new(qpsk_plugin::QpskPlugin::new()), 1.1),
        ("8PSK500", Box::new(psk8_plugin::Psk8Plugin::new()), 1.4),
        ("64QAM1000", Box::new(qam64_plugin::Qam64Plugin::new()), 4.0),
        (
            "SCFDMA52-16QAM",
            Box::new(scfdma_plugin::ScFdmaPlugin::new()),
            8.0,
        ),
        (
            "PILOT-16QAM500",
            Box::new(pilot_plugin::PilotPlugin::new()),
            2.0,
        ),
    ];

    for (mode, plugin, min_factor) in cases {
        let low = mean_abs_llr(plugin.as_ref(), mode, 8.0);
        let high = mean_abs_llr(plugin.as_ref(), mode, 20.0);
        let factor = high / low;
        assert!(
            factor >= min_factor,
            "{mode}: mean|LLR| grew only ×{factor:.2} from 8→20 dB (need ≥×{min_factor}); \
             a flat curve means the LLRs are not divided by σ² and HARQ combining cannot weight them"
        );
    }
}

/// Adding a deeply-faded receive attempt to a HARQ set must not make decoding *harder*.
///
/// This is the property calibration buys. `combine_llrs_map` sums the attempts, so a −14 dB frame's
/// LLRs are added to the good ones; only their being *small* keeps them from corrupting the sum. With
/// uncalibrated LLRs they arrive at full magnitude and the extra attempt costs several dB.
#[test]
fn a_deeply_faded_extra_attempt_does_not_hurt() {
    use openpulse_audio::LoopbackBackend;
    use openpulse_channel::{awgn::AwgnChannel, AwgnConfig, ChannelModel};
    use openpulse_modem::engine::ModemEngine;

    let make = || {
        let backend = LoopbackBackend::new();
        let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
        engine
            .register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
            .expect("register");
        (engine, backend)
    };
    let payload = b"HARQ deep-fade suppression gate payload";
    let tx = {
        let (mut e, b) = make();
        e.transmit_with_fec(payload, "BPSK250", None).expect("tx");
        b.drain_samples()
    };
    let faded = |snr: f32, seed: u64| {
        AwgnChannel::new(AwgnConfig::new(snr, Some(seed)))
            .expect("awgn")
            .apply(&tx)
    };

    // Lowest SNR (0.5 dB grid) at which the attempt set decodes.
    let threshold = |offsets: &[f32]| -> f32 {
        let seeds = [0xCC01u64, 0xCC02, 0xCC03];
        let mut snr = -8.0f32;
        while snr <= 20.0 {
            let (mut rx, b) = make();
            for (o, s) in offsets.iter().zip(seeds.iter()) {
                b.push_frame(&faded(snr + o, *s));
            }
            if rx
                .receive_with_llr_combining("BPSK250", None, offsets.len())
                .map(|d| d == payload)
                .unwrap_or(false)
            {
                return snr;
            }
            snr += 0.5;
        }
        f32::NAN
    };

    let two_good = threshold(&[0.0, 0.0]);
    let plus_faded = threshold(&[0.0, 0.0, -14.0]);
    assert!(
        two_good.is_finite() && plus_faded.is_finite(),
        "thresholds not found (two_good {two_good}, plus_faded {plus_faded})"
    );
    assert!(
        plus_faded <= two_good + 0.5,
        "a −14 dB attempt made decoding harder: {plus_faded:.1} dB with it vs {two_good:.1} dB without \
         — its LLRs are not being suppressed, i.e. they are not calibrated"
    );
}
