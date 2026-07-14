//! REQ-AGC-01 acceptance: an input-amplitude sweep documenting that decode is level-invariant with the
//! receiver AGC on *or* off, and that the AGC's real value is level tracking / metering — not decode
//! rescue.
//!
//! The design review established the AGC already exists and is seam-wired; the open question this test
//! closes is *what it actually buys*. Because the soft-LLR / SNR / carrier-recovery estimators are all
//! amplitude-**ratio**-based, decode above the DCD squelch is already level-invariant without AGC
//! (`normalize_stream_rms` before the PSK loops handles the one genuine level coupling, PR #700). So:
//!   * decode succeeds across a wide amplitude range with AGC **off** and with AGC **on** (no regression);
//!   * the AGC actually runs on the receive path (tripwire) and its gain **tracks the input level** — the
//!     metering / QSB-stabilisation role that is its purpose.
//! This test is also a guard: a future absolute-level assumption creeping into the RX chain would break
//! the AGC-off invariance leg.

use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use qpsk_plugin::QpskPlugin;

const MODE: &str = "QPSK500";

fn tx_frame(payload: &[u8]) -> Vec<f32> {
    let backend = LoopbackBackend::new();
    let handle = backend.clone_shared();
    let mut tx = ModemEngine::new(Box::new(backend));
    tx.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    tx.set_center_frequency(1500.0);
    tx.transmit(payload, MODE, None).unwrap();
    handle.drain_samples()
}

fn rx() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut rx = ModemEngine::new(Box::new(backend.clone_shared()));
    rx.register_plugin(Box::new(QpskPlugin::new())).unwrap();
    rx.set_center_frequency(1500.0);
    (rx, backend)
}

fn decodes(frame: &[f32], scale: f32, agc: bool, payload: &[u8]) -> bool {
    let scaled: Vec<f32> = frame.iter().map(|s| s * scale).collect();
    let (mut engine, backend) = rx();
    if agc {
        engine.enable_agc();
    }
    backend.fill_samples(&scaled);
    let out = engine.receive(MODE, None).unwrap_or_default();
    out.len() >= payload.len() && &out[..payload.len()] == payload
}

#[test]
fn decode_is_level_invariant_with_agc_on_or_off() {
    let payload: Vec<u8> = (0u8..64).collect();
    let frame = tx_frame(&payload);
    // A wide amplitude range, all above the DCD squelch. Decode must not depend on level or on the AGC.
    for scale in [0.1f32, 0.3, 1.0, 3.0] {
        for agc in [false, true] {
            assert!(
                decodes(&frame, scale, agc, &payload),
                "{MODE} must decode at ×{scale} (agc={agc}) — decode is amplitude-ratio-based"
            );
        }
    }
}

#[test]
fn agc_runs_on_the_receive_path_and_tracks_the_input_level() {
    let payload: Vec<u8> = (0u8..64).collect();
    let frame = tx_frame(&payload);

    // Fast loop so the gain converges within one frame for the metering check.
    let gain_and_blocks = |scale: f32| -> (f32, u64) {
        let scaled: Vec<f32> = frame.iter().map(|s| s * scale).collect();
        let (mut engine, backend) = rx();
        engine.configure_agc(0.3, 0.5, 40.0);
        engine.enable_agc();
        backend.fill_samples(&scaled);
        let _ = engine.receive(MODE, None);
        (engine.agc_gain_db(), engine.agc_blocks_processed())
    };

    let (quiet_gain, quiet_blocks) = gain_and_blocks(0.1);
    let (loud_gain, loud_blocks) = gain_and_blocks(3.0);

    assert!(
        quiet_blocks > 0 && loud_blocks > 0,
        "AGC must run on the receive path (tripwire: agc_blocks_processed must increment)"
    );
    // Level tracking: a quiet capture is boosted (higher gain) and a loud one attenuated (lower gain).
    assert!(
        quiet_gain > loud_gain,
        "AGC gain must track input level: quiet {quiet_gain:.1} dB should exceed loud {loud_gain:.1} dB"
    );
}
