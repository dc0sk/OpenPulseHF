//! The swept-applied-correction check (DSP playbook, rule 1) for the OFDM receiver: a frame shifted by
//! Δ decodes when the receiver is told Δ (`center_frequency = 1500 + Δ`), at every Δ and every mode,
//! hard and soft. This isolates the mixing from the estimator (`docs/dev/design/ofdm-carrier-offset.md`).

use ofdm_plugin::OfdmPlugin;
use openpulse_channel::cfo::{CfoChannel, CfoConfig};
use openpulse_channel::ChannelModel;
use openpulse_core::plugin::{ModulationConfig, ModulationPlugin};

const MODES: [&str; 5] = [
    "OFDM52",
    "OFDM52-8PSK",
    "OFDM52-16QAM",
    "OFDM52-32QAM",
    "OFDM52-64QAM",
];
const OFFSETS: [f32; 10] = [-40.0, -20.0, -10.0, -5.0, -2.5, 2.5, 5.0, 10.0, 20.0, 40.0];

fn cfg(mode: &str, center: f32) -> ModulationConfig {
    ModulationConfig {
        mode: mode.into(),
        center_frequency: center,
        ..ModulationConfig::default()
    }
}

/// The frame with quiet around it, shifted by `hz` as a rig mismatch shifts it, over a noise floor of
/// 0.002 RMS. The floor is needed: on exact digital silence the shift's round trip leaves a residue
/// near 1e-6 that the timing search's self-normalised metric can lock to; a receiver never sees that.
fn received(plugin: &OfdmPlugin, mode: &str, payload: &[u8], hz: f32) -> Vec<f32> {
    let mut audio = vec![0.0f32; 2000];
    audio.extend(plugin.modulate(payload, &cfg(mode, 1500.0)).unwrap());
    audio.extend(vec![0.0f32; 2000]);
    let mut rx = CfoChannel::new(CfoConfig::new(hz, 8000.0))
        .unwrap()
        .apply(&audio);
    let mut st = 0x9e37_79b9_7f4a_7c15_u64;
    for v in rx.iter_mut() {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        *v += ((st >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.002 * 3.46;
    }
    rx
}

#[test]
fn an_offset_frame_decodes_when_the_receiver_is_told_the_offset() {
    let plugin = OfdmPlugin::new();
    let payload = b"an OFDM frame received through a rig mismatch, every mode";
    for mode in MODES {
        for hz in OFFSETS {
            let rx = received(&plugin, mode, payload, hz);
            let hard = plugin.demodulate(&rx, &cfg(mode, 1500.0 + hz));
            assert!(
                matches!(&hard, Ok(b) if b.starts_with(payload)),
                "{mode} at {hz:+} Hz, centre told: hard demod {:?}",
                hard.as_ref().map(|b| b.len())
            );
            let soft = plugin
                .demodulate_soft(&rx, &cfg(mode, 1500.0 + hz))
                .unwrap();
            let bits: Vec<u8> = soft
                .chunks(8)
                .take(payload.len())
                // Positive LLR = bit 0, LSB first, as the modulator packs.
                .map(|c| {
                    c.iter()
                        .enumerate()
                        .fold(0u8, |a, (i, &l)| a | (u8::from(l < 0.0) << i))
                })
                .collect();
            assert_eq!(
                bits, payload,
                "{mode} at {hz:+} Hz, centre told: soft demod"
            );
        }
    }
}

/// The receiver is NOT told the offset (centre 1500 Hz): the plugin acquires it. Every mode decodes,
/// hard and soft, across ±40 Hz — 2 m rigs untrimmed were 54–64 Hz apart, and the engine's BPSK
/// settle normally moves the centre to within a few Hz first.
#[test]
fn an_offset_frame_decodes_when_the_receiver_is_not_told() {
    let plugin = OfdmPlugin::new();
    let payload = b"an OFDM frame received through a rig mismatch, every mode";
    for mode in MODES {
        for hz in OFFSETS {
            let rx = received(&plugin, mode, payload, hz);
            let hard = plugin.demodulate(&rx, &cfg(mode, 1500.0));
            assert!(
                matches!(&hard, Ok(b) if b.starts_with(payload)),
                "{mode} at {hz:+} Hz, not told: hard demod {:?}",
                hard.as_ref().map(|b| b.len())
            );
            let soft = plugin.demodulate_soft(&rx, &cfg(mode, 1500.0)).unwrap();
            let bits: Vec<u8> = soft
                .chunks(8)
                .take(payload.len())
                .map(|c| {
                    c.iter()
                        .enumerate()
                        .fold(0u8, |a, (i, &l)| a | (u8::from(l < 0.0) << i))
                })
                .collect();
            assert_eq!(bits, payload, "{mode} at {hz:+} Hz, not told: soft demod");
        }
    }
}

/// The impairment is real: the raw demodulator (no mixing, no acquisition) fails 64QAM at 5 Hz.
/// Without this, the tests above would pass on a channel that moved nothing.
#[test]
fn the_raw_demodulator_fails_64qam_at_5_hz() {
    let plugin = OfdmPlugin::new();
    let payload = b"an OFDM frame received through a rig mismatch, every mode";
    let rx = received(&plugin, "OFDM52-64QAM", payload, 5.0);
    let raw = ofdm_plugin::demodulate::ofdm_demodulate(&rx, "OFDM52-64QAM");
    assert!(
        !matches!(&raw, Ok(b) if b.starts_with(payload)),
        "64QAM decoded 5 Hz off with no correction: the channel did not shift it"
    );
}
