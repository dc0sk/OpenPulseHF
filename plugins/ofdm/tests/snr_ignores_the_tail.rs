//! The OFDM SNR estimate measures the frame, not the noise the receive slice carries after it
//! (2026-11-03: a 16QAM frame read 41.6 dB alone and 6.1 dB with one second of noise after it, under
//! the `fast` profile's floors, so the ladder never climbed on SNR across the OFDM rungs).

use ofdm_plugin::OfdmPlugin;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

fn noise(n: usize, amp: f32, st: &mut u64) -> Vec<f32> {
    (0..n)
        .map(|_| {
            *st ^= *st << 13;
            *st ^= *st >> 7;
            *st ^= *st << 17;
            ((*st >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * amp * 3.46
        })
        .collect()
}

#[test]
fn trailing_noise_does_not_lower_the_snr_reading() {
    let plugin = OfdmPlugin::new();
    for mode in ["OFDM52", "OFDM52-16QAM", "OFDM52-64QAM"] {
        let cfg = ModulationConfig {
            mode: mode.into(),
            center_frequency: 1500.0,
            ..ModulationConfig::default()
        };
        let tx = plugin.modulate(&[0x5a; 120], &cfg).unwrap();
        for amp in [0.002f32, 0.02] {
            let mut st = 7u64;
            let mut frame = vec![0.0f32; 800];
            frame.extend(&tx);
            let floor = noise(frame.len(), amp, &mut st);
            let frame: Vec<f32> = frame.iter().zip(&floor).map(|(a, b)| a + b).collect();
            let mut with_tail = frame.clone();
            with_tail.extend(noise(8000, amp, &mut st));
            let alone = plugin
                .estimate_snr_db(&frame, &cfg)
                .expect("SNR of the frame");
            let tail = plugin
                .estimate_snr_db(&with_tail, &cfg)
                .expect("SNR with the tail");
            assert!(
                tail >= alone - 1.0,
                "{mode}, noise {amp}: {tail:.1} dB with 1 s of noise after the frame against \
                 {alone:.1} dB for the frame alone: the estimate counts the tail as data"
            );
        }
    }
}
