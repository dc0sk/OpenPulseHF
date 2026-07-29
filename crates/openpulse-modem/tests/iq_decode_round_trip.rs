//! An I/Q-transmitted frame must decode on a receiver of the same build.
//!
//! **The defect this pins** (archetype scan 2026-07-29, finding 8). `stage_modulate_payload` whitens
//! the wire immediately before modulation, under a comment reading *"this is the single TX seam, so
//! every caller is covered by construction"*. That was not true: `transmit_iq` reaches
//! `plugin.modulate_iq()` on its own and applied no transform, while **every** receive path
//! un-whitens unconditionally. An I/Q-transmitted frame was therefore XORed with a keystream it never
//! carried and decoded to `invalid magic` — the same signature that made #1021 undiagnosable.
//!
//! **Why the suite could not have caught it.** `tests/iq_output.rs` asserts sample counts, Q-channel
//! RMS, the attenuation ratio and the regulatory log — everything *about* the samples, and nothing
//! that requires the bytes to survive a round trip. There was no decode anywhere on the I/Q path.
//!
//! This test closes that by upconverting the baseband I/Q back to the real passband the receiver
//! expects (`i·cos(ωt) − q·sin(ωt)`) and decoding it through the normal receive path — which is what
//! an external upconverter/SDR does with these samples in the first place.

use std::f32::consts::PI;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use qpsk_plugin::QpskPlugin;

/// The engine's default TX/RX centre frequency.
const CENTER_HZ: f32 = 1500.0;
const SAMPLE_RATE: f32 = 8000.0;

fn reg(e: &mut ModemEngine) {
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    e.register_plugin(Box::new(QpskPlugin::new())).unwrap();
}

/// Upconvert baseband I/Q to the real passband signal a receiver sees.
fn upconvert(iq: &[(f32, f32)]) -> Vec<f32> {
    let omega = 2.0 * PI * CENTER_HZ / SAMPLE_RATE;
    iq.iter()
        .enumerate()
        .map(|(k, &(i, q))| {
            let t = omega * k as f32;
            i * t.cos() - q * t.sin()
        })
        .collect()
}

/// Transmit `payload` over the I/Q path, upconvert, and decode it back through the audio receive.
fn iq_round_trip(payload: &[u8], mode: &str) -> Result<Vec<u8>, String> {
    let tx_lb = LoopbackBackend::new();
    let mut tx = ModemEngine::new(Box::new(tx_lb.clone_shared()));
    reg(&mut tx);
    tx.transmit_iq(payload, mode, None)
        .map_err(|e| format!("transmit_iq: {e}"))?;

    let iq = tx_lb.drain_iq_samples();
    assert!(
        !iq.is_empty(),
        "no I/Q samples were produced — the test would prove nothing"
    );
    let passband = upconvert(&iq);

    let rx_lb = LoopbackBackend::new();
    let mut rx = ModemEngine::new(Box::new(rx_lb.clone_shared()));
    reg(&mut rx);
    rx_lb.fill_samples(&passband);
    rx.receive(mode, None).map_err(|e| format!("{e}"))
}

/// THE GATE: I/Q transmit → upconvert → audio receive must return the payload.
///
/// Before the fix this failed with `invalid magic`, because the I/Q path skipped the whitening the
/// receive path unconditionally undoes.
#[test]
fn an_iq_transmitted_frame_decodes_on_the_audio_receive_path() {
    let payload = b"IQ path must round-trip".to_vec();
    let got = iq_round_trip(&payload, "BPSK100")
        .expect("an I/Q-transmitted frame must decode on a receiver of the same build");
    assert_eq!(got, payload, "decoded payload differs from what was sent");
}

/// A second mode with its own native `modulate_iq` override, so the fix is proven at the seam rather
/// than for one plugin.
#[test]
fn an_iq_transmitted_qpsk_frame_decodes_too() {
    let payload = b"QPSK over IQ".to_vec();
    let got = iq_round_trip(&payload, "QPSK250")
        .expect("a QPSK I/Q-transmitted frame must decode on a receiver of the same build");
    assert_eq!(got, payload);
}

/// Control: the ordinary audio transmit path round-trips through the same upconvert-free route.
///
/// If this ever fails, the I/Q assertions above are failing for a reason that has nothing to do with
/// the I/Q seam, and the diagnosis should start here instead.
#[test]
fn the_audio_path_control_round_trips() {
    let tx_lb = LoopbackBackend::new();
    let mut tx = ModemEngine::new(Box::new(tx_lb.clone_shared()));
    reg(&mut tx);
    let payload = b"audio control".to_vec();
    tx.transmit(&payload, "BPSK100", None).expect("transmit");

    let rx_lb = LoopbackBackend::new();
    let mut rx = ModemEngine::new(Box::new(rx_lb.clone_shared()));
    reg(&mut rx);
    rx_lb.fill_samples(&tx_lb.drain_samples());
    assert_eq!(rx.receive("BPSK100", None).expect("receive"), payload);
}

/// Anti-vacuity: the I/Q path must actually be exercised — a `transmit_iq` that silently produced
/// nothing, or an upconvert that returned silence, would otherwise leave the gates above passing for
/// the wrong reason.
#[test]
fn the_iq_fixture_carries_real_signal() {
    let tx_lb = LoopbackBackend::new();
    let mut tx = ModemEngine::new(Box::new(tx_lb.clone_shared()));
    reg(&mut tx);
    tx.transmit_iq(b"fixture check", "BPSK100", None)
        .expect("transmit_iq");
    let iq = tx_lb.drain_iq_samples();
    assert!(
        iq.len() > 1000,
        "I/Q burst is implausibly short: {}",
        iq.len()
    );

    let passband = upconvert(&iq);
    let mean_sq = passband.iter().map(|s| s * s).sum::<f32>() / passband.len() as f32;
    assert!(
        mean_sq > 1e-6,
        "upconverted passband is effectively silent (mean_sq {mean_sq:e})"
    );
}
