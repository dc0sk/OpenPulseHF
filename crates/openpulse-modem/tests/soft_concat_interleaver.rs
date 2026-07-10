//! The SoftConcatenated wire carries a byte interleaver between the outer RS and inner conv, so a
//! deep-fade *burst* (a contiguous span of destroyed symbols) is spread across RS blocks instead of
//! clustering into one — which is what lets the outer RS correct it. Without the interleaver a burst
//! that fits inside one RS block's error budget when spread, instead overwhelms a couple of blocks and
//! the frame fails (measured burst-fade FER 0.98 → 0.20 @4 dB).
//!
//! This gate feeds SoftConcatenated a contiguous *phase-inverted* burst (a real deep fade rotates the
//! carrier → confident-WRONG symbols that soft-Viterbi trusts and propagates — unlike a pure attenuation
//! burst, which just yields low-confidence LLRs the soft decoder recovers). It decodes only because the
//! interleaver spreads the resulting byte-error run across both RS blocks; deleting the interleaver (or
//! applying it on one end only) collapses this to 0.00 (verified by ablation).

use openpulse_channel::{awgn::AwgnChannel, AwgnConfig, ChannelModel};
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;
use qpsk_plugin::QpskPlugin;

// A multi-block payload: >223 bytes framed spans two RS(255,223) blocks. The RS↔conv byte interleaver
// helps precisely here — it spreads a burst's byte errors across *both* blocks so each stays under RS's
// t=16, where clustered the burst overwhelms one block and the frame fails. A single-block payload sees
// no benefit (RS corrects 16 errors wherever they sit), which is the subtlety this gate encodes.
fn payload() -> Vec<u8> {
    (0..240u32)
        .map(|i| (i.wrapping_mul(97) & 0xff) as u8)
        .collect()
}
// Single-carrier: a contiguous time-burst maps directly to a run of destroyed symbols — the classic
// burst-error case an outer-RS/inner-conv interleaver is for (OFDM would smear a time-burst across a
// whole symbol's subcarriers, a different failure).
const MODE: &str = "QPSK500";

/// A contiguous deep-fade burst: multiply a `burst_frac` span of the frame by `depth` (−1.0 = a 180°
/// carrier phase inversion, the confident-wrong case), then AWGN.
struct BurstFade {
    depth: f32,
    burst_frac: f32,
    awgn: AwgnChannel,
}

impl ChannelModel for BurstFade {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        let n = input.len();
        // Place the burst in the payload region (past the preamble), a contiguous span.
        let start = n / 4;
        let len = (n as f32 * self.burst_frac) as usize;
        let faded: Vec<f32> = input
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                if i >= start && i < start + len {
                    s * self.depth
                } else {
                    s
                }
            })
            .collect();
        self.awgn.apply(&faded)
    }
    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        self.awgn.generate_noise(length)
    }
}

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for e in [&mut h.tx_engine, &mut h.rx_engine] {
        e.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    }
    h
}

#[test]
fn soft_concat_decodes_a_contiguous_burst_fade() {
    let trials = 20u32;
    let mut ok = 0u32;
    for t in 0..trials {
        let mut h = harness();
        h.tx_engine
            .transmit_with_fec_mode(&payload(), MODE, FecMode::SoftConcatenated, None)
            .unwrap();
        let mut ch = BurstFade {
            depth: -1.0,
            burst_frac: 0.05,
            awgn: AwgnChannel::new(AwgnConfig::new(22.0, Some(3000 + t as u64))).unwrap(),
        };
        h.route(&mut ch);
        if h.rx_engine
            .receive_with_fec_mode(MODE, FecMode::SoftConcatenated, None)
            .map(|d| d == payload())
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    let rate = ok as f32 / trials as f32;
    assert!(
        rate >= 0.80,
        "SoftConcatenated decoded only {rate:.2} of frames through a contiguous deep-fade burst — the \
         RS↔conv byte interleaver must spread the burst across RS blocks (it clusters and fails without)"
    );
}

/// Control: the interleaver is free on a clean channel — no burst, must still decode every frame.
#[test]
fn soft_concat_interleaver_is_free_on_a_clean_channel() {
    let mut ok = 0u32;
    for t in 0..10u32 {
        let mut h = harness();
        h.tx_engine
            .transmit_with_fec_mode(&payload(), MODE, FecMode::SoftConcatenated, None)
            .unwrap();
        let mut ch = AwgnChannel::new(AwgnConfig::new(30.0, Some(t as u64))).unwrap();
        h.route(&mut ch);
        if h.rx_engine
            .receive_with_fec_mode(MODE, FecMode::SoftConcatenated, None)
            .map(|d| d == payload())
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    assert_eq!(
        ok, 10,
        "the interleaver must not cost anything on a clean channel"
    );
}
