//! Differential QPSK (DQPSK) survives HF fading where coherent QPSK dies — issue #923.
//!
//! `hpx_hf` SL6 was `QPSK250+Rs`, which decodes 0% on Watterson `moderate_f1` (1 Hz Doppler,
//! 1.0 ms delay) at *every* SNR up to 40 dB — a coherent absolutely-encoded waveform cannot hold a
//! carrier-phase reference through the fade, so a cycle slip at a fade null ruins the frame tail.
//! Ablation (issue #923) showed it is carrier tracking, not ISI or noise: removing the Doppler
//! rescues it, removing the delay spread does not. The fix is the same one that makes BPSK immune —
//! differential encoding (`-D`), where each dibit is a phase *increment* so the fade rotation cancels
//! in the symbol-to-symbol difference and a slip costs one dibit, not the tail. Differential needs
//! FEC (a per-slip dibit error must be corrected), so SL6 is `QPSK250-D+Rs`.

use openpulse_channel::{watterson::WattersonChannel, WattersonConfig};
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;

const PAYLOAD: &[u8] = b"issue 923 differential-QPSK HF-fading gate payload, 64 bytes AAA";
const FRAMES: u32 = 24;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
            .unwrap();
    }
    h
}

/// Decode rate over `moderate_f1` at `snr_db`, `FRAMES` independent realisations.
fn decode_rate_moderate_f1(mode: &str, snr_db: f32) -> f32 {
    let mut ok = 0u32;
    for f in 0..FRAMES {
        let mut h = harness();
        if h.tx_engine
            .transmit_with_fec_mode(PAYLOAD, mode, FecMode::Rs, None)
            .is_err()
        {
            continue;
        }
        let mut cfg = WattersonConfig::moderate_f1(Some(4000 + f as u64));
        cfg.snr_db = snr_db;
        let Ok(mut ch) = WattersonChannel::new(cfg) else {
            continue;
        };
        h.route(&mut ch);
        if h.rx_engine
            .receive_with_fec_mode(mode, FecMode::Rs, None)
            .map(|d| d == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    ok as f32 / FRAMES as f32
}

#[test]
fn differential_qpsk_round_trips_clean() {
    let mut h = harness();
    h.tx_engine
        .transmit_with_fec_mode(PAYLOAD, "QPSK250-D", FecMode::Rs, None)
        .expect("modulate");
    h.route_clean();
    let decoded = h
        .rx_engine
        .receive_with_fec_mode("QPSK250-D", FecMode::Rs, None)
        .expect("demodulate");
    assert_eq!(
        decoded, PAYLOAD,
        "differential QPSK must round-trip on a clean channel"
    );
}

#[test]
fn differential_qpsk_survives_moderate_f1_where_coherent_dies() {
    // 20 dB is a routine moderate-HF operating point and above SL6's ~7 dB nominal.
    let coherent = decode_rate_moderate_f1("QPSK250", 20.0);
    let differential = decode_rate_moderate_f1("QPSK250-D", 20.0);

    // Coherent QPSK250 is effectively dead on this channel — the whole point of the issue.
    assert!(
        coherent <= 0.10,
        "coherent QPSK250 should be ~dead on moderate_f1 (got {coherent:.3}); \
         if this rose, the harness or channel changed — re-check the premise"
    );
    // Differential recovers the rung to a usable rate. Measured ~0.65 at 20 dB over 40 frames;
    // gate well below that so seed variation over 24 frames does not flake, but far above coherent.
    assert!(
        differential >= 0.40,
        "differential QPSK250-D should recover moderate_f1 (got {differential:.3}, expected >= 0.40)"
    );
    assert!(
        differential >= coherent + 0.30,
        "differential must beat coherent by a wide margin on fading \
         (differential {differential:.3} vs coherent {coherent:.3})"
    );
}
