//! TEMPORARY probe: QPSK250-D decode rate on moderate_f1 (hpx_hf SL6 geometry, the
//! hpx_hf_rungs_survive_fade gate's exact seeds), Rs vs RsStrong, at floor+4 (11 dB) and 20 dB.

use openpulse_channel::{watterson::WattersonChannel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;

const PAYLOAD: &[u8] = b"hpx_hf fade gate payload, sixty-four bytes in total AAAAAAAAAAAA";
const FRAMES: u32 = 12;
const MODE: &str = "QPSK250-D";

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for e in [&mut h.tx_engine, &mut h.rx_engine] {
        e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
            .ok();
    }
    h
}

fn decode_rate(fec: FecMode, snr_db: f32) -> f32 {
    let mut ok = 0u32;
    for f in 0..FRAMES {
        let mut h = harness();
        if h.tx_engine
            .transmit_with_fec_mode(PAYLOAD, MODE, fec, None)
            .is_err()
        {
            continue;
        }
        let mut cfg = WattersonConfig::moderate_f1(Some(8100 + f as u64));
        cfg.snr_db = snr_db;
        let Ok(mut ch) = WattersonChannel::new(cfg) else {
            continue;
        };
        h.route(&mut ch);
        if h.rx_engine
            .receive_with_fec_mode(MODE, fec, None)
            .map(|d| d == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    ok as f32 / FRAMES as f32
}

fn main() {
    for fec in [FecMode::Rs, FecMode::RsStrong] {
        for snr in [11.0f32, 20.0] {
            println!("{MODE} + {fec:?} @ {snr} dB -> {:.2}", decode_rate(fec, snr));
        }
    }
}
