//! FSK4 plugin integration tests: loopback correctness and channel degradation.

use fsk4_plugin::Fsk4Plugin;
use openpulse_channel::{awgn::AwgnChannel, AwgnConfig};
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

fn ack_config() -> ModulationConfig {
    ModulationConfig {
        mode: "FSK4-ACK".to_string(),
        sample_rate: 8000,
        center_frequency: 1050.0,
        ..ModulationConfig::default()
    }
}

/// FSK4-ACK loopback over a clean channel: recovered bytes must match the transmitted payload.
#[test]
fn fsk4_ack_clean_loopback() {
    let plugin = Fsk4Plugin::new();
    let cfg = ack_config();
    let payload = [0x01u8, 0x02, 0x03, 0x04, 0x05];
    let samples = plugin.modulate(&payload, &cfg).unwrap();
    let recovered = plugin.demodulate(&samples, &cfg).unwrap();
    assert_eq!(recovered, payload);
}

/// FSK4-ACK at severe AWGN (-20 dB SNR): demodulation should produce at least one bit error.
///
/// FSK4's Goertzel integration over 80 samples/symbol gives ~19 dB processing gain, so
/// only SNR below about -16 dB reliably causes errors; -20 dB is chosen as a safe margin.
#[test]
fn fsk4_ack_awgn_minus20db_degrades() {
    use openpulse_channel::ChannelModel;
    let plugin = Fsk4Plugin::new();
    let cfg = ack_config();
    let payload = [0xAAu8, 0x55, 0xAA, 0x55, 0xAA];
    let samples = plugin.modulate(&payload, &cfg).unwrap();
    let mut channel = AwgnChannel::new(AwgnConfig::new(-20.0, Some(77))).unwrap();
    let noisy = channel.apply(&samples);
    let recovered = plugin.demodulate(&noisy, &cfg).unwrap();
    assert_ne!(
        recovered.as_slice(),
        payload.as_slice(),
        "FSK4-ACK should degrade at -20 dB SNR"
    );
}
