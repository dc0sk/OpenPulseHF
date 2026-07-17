//! BPSK's SNR estimate must carry channel information on a fade — issue #934.
//!
//! BPSK had **no** `estimate_snr_db`, so the engine fell back to the waveform-blind M2M4 moment
//! estimator. M2M4 assumes a constant-modulus envelope, which a fade destroys: on Watterson
//! `moderate_f1` it read a **flat ≈ −4 dB from 15 dB of true SNR upward** — the same number at 15 dB
//! and at 35 dB, i.e. no information at all. `hpx_hf`'s SL2–SL5 are all BPSK, so the rate controller
//! was making decisions on a constant.
//!
//! This gate is deliberately about **monotonicity, not accuracy**. The estimate is a symbol-domain
//! (post-matched-filter Es/N0) figure, so it sits above the channel SNR by the mode's processing
//! gain and saturates at the mode's residual-EVM floor — both fine for a rate decision, which only
//! needs the number to *move* with the channel. What is not fine is a constant.
//!
//! Scope note: fixing the estimate does **not** on its own free the ladder — the controller can only
//! climb when the estimate clears a ceiling, and at 31 baud a 1 Hz fade is too fast for any window to
//! separate the multiplicative channel from noise (see #934). BPSK250 is the rung tested here because
//! it is the fastest BPSK rung and the one where the estimate is recoverable.

use openpulse_channel::{
    awgn::AwgnChannel, watterson::WattersonChannel, AwgnConfig, WattersonConfig,
};
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;

const MODE: &str = "BPSK250";
const PAYLOAD: &[u8] = b"bpsk snr-on-a-fade gate payload, sixty-four bytes total AAAAAAA";

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for e in [&mut h.tx_engine, &mut h.rx_engine] {
        e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
            .unwrap();
    }
    h
}

/// Mean reported SNR at a given true channel SNR through `moderate_f1`.
fn reported_snr(true_snr_db: f32, frames: u32) -> f32 {
    let mut acc = 0.0f32;
    let mut n = 0u32;
    for f in 0..frames {
        let mut h = harness();
        if h.tx_engine
            .transmit_with_fec_mode(PAYLOAD, MODE, FecMode::Rs, None)
            .is_err()
        {
            continue;
        }
        let mut cfg = WattersonConfig::moderate_f1(Some(4400 + f as u64));
        cfg.snr_db = true_snr_db;
        let Ok(mut ch) = WattersonChannel::new(cfg) else {
            continue;
        };
        let (_, rx) = h.route_tapped(&mut ch);
        acc += h.rx_engine.rx_snr_db(MODE, &rx);
        n += 1;
    }
    assert!(n > 0, "no frames survived the harness");
    acc / n as f32
}

#[test]
fn bpsk_snr_estimate_still_carries_information_on_a_fade() {
    let low = reported_snr(5.0, 5);
    let mid = reported_snr(15.0, 5);
    let high = reported_snr(25.0, 5);

    // The defect this guards: M2M4 reported the SAME number across this whole span.
    assert!(
        high - low >= 3.0,
        "the SNR estimate must move with the channel: 5 dB → {low:.1}, 25 dB → {high:.1} \
         (spread {:.1} dB). A flat estimate makes every rate decision a decision on a constant — \
         it is what pinned the ladder in #934.",
        high - low
    );
    // Monotonic (with a little slack for fade-draw variance at 5 frames/point).
    assert!(
        mid >= low - 1.0 && high >= mid - 1.0,
        "estimate must be non-decreasing in true SNR: 5 dB → {low:.1}, 15 dB → {mid:.1}, \
         25 dB → {high:.1}"
    );
}

/// Mean reported SNR at a given true channel SNR through AWGN.
fn reported_snr_awgn(true_snr_db: f32, frames: u32) -> f32 {
    let mut acc = 0.0f32;
    let mut n = 0u32;
    for f in 0..frames {
        let mut h = harness();
        if h.tx_engine
            .transmit_with_fec_mode(PAYLOAD, MODE, FecMode::Rs, None)
            .is_err()
        {
            continue;
        }
        let Ok(mut ch) = AwgnChannel::new(AwgnConfig {
            snr_db: true_snr_db,
            seed: Some(5500 + f as u64),
        }) else {
            continue;
        };
        let (_, rx) = h.route_tapped(&mut ch);
        acc += h.rx_engine.rx_snr_db(MODE, &rx);
        n += 1;
    }
    assert!(n > 0, "no frames survived the harness");
    acc / n as f32
}

/// The estimate must be on the **channel-SNR scale the ladder's floors are written in**, not the raw
/// symbol-domain Es/N0 it is measured in.
///
/// This is the trap that makes #934 recursive: the fix for a flat estimate is measured after the
/// matched filter, so it carries the mode's processing gain — BPSK31 reads ~+17 dB and BPSK250 ~+8 dB
/// above the channel. Landed unconverted, the receiver read a 2 dB AWGN channel as good enough for
/// SL5 and over-recommended rungs it could not carry (`awgn_low_snr_does_not_overclimb` caught it).
/// Fixing one scale error by introducing another is not a fix.
#[test]
fn bpsk_snr_awgn_scale_matches_channel_snr() {
    for true_snr in [5.0f32, 10.0, 15.0] {
        let got = reported_snr_awgn(true_snr, 5);
        assert!(
            (got - true_snr).abs() <= 3.0,
            "on AWGN the estimate must read the channel SNR it is compared against: true {true_snr} \
             dB → {got:.1} dB. An uncorrected symbol-domain Es/N0 reads ~+8 dB high at 250 baud and \
             ~+17 dB at 31 baud, and the ladder over-climbs on it."
        );
    }
}
