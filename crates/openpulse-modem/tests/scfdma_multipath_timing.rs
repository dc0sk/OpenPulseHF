//! SC-FDMA must decode a static two-ray channel that fits inside the cyclic prefix.
//!
//! The matched filter's argmax sits on whichever ray is instantaneously strongest — the *delayed* one
//! about half the time — and a late FFT window start pulls samples of the next symbol in. The cyclic
//! prefix only protects an **early** start, so `find_sync_offset` backs off the peak.
//!
//! Before that, SCFDMA52-16QAM could not decode `x[n] + x[n−4]` **noiselessly** (BER 1.000), and every
//! SC-FDMA rung decoded a flat 2–7 % of Watterson `good_f1` frames at *every* SNR from 8 to 32 dB. The
//! flatness was the tell: a noise-enhancement mechanism cannot survive the removal of noise.

use openpulse_channel::{
    awgn::AwgnChannel, watterson::WattersonChannel, AwgnConfig, ChannelModel, WattersonConfig,
};
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;
use scfdma_plugin::ScFdmaPlugin;

const PAYLOAD: &[u8] = b"SC-FDMA multipath timing gate payload, sixty-four bytes AAAAAAA";

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(ScFdmaPlugin::new())).unwrap();
    }
    h
}

/// Two static rays `y[n] = a0·x[n] + a1·x[n−d]` (delay well inside the 32-sample CP), then AWGN.
struct TwoRayAwgn {
    a0: f32,
    a1: f32,
    d: usize,
    awgn: AwgnChannel,
}

impl ChannelModel for TwoRayAwgn {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        let echoed: Vec<f32> = (0..input.len())
            .map(|n| {
                self.a0 * input[n]
                    + if n >= self.d {
                        self.a1 * input[n - self.d]
                    } else {
                        0.0
                    }
            })
            .collect();
        self.awgn.apply(&echoed)
    }
    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        self.awgn.generate_noise(length)
    }
}

/// Frame success over `frames` AWGN realisations of a static two-ray channel.
fn decode_rate(mode: &str, a0: f32, a1: f32, d: usize, snr_db: f32, frames: u32) -> f32 {
    let mut ok = 0u32;
    for f in 0..frames {
        let mut h = harness();
        h.tx_engine
            .transmit_with_fec_mode(PAYLOAD, mode, FecMode::SoftConcatenated, None)
            .unwrap();
        let mut ch = TwoRayAwgn {
            a0,
            a1,
            d,
            awgn: AwgnChannel::new(AwgnConfig {
                snr_db,
                seed: Some(4000 + f as u64),
            })
            .unwrap(),
        };
        h.route(&mut ch);
        if h.rx_engine
            .receive_with_fec_mode(mode, FecMode::SoftConcatenated, None)
            .map(|got| got == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    ok as f32 / frames as f32
}

/// The **delayed** ray is the stronger one, so the matched filter's argmax lands on it. This is the
/// case the cyclic prefix cannot rescue, and the one Watterson produces on half of its draws (whichever
/// ray is instantaneously stronger wins).
///
/// Every case below decoded **0.00** of frames before `find_sync_offset` started backing off the peak.
/// When the *direct* ray is stronger the argmax was already right, which is why the bug hid: a
/// symmetric static two-ray test passes either way.
///
/// Ray amplitudes are 0.5 and 1.0 — a −6 dB spectral notch, deep enough to be a real channel but not
/// the exact null that equal rays produce (an erased subcarrier is a genuine SC-FDE limit, not a
/// timing bug; see research item P7).
#[test]
fn decodes_a_stronger_delayed_ray_inside_the_cyclic_prefix() {
    const FRAMES: u32 = 20;
    for (mode, a0, a1, d) in [
        ("SCFDMA52", 0.5f32, 1.0f32, 4usize),
        ("SCFDMA52", 0.5, 1.0, 8),
        ("SCFDMA52-8PSK", 0.5, 1.0, 4),
        ("SCFDMA52-16QAM", 0.5, 1.0, 4),
        ("SCFDMA52-16QAM", 0.5, 1.0, 8),
        ("SCFDMA52-32QAM", 0.5, 1.0, 4),
    ] {
        let rate = decode_rate(mode, a0, a1, d, 24.0, FRAMES);
        assert!(
            rate >= 0.90,
            "{mode}: rays {a0}/{a1} at delay {d} @24 dB decoded only {rate:.2} of frames — \
             the FFT window is starting late, so `find_sync_offset` must lock ahead of the peak"
        );
    }
}

/// Control: with the *direct* ray stronger the argmax was always right, so this passed even with the
/// bug. Kept so a future change to `find_sync_offset` cannot fix the case above by breaking this one.
#[test]
fn a_stronger_direct_ray_still_decodes() {
    assert!(decode_rate("SCFDMA52", 1.0, 0.5, 4, 24.0, 20) >= 0.90);
    assert!(decode_rate("SCFDMA52-16QAM", 1.0, 0.5, 4, 24.0, 20) >= 0.90);
}

/// A frame whose *preamble* lands in a fade must still be acquired.
///
/// A **flat** Rayleigh fade (no delay spread) at 60 dB SNR: there is no noise and no frequency
/// selectivity, so a receiver that loses frames here has an acquisition bug and nothing else.
/// `IqMatchedFilter::search`'s unnormalised argmax deliberately prefers a high-energy window — but when
/// the preamble itself is the faded part, that hands the frame to a data-region window that merely
/// shares the pilot comb. Measured at ρ = 0.994 (energy 19.4) for the true offset against ρ = 0.657
/// (energy 83.0) at offset +4896, which won.
///
/// Before `find_sync_offset` switched to `search_normalized`, SCFDMA52-16QAM decoded 0.47 of these
/// frames at 0.5 Hz Doppler and SCFDMA52 decoded 0.75.
#[test]
fn acquires_a_frame_whose_preamble_is_faded() {
    const FRAMES: u32 = 40;

    let rate = |mode: &str, doppler: f32| -> f32 {
        let mut ok = 0u32;
        for f in 0..FRAMES {
            let mut h = harness();
            h.tx_engine
                .transmit_with_fec_mode(PAYLOAD, mode, FecMode::SoftConcatenated, None)
                .unwrap();
            let mut cfg = WattersonConfig::good_f1(Some(2000 + f as u64));
            cfg.snr_db = 60.0; // no noise
            cfg.delay_spread_ms = 0.0; // no frequency selectivity
            cfg.doppler_spread_hz = doppler;
            let Ok(mut ch) = WattersonChannel::new(cfg) else {
                continue;
            };
            h.route(&mut ch);
            if h.rx_engine
                .receive_with_fec_mode(mode, FecMode::SoftConcatenated, None)
                .map(|got| got == PAYLOAD)
                .unwrap_or(false)
            {
                ok += 1;
            }
        }
        ok as f32 / FRAMES as f32
    };

    for (mode, doppler) in [
        ("SCFDMA52", 0.5f32),
        ("SCFDMA52", 0.1),
        ("SCFDMA52-16QAM", 0.5),
        ("SCFDMA52-16QAM", 0.1),
    ] {
        let r = rate(mode, doppler);
        assert!(
            r >= 0.90,
            "{mode}: flat fade at {doppler} Hz Doppler, 60 dB SNR (no noise, no selectivity) decoded \
             only {r:.2} of frames — sync is losing the faded preamble to a higher-energy data window"
        );
    }
}
