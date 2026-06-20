//! Characterization: freq_acquire CFO accuracy vs the per-plugin estimate_afc_hz,
//! on real modulated preambles across applied offsets. Answers: is freq_acquire a
//! better CFO estimator (esp. at large offset), and does the preamble suffice?

use openpulse_core::iq::hilbert_iq;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use openpulse_dsp::freq_acquire::acquire;

fn plugin_for(mode: &str) -> Box<dyn ModulationPlugin> {
    if mode.starts_with("BPSK") {
        Box::new(bpsk_plugin::BpskPlugin::new())
    } else if mode.starts_with("QPSK") {
        Box::new(qpsk_plugin::QpskPlugin::new())
    } else if mode.starts_with("8PSK") {
        Box::new(psk8_plugin::Psk8Plugin::new())
    } else {
        Box::new(qam64_plugin::Qam64Plugin::new())
    }
}

fn cfg(mode: &str, center: f32) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        sample_rate: 8000,
        center_frequency: center,
        ..Default::default()
    }
}

#[test]
#[ignore = "characterization: prints CFO-estimator accuracy; not a gate"]
fn freq_acquire_vs_afc_estimator() {
    let payload = b"freqacq-characterization-payload-0123456789abcdef";
    let offsets = [-50.0f32, -25.0, -10.0, 10.0, 25.0, 50.0, 100.0, 200.0];
    let modes = ["BPSK250", "QPSK500", "8PSK500", "64QAM500"];
    let fs = 8000.0f32;

    eprintln!(
        "\n{:<10} {:>7} | {:>12} {:>12}",
        "mode", "Δf", "afc_err", "freqacq_err"
    );
    for mode in modes {
        let plug = plugin_for(mode);
        let pre_samps = plug
            .frame_geometry(&cfg(mode, 1500.0))
            .map(|g| g.preamble_samples)
            .unwrap_or(1024);
        // clean reference preamble in complex baseband (carrier 1500 removed)
        let ref_pb = plug.modulate(payload, &cfg(mode, 1500.0)).unwrap();
        let (ri, rq) = hilbert_iq(&ref_pb, 1500.0, fs);
        let reference: Vec<(f32, f32)> = ri
            .iter()
            .zip(&rq)
            .take(pre_samps)
            .map(|(&i, &q)| (i, q))
            .collect();

        for off in offsets {
            // TX carrier at 1500+off; RX mixes at 1500 → residual = off
            let tx_pb = plug.modulate(payload, &cfg(mode, 1500.0 + off)).unwrap();
            // current estimator (passband + config center 1500)
            let afc = plug.estimate_afc_hz(&tx_pb, &cfg(mode, 1500.0));
            let afc_err = afc.map(|e| (e - off).abs()).unwrap_or(f32::NAN);
            // freq_acquire on baseband
            let (ti, tq) = hilbert_iq(&tx_pb, 1500.0, fs);
            let rx: Vec<(f32, f32)> = ti.iter().zip(&tq).map(|(&i, &q)| (i, q)).collect();
            let fa = acquire(&rx, &reference, 0, 64).map(|a| a.cfo_cycles_per_sample * fs);
            let fa_err = fa.map(|e| (e - off).abs()).unwrap_or(f32::NAN);
            eprintln!(
                "{:<10} {:>7.0} | {:>12.1} {:>12.1}",
                mode, off, afc_err, fa_err
            );
        }
    }
}
