//! Does BPSK250 survive a 200 Hz brick-wall mask at all? (#1060, finding F-1060-03)
//!
//! The `f9_decode_conditioned_rho_tail` sweep decoded **0 of 180** faded, noisy trials through a
//! 1400–1600 Hz mask, and the obvious reading was "the mask is narrower than the waveform, so that
//! configuration cannot work". That reading is a claim about *physics* and it was made from a
//! measurement that also contained fade, noise and a bounded scan budget — three other explanations.
//!
//! This is the noiseless extreme the project's own playbook prescribes for exactly that situation:
//! one frame, the same mask, no fade, no noise. A receiver that cannot decode *that* has nowhere to
//! hide; a receiver that can decode it means the 0/180 measured margin erosion, not impossibility.
//!
//! Run: `cargo test -p openpulse-modem --no-default-features --test narrow_mask_decode_check`

use bpsk_plugin::BpskPlugin;
use openpulse_channel::ChannelModel;
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;

const MODE: &str = "BPSK250";
const PAYLOAD: &[u8] = b"NARROW MASK CHECK 1060";

/// Brick-wall band mask by FFT bin zeroing — the same shape the ρ probes use, so the two
/// measurements are talking about the same filter.
struct BrickWall {
    lo: f32,
    hi: f32,
}

impl ChannelModel for BrickWall {
    fn apply(&mut self, input: &[f32]) -> Vec<f32> {
        use rustfft::{num_complex::Complex, FftPlanner};
        let n = input.len().next_power_of_two();
        let mut buf: Vec<Complex<f32>> = input
            .iter()
            .map(|&v| Complex::new(v, 0.0))
            .chain(std::iter::repeat_n(Complex::new(0.0, 0.0), n - input.len()))
            .collect();
        let mut planner = FftPlanner::new();
        planner.plan_fft_forward(n).process(&mut buf);
        let bin_hz = 8_000.0 / n as f32;
        for (k, v) in buf.iter_mut().enumerate() {
            let f = if k <= n / 2 {
                k as f32 * bin_hz
            } else {
                (n - k) as f32 * bin_hz
            };
            if f < self.lo || f > self.hi {
                *v = Complex::new(0.0, 0.0);
            }
        }
        planner.plan_fft_inverse(n).process(&mut buf);
        let scale = 1.0 / n as f32;
        // Truncate back to the input length: the FFT pads to a power of two, and handing the
        // receiver a longer buffer changes the frame geometry it is being asked about. (Caught by
        // the 500 Hz control below failing, which is what that control is for.)
        buf.iter().take(input.len()).map(|c| c.re * scale).collect()
    }
    fn generate_noise(&mut self, length: usize) -> Vec<f32> {
        vec![0.0; length]
    }
}

fn decodes_through(lo: f32, hi: f32) -> bool {
    let mut h = ChannelSimHarness::new();
    for e in [&mut h.tx_engine, &mut h.rx_engine] {
        e.register_plugin(Box::new(BpskPlugin::new()))
            .expect("register");
    }
    h.tx_engine
        .transmit_with_fec_mode(PAYLOAD, MODE, FecMode::Rs, None)
        .expect("transmit");
    let mut chan = BrickWall { lo, hi };
    h.route(&mut chan);
    h.rx_engine
        .receive_with_fec_mode(MODE, FecMode::Rs, None)
        .map(|p| p == PAYLOAD)
        .unwrap_or(false)
}

/// The finding: a 200 Hz brick-wall does **not** make BPSK250 undecodable.
#[test]
fn bpsk250_still_decodes_through_a_200_hz_brick_wall_when_the_channel_is_otherwise_clean() {
    assert!(
        decodes_through(1_250.0, 1_750.0),
        "control: the 500 Hz mask must decode, or this fixture proves nothing about the 200 Hz one"
    );
    assert!(
        decodes_through(1_400.0, 1_600.0),
        "BPSK250 failed through a 200 Hz mask on a noiseless channel — if this ever fails, the \
         'cannot work at all' reading of #1060's 0/180 becomes defensible again"
    );
}
