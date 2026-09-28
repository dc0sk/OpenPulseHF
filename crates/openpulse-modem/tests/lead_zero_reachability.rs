//! #1438 PR2: a frame that starts at the very start of the capture decodes through the engine.
//!
//! BPSK's timing objective peaks a quarter symbol before the symbol boundary, which is also where
//! the stronger (uncancelled) decision arm is best sampled. The shipped search scanned offsets
//! `[0, n)` only, so a frame starting within a quarter symbol of the slice could not reach that
//! peak: the lock was clamped toward the boundary, and on AWGN near the coded cliff nothing decoded.
//! PR2 searches `[−n/2, n)` and decodes at both locks, letting the FEC adjudicate (P6).
//!
//! Measured before implementing, plugin-level, BPSK250 + Rs, 200 B, AWGN −4 dB, frame at the slice
//! start: the restricted search decoded 0/96, the two-lock decode 91/96. This gate runs the real
//! engine entry (`receive_with_fec_mode`) on a buffer that IS the frame — the lead-0 case itself.

use openpulse_audio::LoopbackBackend;
use openpulse_channel::awgn::AwgnChannel;
use openpulse_channel::{AwgnConfig, ChannelModel};
use openpulse_core::fec::FecMode;
use openpulse_modem::engine::ModemEngine;

const MODE: &str = "BPSK250";
const SEEDS: u64 = 16;

fn engine() -> (ModemEngine, LoopbackBackend) {
    let b = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(b.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register");
    (e, b)
}

#[test]
fn a_frame_at_the_start_of_the_capture_decodes_near_the_coded_cliff() {
    let payload: Vec<u8> = (0..200u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    let tx = {
        let (mut e, b) = engine();
        e.transmit_with_fec_mode(&payload, MODE, FecMode::Rs, None)
            .expect("tx");
        b.drain_samples()
    };
    let mut decoded = 0u64;
    for seed in 0..SEEDS {
        let rx = AwgnChannel::new(AwgnConfig::new(-4.0, Some(9_100 + seed)))
            .expect("awgn")
            .apply(&tx);
        let (mut e, b) = engine();
        b.push_frame(&rx);
        if e.receive_with_fec_mode(MODE, FecMode::Rs, None)
            .map(|d| d == payload)
            .unwrap_or(false)
        {
            decoded += 1;
        }
    }
    println!("lead 0, AWGN −4 dB, BPSK250 + Rs: {decoded}/{SEEDS} decoded");
    assert!(
        decoded >= 12,
        "a frame at the start of the capture decoded {decoded}/{SEEDS} at −4 dB: the widened timing \
         search is not reaching the early peak (the restricted search alone decoded 0/96 here)"
    );
}
