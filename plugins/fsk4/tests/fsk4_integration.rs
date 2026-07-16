//! FSK4 plugin integration tests: loopback correctness and the ACK channel's real noise floor.

use fsk4_plugin::Fsk4Plugin;
use openpulse_channel::{awgn::AwgnChannel, AwgnConfig, ChannelModel};
use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

fn ack_config() -> ModulationConfig {
    ModulationConfig {
        mode: "FSK4-ACK".to_string(),
        sample_rate: 8000,
        center_frequency: 1050.0,
        ..ModulationConfig::default()
    }
}

/// Measured `AckFrame` round-trip success through AWGN over `seeds` independent noise draws.
///
/// Frames, not bytes: the ACK is only useful if `AckFrame::decode` accepts it (CRC-8 + session
/// hash), which is what the rate adapter actually consumes.
fn ack_success_rate(snr_db: f32, seeds: u32) -> f32 {
    let plugin = Fsk4Plugin::new();
    let cfg = ack_config();
    let mut ok = 0u32;
    for seed in 0..seeds {
        let frame = AckFrame::new(AckType::AckOk, "session-test");
        let payload = frame.encode();
        let samples = plugin.modulate(&payload, &cfg).expect("modulate");
        let mut channel =
            AwgnChannel::new(AwgnConfig::new(snr_db, Some(9_000 + seed as u64))).expect("awgn");
        let noisy = channel.apply(&samples);
        let Ok(bytes) = plugin.demodulate(&noisy, &cfg) else {
            continue;
        };
        if bytes.len() < 5 {
            continue;
        }
        let Ok(chunk) = bytes[..5].try_into() else {
            continue;
        };
        if AckFrame::decode(chunk).map(|d| d == frame).unwrap_or(false) {
            ok += 1;
        }
    }
    ok as f32 / seeds as f32
}

/// FSK4-ACK loopback over a clean channel: recovered bytes must match the transmitted payload.
#[test]
fn fsk4_ack_clean_loopback() {
    let plugin = Fsk4Plugin::new();
    let cfg = ack_config();
    let payload = [0x01u8, 0x02, 0x03, 0x04, 0x05];
    let samples = plugin.modulate(&payload, &cfg).unwrap();
    let recovered = plugin.demodulate(&samples, &cfg).unwrap();
    assert_eq!(recovered, payload);
}

/// A real `AckFrame` must survive AWGN at the ACK channel's operating point.
///
/// This is the gate the plugin previously lacked: the only noise test asserted the ACK *breaks* at
/// -20 dB, which a demodulator returning constant garbage would also pass. What matters for the rate
/// ladder is the opposite — that ACKs get through, since they are the feedback path the adapter
/// steers on.
///
/// Measured waterfall (real `AckFrame` round-trip, 200 seeds/point), which also fixes the record:
/// the old test claimed "~19 dB processing gain, so only SNR below about -16 dB reliably causes
/// errors". That is wrong by ~14 dB — the frame is already dead at -8 dB.
///
/// ```text
///  SNR (dB)  -8     -6     -4     -2      0      2      4     6+
///  frame ok   0.005  0.035  0.195  0.545  0.875  0.990  1.000  1.000
/// ```
///
/// The 5-byte ACK is 20 symbols, so it needs a very low per-symbol error rate to arrive intact —
/// which is why the frame floor sits well above the point where individual symbols start to slip.
#[test]
fn fsk4_ack_decodes_through_noise_at_its_operating_point() {
    let rate = ack_success_rate(4.0, 200);
    assert!(
        rate >= 0.98,
        "FSK4-ACK delivered {rate:.3} of frames at +4 dB SNR — the ACK channel must be reliable at \
         its operating point or the rate adapter loses the feedback it steers on"
    );
}

/// The ACK channel degrades below its floor rather than silently accepting corrupt frames.
///
/// Pinned at -8 dB, not the old -20 dB: -20 dB sat ~18 dB below the real floor, so the assertion was
/// trivially true and would have held even if the plugin were 16 dB better than it is. -8 dB is just
/// under the measured knee, so this fails if the floor moves much in either direction — paired with
/// the operating-point gate above, the two bracket the waterfall.
#[test]
fn fsk4_ack_degrades_below_its_floor() {
    let rate = ack_success_rate(-8.0, 200);
    assert!(
        rate <= 0.10,
        "FSK4-ACK delivered {rate:.3} of frames at -8 dB SNR — expected the channel to be well past \
         its floor here; if this improved, re-measure the waterfall and move the gates"
    );
}
