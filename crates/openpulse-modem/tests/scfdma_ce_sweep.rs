//! SC-FDMA channel-estimation sweep — the before/after harness for receiver-side CE work
//! (research doc `docs/dev/research/scfdma-improvements.md`, items P2/P3).
//!
//! **This is a measurement harness, not an acceptance gate.** Every test here is `#[ignore]`d and
//! asserts nothing — it prints curves for a human to compare across a change. The acceptance table in
//! `CLAUDE.md` once cited it as the gate for "SC-FDMA channel estimator vs. selective channels",
//! which meant that requirement was enforced by nobody; the gate is `scfdma_multipath_timing`, which
//! asserts and runs by default. Don't re-link this file as a gate without giving it assertions.
//!
//! Prints a decode-rate-vs-SNR curve for every SC-FDMA rung of `hpx_hf`, over AWGN and over
//! Watterson `good_f1` fading. RX-only changes (CPE removal, non-causal CE smoothing, noise-variance
//! smoothing) must not move the AWGN curve down and should move the fading curve left.
//!
//! ```text
//! cargo test -p openpulse-modem --no-default-features --test scfdma_ce_sweep -- --ignored --nocapture
//! ```

use openpulse_channel::{
    awgn::AwgnChannel, watterson::WattersonChannel, AwgnConfig, WattersonConfig,
};
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;
use scfdma_plugin::ScFdmaPlugin;

/// The SC-FDMA rungs of `hpx_hf`, in ladder order, with the FEC each rung is assigned.
const RUNGS: &[&str] = &[
    "SCFDMA26-32QAM",
    "SCFDMA52-8PSK",
    "SCFDMA52-16QAM",
    "SCFDMA52-32QAM",
    "SCFDMA52-64QAM-P4",
    "SCFDMA52-64QAM",
];

const FEC: FecMode = FecMode::SoftConcatenated;
const FRAMES: u32 = 60;
const PAYLOAD: &[u8] = b"SC-FDMA CE sweep payload, sixty-four bytes in total AAAAAAAAAAAAA";

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(ScFdmaPlugin::new())).unwrap();
    }
    h
}

fn decode_rate_awgn(mode: &str, snr_db: f32) -> f32 {
    let mut ok = 0u32;
    for f in 0..FRAMES {
        let mut h = harness();
        if h.tx_engine
            .transmit_with_fec_mode(PAYLOAD, mode, FEC, None)
            .is_err()
        {
            continue;
        }
        let Ok(mut ch) = AwgnChannel::new(AwgnConfig {
            snr_db,
            seed: Some(1000 + f as u64),
        }) else {
            continue;
        };
        h.route(&mut ch);
        if h.rx_engine
            .receive_with_fec_mode(mode, FEC, None)
            .map(|d| d == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    ok as f32 / FRAMES as f32
}

fn decode_rate_good_f1(mode: &str, snr_db: f32) -> f32 {
    let mut ok = 0u32;
    for f in 0..FRAMES {
        let mut h = harness();
        if h.tx_engine
            .transmit_with_fec_mode(PAYLOAD, mode, FEC, None)
            .is_err()
        {
            continue;
        }
        let mut cfg = WattersonConfig::good_f1(Some(2000 + f as u64));
        cfg.snr_db = snr_db;
        let Ok(mut ch) = WattersonChannel::new(cfg) else {
            continue;
        };
        h.route(&mut ch);
        if h.rx_engine
            .receive_with_fec_mode(mode, FEC, None)
            .map(|d| d == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    ok as f32 / FRAMES as f32
}

fn sweep(label: &str, snrs: &[f32], rate: fn(&str, f32) -> f32) {
    println!("\n=== SC-FDMA CE sweep — {label} ({FRAMES} frames/point, FEC={FEC:?}) ===");
    print!("{:<20}", "mode \\ SNR dB");
    for s in snrs {
        print!("{s:>7.0}");
    }
    println!();
    for mode in RUNGS {
        print!("{mode:<20}");
        for &s in snrs {
            print!("{:>7.2}", rate(mode, s));
        }
        println!();
    }
}

#[test]
#[ignore = "measurement sweep; run manually with --ignored --nocapture"]
fn scfdma_ce_sweep_awgn() {
    sweep(
        "AWGN",
        &[4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0, 20.0],
        decode_rate_awgn,
    );
}

#[test]
#[ignore = "measurement sweep; run manually with --ignored --nocapture"]
fn scfdma_ce_sweep_watterson_good_f1() {
    sweep(
        "Watterson good_f1",
        &[8.0, 12.0, 16.0, 20.0, 24.0, 28.0, 32.0],
        decode_rate_good_f1,
    );
}
