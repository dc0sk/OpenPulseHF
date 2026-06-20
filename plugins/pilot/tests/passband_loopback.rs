//! Passband loopback for the pilot-framed QPSK plugin: modulate → (channel) →
//! demodulate, validating the full audio chain plus pilot-aided recovery.

use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};
use pilot_plugin::PilotPlugin;

fn cfg(center: f32) -> ModulationConfig {
    ModulationConfig {
        center_frequency: center,
        sample_rate: 8000,
        mode: "PILOT-QPSK500".to_string(),
        ..Default::default()
    }
}

const PAYLOAD: &[u8] = b"pilot-framed QPSK passband loopback 0123456789 abcdefghij KN4xyz";

#[test]
fn clean_loopback_with_leadin() {
    let plugin = PilotPlugin::new();
    let audio = plugin.modulate(PAYLOAD, &cfg(1500.0)).unwrap();
    assert!(!audio.is_empty());

    // Prepend lead-in silence so the demodulator must locate the onset.
    let mut buf = vec![0.0f32; 640];
    buf.extend_from_slice(&audio);

    let out = plugin.demodulate(&buf, &cfg(1500.0)).unwrap();
    assert!(
        out.len() >= PAYLOAD.len() && &out[..PAYLOAD.len()] == PAYLOAD,
        "clean loopback must recover the payload (got {} bytes)",
        out.len()
    );
}

#[test]
fn loopback_through_carrier_offset() {
    // TX carrier 2 Hz above the RX's nominal: the downconverter leaves a residual
    // that the symbol-level pilot tracker removes. The offset that the *passband
    // POC* tolerates is bounded here by onset precision, not by the tracker: with
    // rectangular pulses and integer-sample onset (no timing recovery yet) a
    // larger offset shifts the coherent preamble-correlation peak by a whole
    // sample, straddling symbol boundaries. Sub-sample timing recovery (Gardner)
    // plus a coarse-CFO stage — and, in normal use, the engine's AFC chain — lift
    // this in the integration step; the symbol-level codec itself already tracks
    // far larger offsets (see frame.rs `round_trip_through_carrier_frequency_offset`).
    let plugin = PilotPlugin::new();
    let audio = plugin.modulate(PAYLOAD, &cfg(1502.0)).unwrap();
    let out = plugin.demodulate(&audio, &cfg(1500.0)).unwrap();
    assert!(
        out.len() >= PAYLOAD.len() && &out[..PAYLOAD.len()] == PAYLOAD,
        "pilot tracking must recover the payload through a carrier offset (got {} bytes)",
        out.len()
    );
}

fn cfg_mode(mode: &str, center: f32) -> ModulationConfig {
    ModulationConfig {
        center_frequency: center,
        sample_rate: 8000,
        mode: mode.to_string(),
        ..Default::default()
    }
}

#[test]
fn rrc_clean_loopback_all_modes() {
    let plugin = PilotPlugin::new();
    for mode in [
        "PILOT-QPSK500-RRC",
        "PILOT-8PSK500-RRC",
        "PILOT-16QAM500-RRC",
        "PILOT-32APSK500-RRC",
    ] {
        let audio = plugin.modulate(PAYLOAD, &cfg_mode(mode, 1500.0)).unwrap();
        assert!(!audio.is_empty(), "{mode}: empty audio");
        let peak = audio.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(peak <= 1.0, "{mode}: peak {peak} clips");
        let mut buf = vec![0.0f32; 640];
        buf.extend_from_slice(&audio);
        let out = plugin.demodulate(&buf, &cfg_mode(mode, 1500.0)).unwrap();
        assert!(
            out.len() >= PAYLOAD.len() && &out[..PAYLOAD.len()] == PAYLOAD,
            "{mode}: RRC clean loopback must recover payload (got {} bytes)",
            out.len()
        );
    }
}

/// Coarse periodogram power in a frequency band [lo, hi] Hz via naive DFT.
fn band_power(sig: &[f32], fs: f32, lo: f32, hi: f32) -> f32 {
    let bins = 200usize;
    let mut total = 0.0f32;
    let mut inband = 0.0f32;
    for b in 0..bins {
        let f = b as f32 / bins as f32 * (fs / 2.0);
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (n, &x) in sig.iter().enumerate() {
            let ph = -2.0 * std::f32::consts::PI * f * n as f32 / fs;
            re += x * ph.cos();
            im += x * ph.sin();
        }
        let p = re * re + im * im;
        total += p;
        if f >= lo && f <= hi {
            inband += p;
        }
    }
    inband / total.max(1e-12)
}

#[test]
fn rrc_occupies_less_bandwidth_than_rectangular() {
    let plugin = PilotPlugin::new();
    let fs = 8000.0;
    // RRC at α=0.35, 500 baud → ~675 Hz → within ±400 Hz of fc=1500.
    let rect = plugin
        .modulate(PAYLOAD, &cfg_mode("PILOT-QPSK500", 1500.0))
        .unwrap();
    let rrc = plugin
        .modulate(PAYLOAD, &cfg_mode("PILOT-QPSK500-RRC", 1500.0))
        .unwrap();
    // Fraction of power OUTSIDE the RRC main band (±400 Hz of carrier).
    let oob = |s: &[f32]| 1.0 - band_power(s, fs, 1100.0, 1900.0);
    let rect_oob = oob(&rect);
    let rrc_oob = oob(&rrc);
    eprintln!("out-of-band power: rectangular={rect_oob:.3}  rrc={rrc_oob:.3}");
    assert!(
        rrc_oob < rect_oob * 0.5,
        "RRC must put far less power out of band (rect {rect_oob:.3} vs rrc {rrc_oob:.3})"
    );
}
