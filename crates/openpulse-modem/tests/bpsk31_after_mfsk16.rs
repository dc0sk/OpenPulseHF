//! #1446: does a receive engine that just decoded MFSK16 fail the next BPSK31 frame?
//!
//! The issue saw 19/19 BPSK31 failures after MFSK16 in a linksim trace at `17d59b22` (before the
//! linksim stopped passing SNR on failed frames). Measured 2026-10-02 on `41eab03a`, release build,
//! Watterson moderate_f1 at 20 dB, 12 seeds: BPSK31 + Rs decoded as the second frame 12/12 after
//! MFSK16 on the same engine, 12/12 on a fresh engine, 12/12 after BPSK31. No carried state. The
//! same linksim run (`fast`, seed 7, 64 B, 60 frames) climbs SL2 → SL13 without visiting SL1.
//! Kept as a measurement: it has no failing control at this SNR, so it cannot gate.
use openpulse_audio::LoopbackBackend;
use openpulse_channel::watterson::WattersonChannel;
use openpulse_channel::{ChannelModel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::bridge_through;
use openpulse_modem::ModemEngine;

fn engine(lb: &LoopbackBackend) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .unwrap();
    e
}

fn chan(seed: u64) -> WattersonChannel {
    let mut c = WattersonConfig::moderate_f1(Some(seed)).continuous();
    c.snr_db = 20.0;
    WattersonChannel::new(c).unwrap()
}

/// Send `frames` (mode, fec) through one channel into one RX engine; return per-frame success.
fn run(seed: u64, frames: &[(&str, FecMode)], fresh_rx_each: bool) -> Vec<bool> {
    let tx_lb = LoopbackBackend::new();
    let mut tx = engine(&tx_lb);
    let mut ch = chan(seed);
    let mut rx_lb = LoopbackBackend::new();
    let mut rx = engine(&rx_lb);
    let data: Vec<u8> = (0..64u8).collect();
    frames
        .iter()
        .map(|(mode, fec)| {
            if fresh_rx_each {
                rx_lb = LoopbackBackend::new();
                rx = engine(&rx_lb);
            }
            tx.transmit_with_fec_mode(&data, mode, *fec, None).unwrap();
            bridge_through(&tx_lb, &rx_lb, &mut ch as &mut dyn ChannelModel);
            rx.receive_with_fec_mode(mode, *fec, None).ok().as_deref() == Some(&data[..])
        })
        .collect()
}

#[test]
#[ignore = "measurement (#1446); minutes of decode work — run with --release --ignored --nocapture"]
fn bpsk31_after_mfsk16_on_one_receive_engine() {
    let seq = [("MFSK16", FecMode::Rs), ("BPSK31", FecMode::Rs)];
    let ctrl = [("BPSK31", FecMode::Rs), ("BPSK31", FecMode::Rs)];
    let (mut a, mut b, mut c) = (0, 0, 0);
    let n = 12;
    for seed in 0..n {
        let same = run(seed, &seq, false);
        let fresh = run(seed, &seq, true);
        let base = run(seed, &ctrl, false);
        println!("seed {seed}: after-MFSK16 same-engine {same:?} fresh-engine {fresh:?} BPSK31-twice {base:?}");
        a += same[1] as u32;
        b += fresh[1] as u32;
        c += base[1] as u32;
    }
    println!("BPSK31 decoded as 2nd frame: same engine after MFSK16 {a}/{n}, fresh engine {b}/{n}, after BPSK31 {c}/{n}");
}
