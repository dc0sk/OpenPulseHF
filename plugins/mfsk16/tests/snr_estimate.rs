//! MFSK16 `estimate_snr_db` (REQ-WSIG-01, PR-1): the non-coherent symbol-domain SNR estimate must track
//! the true channel SNR, so the receiver-led ladder can climb the sub-floor rung out of SL1. The M2M4
//! fallback reads fading as noise and would pin SL1 — a self-sealing trapdoor.

use mfsk16_plugin::Mfsk16Plugin;
use openpulse_channel::{awgn::AwgnChannel, AwgnConfig, ChannelModel};
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

fn cfg() -> ModulationConfig {
    ModulationConfig {
        mode: "MFSK16".into(),
        center_frequency: 1500.0,
        sample_rate: 8000,
        ..Default::default()
    }
}

fn est_at(snr_db: f32) -> f32 {
    let p = Mfsk16Plugin::new();
    let data: Vec<u8> = (0..255u16).map(|i| i as u8).collect();
    let tx = p.modulate(&data, &cfg()).expect("modulate");
    let rx = AwgnChannel::new(AwgnConfig::new(snr_db, Some(7)))
        .expect("awgn")
        .apply(&tx);
    p.estimate_snr_db(&rx, &cfg()).expect("snr estimate")
}

#[test]
fn snr_estimate_tracks_true_snr() {
    let low = est_at(0.0);
    let high = est_at(15.0);
    assert!(
        high > low + 4.0,
        "MFSK16 estimate_snr_db must rise with true SNR (0 dB → {low:.1}, 15 dB → {high:.1})"
    );
}

/// Audit DSP#2 regression gate: the estimate must be on the true full-band SNR **scale** the ladder's
/// floors/ceilings use — not the ~21 dB-hot per-Goertzel-bin scale. Without the processing-gain subtraction
/// a 0 dB channel reported ~+20 dB, always clearing SL1's 5 dB climb-out ceiling, so the sub-floor rung
/// self-ejected to dead BPSK31 after every decode. At 0 dB true it must read well below that ceiling.
#[test]
fn snr_estimate_intercept_is_on_the_channel_scale() {
    let at0 = est_at(0.0);
    assert!(
        at0 < 5.0,
        "estimate_snr_db at 0 dB true = {at0:.1}; must be below SL1's 5 dB ceiling (it was ~+20 dB \
         before the processing-gain fix, which pinned the ladder off the sub-floor rung)"
    );
}
