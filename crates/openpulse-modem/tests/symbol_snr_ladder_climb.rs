//! End-to-end: the per-plugin symbol-domain SNR lets the receiver-led OTA ladder climb **past the
//! M2M4 cap** on a strong channel.
//!
//! M2M4 saturates near ~15 dB on the crossfade-enveloped PSK rungs (see `symbol_domain_snr.rs`), so
//! a rung whose SNR ceiling is above that can never be promoted to — the documented "ladder capped
//! ~SL8" symptom. With `ModulationPlugin::estimate_snr_db` wired into the OTA decision (`rx_snr_db`),
//! the receiver reads the true high SNR on QPSK500/8PSK500 and steps up through them.
//!
//! Real stack: two `ModemEngine`s run the actual `OtaRateController`, bridged forward (data, at the
//! MODCOD FEC) and reverse (FSK4 ACK) through AWGN. No injected SNR — the climb is driven by the
//! engine's own estimate on the faded capture.

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::{AwgnConfig, ChannelModel};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::channel_sim::bridge_through;
use openpulse_modem::engine::ModemEngine;
use psk8_plugin::Psk8Plugin;
use qpsk_plugin::QpskPlugin;
use scfdma_plugin::ScFdmaPlugin;

const PAYLOAD: &[u8] = b"symbol-domain SNR ladder-climb payload over a strong AWGN channel AA";

fn make_engine() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut engine = ModemEngine::new(Box::new(backend.clone_shared()));
    engine.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    engine.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    engine.register_plugin(Box::new(Psk8Plugin::new())).unwrap();
    engine
        .register_plugin(Box::new(ScFdmaPlugin::new()))
        .unwrap();
    // hpx_hf SL11+ is OFDM after the dense-rung re-seat; SL10 stays SC-FDMA (narrowband).
    engine
        .register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
        .unwrap();
    engine.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    engine.start_ota_session(SessionProfile::hpx_hf());
    (engine, backend)
}

struct OtaLink {
    iss: ModemEngine,
    irs: ModemEngine,
    iss_lb: LoopbackBackend,
    irs_lb: LoopbackBackend,
}

impl OtaLink {
    fn new() -> Self {
        let (iss, iss_lb) = make_engine();
        let (irs, irs_lb) = make_engine();
        Self {
            iss,
            irs,
            iss_lb,
            irs_lb,
        }
    }

    /// One ISS→IRS data frame **at the current MODCOD FEC** (the dense rungs only ever run
    /// FEC-protected), reverse ACK IRS→ISS. Returns the level the ISS transmitted at.
    fn exchange(&mut self, fwd: &mut dyn ChannelModel, rev: &mut dyn ChannelModel) -> SpeedLevel {
        let tx_mode = self.iss.ota_tx_mode().expect("OTA active").to_owned();
        let tx_fec = self.iss.ota_tx_fec();
        let tx_level = self.iss.ota_tx_level().unwrap();
        self.iss
            .transmit_with_fec_mode(PAYLOAD, &tx_mode, tx_fec, None)
            .unwrap();
        bridge_through(&self.iss_lb, &self.irs_lb, fwd);

        let _ = self.irs.respond_arq_ota("climb", None);
        bridge_through(&self.irs_lb, &self.iss_lb, rev);
        if let Ok(ack) = self.iss.receive_ack_with_short_fec(None) {
            self.iss.apply_ota_ack(&ack);
        }
        tx_level
    }
}

/// On a strong (35 dB) AWGN channel the ladder must climb above SL8 — the level where the M2M4
/// saturation ceiling (~15 dB) would otherwise pin it. Reaching SL9 (8PSK500) or beyond means the
/// receiver read a true SNR above M2M4's ceiling, which only the symbol-domain estimator provides.
#[test]
fn strong_channel_climbs_past_the_m2m4_cap() {
    let mut link = OtaLink::new();
    let mut max_level = SpeedLevel::Sl2;
    for i in 0..24 {
        let mut fwd = AwgnChannel::new(AwgnConfig::new(35.0, Some(4000 + i))).unwrap();
        let mut rev = AwgnChannel::new(AwgnConfig::new(35.0, Some(9000 + i))).unwrap();
        let level = link.exchange(&mut fwd, &mut rev);
        if level > max_level {
            max_level = level;
        }
    }
    eprintln!("strong-channel climb reached {max_level:?}");
    assert!(
        max_level >= SpeedLevel::Sl9,
        "a 35 dB channel must climb past the M2M4 SL8 cap into the 8PSK/SC-FDMA rungs; \
         reached only {max_level:?}"
    );
}
