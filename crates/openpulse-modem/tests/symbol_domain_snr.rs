//! Per-plugin symbol-domain SNR (`ModulationPlugin::estimate_snr_db`) tracks SNR where M2M4 saturates.
//!
//! M2M4 assumes a constant-modulus *envelope*; on a pulse-shaped waveform the crossfade/RRC envelope
//! variation folds into its "noise" term and its output stops tracking SNR — measured flat ~11 dB on
//! 8PSK500 from 10 to 30 dB, which is what capped the receiver-led ladder around SL8. The per-plugin
//! estimate instead measures noise from the component of each equalized symbol *orthogonal* to its
//! decision (`psk_symbol_noise_var`), so it keeps rising with SNR up to the mode's residual-EVM floor.
//!
//! These gates pin that difference: over an AWGN sweep the plugin estimate must span far more dB than
//! M2M4 across the low-to-mid range where the promotion decisions live. (It saturates at the EVM floor
//! at high SNR — expected and safe, since a rate decision only needs "high enough".)

use openpulse_audio::LoopbackBackend;
use openpulse_channel::{awgn::AwgnChannel, AwgnConfig, ChannelModel};
use openpulse_core::plugin::ModulationPlugin;
use openpulse_core::ModulationConfig;
use openpulse_modem::engine::ModemEngine;
use psk8_plugin::Psk8Plugin;
use qpsk_plugin::QpskPlugin;

const PAYLOAD: &[u8] = b"symbol-domain SNR gate payload, sixty-four bytes AAAAAAAAAAAAA";

fn tx(mode: &str, plugin: Box<dyn ModulationPlugin>) -> Vec<f32> {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine.register_plugin(plugin).expect("register");
    engine.transmit_with_fec(PAYLOAD, mode, None).expect("tx");
    backend.drain_samples()
}

fn awgn(tx: &[f32], snr_db: f32, seed: u64) -> Vec<f32> {
    AwgnChannel::new(AwgnConfig::new(snr_db, Some(seed)))
        .expect("awgn")
        .apply(tx)
}

fn mod_cfg(mode: &str) -> ModulationConfig {
    ModulationConfig {
        mode: mode.to_string(),
        center_frequency: 1500.0,
        ..ModulationConfig::default()
    }
}

/// Two properties, both on identical AWGN samples:
///   1. the plugin estimate **tracks** — it rises materially from low (8 dB) to mid (20 dB) SNR;
///   2. it does not **saturate** where M2M4 does — at high SNR (32 dB) M2M4 flattens near ~15 dB
///      (its crossfade-envelope ceiling), so a rung whose floor is above that can never be promoted
///      to; the plugin reads far higher, which is the whole unlock.
fn assert_tracks(mode: &str, make: impl Fn() -> Box<dyn ModulationPlugin>) {
    let signal = tx(mode, make());
    let cfg = mod_cfg(mode);
    let fs = 8000.0f32;
    let p = make();

    let measure = |snr: f32, seed: u64| -> (f32, f32) {
        let faded = awgn(&signal, snr, seed);
        let est = p
            .estimate_snr_db(&faded, &cfg)
            .expect("plugin returns an estimate");
        let m2m4 = openpulse_core::snr_estimate::m2m4_snr_db_gated_from_real(&faded, 1500.0, fs);
        (est, m2m4)
    };
    let (plugin_lo, _) = measure(8.0, 300);
    let (plugin_mid, _) = measure(20.0, 301);
    let (plugin_hi, m2m4_hi) = measure(32.0, 302);

    eprintln!(
        "{mode}: plugin 8->{plugin_lo:.1} 20->{plugin_mid:.1} 32->{plugin_hi:.1}; M2M4 32->{m2m4_hi:.1}"
    );
    assert!(
        plugin_mid > plugin_lo + 4.0,
        "{mode}: plugin estimate must rise with SNR (8->20 dB): {plugin_lo:.1} -> {plugin_mid:.1}"
    );
    assert!(
        plugin_hi > m2m4_hi + 5.0,
        "{mode}: at 32 dB the plugin must read well above M2M4's saturation ceiling — plugin \
         {plugin_hi:.1} dB vs M2M4 {m2m4_hi:.1} dB (M2M4 caps the ladder; the plugin unlocks it)"
    );
}

#[test]
fn qpsk500_symbol_snr_tracks() {
    assert_tracks("QPSK500", || Box::new(QpskPlugin::new()));
}

#[test]
fn psk8_500_symbol_snr_tracks() {
    assert_tracks("8PSK500", || Box::new(Psk8Plugin::new()));
}
