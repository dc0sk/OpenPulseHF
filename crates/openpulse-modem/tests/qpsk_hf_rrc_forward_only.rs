//! QPSK1000-HF-RRC runs a forward-only LMS equalizer (no DFE). A coded Watterson sweep showed the old
//! (fwd=11, dfe=2) profile *loses* to forward-only on good_f1 fading (0.60 vs 0.68) and only ties on AWGN
//! and static two-ray ISI — the decision-feedback section propagates errors on a fading channel and buys
//! nothing the forward filter + soft FEC don't already cover. This pins the forward-only fading floor so a
//! future re-introduction of the DFE (which would drop it) is caught.

use openpulse_channel::{watterson::WattersonChannel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;
use qpsk_plugin::QpskPlugin;

const MODE: &str = "QPSK1000-HF-RRC";
const PAYLOAD: &[u8] = b"QPSK1000-HF-RRC forward-only fading gate payload, sixty-four AAA";

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for e in [&mut h.tx_engine, &mut h.rx_engine] {
        e.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    }
    h
}

#[test]
fn forward_only_holds_the_good_f1_coded_floor() {
    let n = 40u32;
    let mut ok = 0u32;
    for s in 0..n {
        let mut h = harness();
        if h.tx_engine
            .transmit_with_fec_mode(PAYLOAD, MODE, FecMode::SoftConcatenated, None)
            .is_err()
        {
            continue;
        }
        let mut cfg = WattersonConfig::good_f1(Some(500 + s as u64));
        cfg.snr_db = 20.0;
        let mut ch = WattersonChannel::new(cfg).unwrap();
        h.route(&mut ch);
        if h.rx_engine
            .receive_with_fec_mode(MODE, FecMode::SoftConcatenated, None)
            .map(|d| d == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    let rate = ok as f32 / n as f32;
    assert!(
        rate >= 0.55,
        "QPSK1000-HF-RRC decoded only {rate:.2} of good_f1 @20 dB coded frames — forward-only measured \
         0.68; a regression (e.g. a re-added DFE, which measured 0.60) drops it"
    );
}
