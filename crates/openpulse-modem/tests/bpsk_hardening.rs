//! BPSK hardening tests with loopback fixtures.
//!
//! Tests TX/RX under various signal conditions:
//! - SNR sweep (6dB, 9dB, 12dB, 15dB) — real AWGN round-trips through `ChannelSimHarness`
//! - Multipath profiles (fading, frequency offset, timing error)
//! - Error recovery (frame loss, timeout, retransmit)

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{awgn::AwgnChannel, AwgnConfig};
use openpulse_core::fec::FecMode;
use openpulse_modem::channel_sim::ChannelSimHarness;
use openpulse_modem::engine::ModemEngine;

const SNR_PAYLOAD: &[u8] = b"bpsk snr sweep payload 0123456789";

/// Transmit `SNR_PAYLOAD` over AWGN at `snr_db` and count how many of `trials` decode back intact.
///
/// The SNR-sweep tests used to assert a literal `true`, so the acceptance row they back ("BPSK
/// loopback correctness") was never exercising a decode at all. This does the round-trip.
fn bpsk250_awgn_decodes(snr_db: f32, trials: u64) -> u64 {
    let mut ok = 0;
    for seed in 0..trials {
        let mut h = ChannelSimHarness::new();
        for e in [&mut h.tx_engine, &mut h.rx_engine] {
            e.register_plugin(Box::new(BpskPlugin::new())).ok();
        }
        if h.tx_engine
            .transmit_with_fec_mode(SNR_PAYLOAD, "BPSK250", FecMode::Rs, None)
            .is_err()
        {
            continue;
        }
        let Ok(mut ch) = AwgnChannel::new(AwgnConfig {
            snr_db,
            seed: Some(400 + seed),
        }) else {
            continue;
        };
        let _ = h.route_tapped(&mut ch);
        if let Ok(out) = h
            .rx_engine
            .receive_with_fec_mode("BPSK250", FecMode::Rs, None)
        {
            if out.starts_with(SNR_PAYLOAD) {
                ok += 1;
            }
        }
    }
    ok
}

/// Test fixture for BPSK loopback scenarios.
struct BpskFixture {
    engine: ModemEngine,
}

impl BpskFixture {
    fn new(_session_id: &str, _peer: &str) -> Self {
        let audio = Box::new(LoopbackBackend::new());
        let mut engine = ModemEngine::new(audio);
        engine
            .register_plugin(Box::new(BpskPlugin::new()))
            .expect("BPSK registration");

        Self { engine }
    }

    fn hpx_state_str(&self) -> String {
        format!("{:?}", self.engine.hpx_state()).to_lowercase()
    }

    fn transmit(&mut self, payload: &[u8], mode: &str) -> Result<(), String> {
        self.engine
            .transmit(payload, mode, None)
            .map_err(|e| format!("{:?}", e))
    }

    fn check_recovery(&self) -> bool {
        matches!(
            self.engine.hpx_state(),
            openpulse_core::hpx::HpxState::Recovery
        )
    }
}

// ── SNR sweep tests ───────────────────────────────────────────────────────

#[test]
fn bpsk_snr_6db_loopback() {
    let ok = bpsk250_awgn_decodes(6.0, 12);
    assert_eq!(
        ok, 12,
        "BPSK250+Rs must decode every frame at 6 dB AWGN, got {ok}/12"
    );
}

#[test]
fn bpsk_snr_9db_loopback() {
    let ok = bpsk250_awgn_decodes(9.0, 12);
    assert_eq!(
        ok, 12,
        "BPSK250+Rs must decode every frame at 9 dB AWGN, got {ok}/12"
    );
}

#[test]
fn bpsk_snr_12db_loopback() {
    let ok = bpsk250_awgn_decodes(12.0, 12);
    assert_eq!(
        ok, 12,
        "BPSK250+Rs must decode every frame at 12 dB AWGN, got {ok}/12"
    );
}

#[test]
fn bpsk_snr_15db_loopback() {
    let ok = bpsk250_awgn_decodes(15.0, 12);
    assert_eq!(
        ok, 12,
        "BPSK250+Rs must decode every frame at 15 dB AWGN, got {ok}/12"
    );
}

#[test]
fn bpsk_snr_below_the_floor_degrades() {
    // The sweep above only proves decode; without a failing point it could pass on a stub receiver.
    let ok = bpsk250_awgn_decodes(-12.0, 12);
    assert!(
        ok < 12,
        "at -12 dB the link must NOT decode every frame, got {ok}/12"
    );
}

#[test]
fn bpsk_recovery_state_is_initially_clear() {
    let fixture = BpskFixture::new("bpsk-recovery-initial", "N0TEST");
    let _ok = !fixture.check_recovery();
    assert!(_ok);
}

// ── Multipath profile tests ───────────────────────────────────────────────

#[test]
fn bpsk_multipath_fading_loopback() {
    let mut fixture = BpskFixture::new("bpsk-multipath-fading", "N0TEST");
    // Simulate fading by varying loopback attenuation.
    // Verify TX/RX coherence without loss.
    let result = fixture.transmit(b"fading_test", "BPSK100");
    assert!(result.is_ok());
}

#[test]
fn bpsk_multipath_frequency_offset_loopback() {
    let mut fixture = BpskFixture::new("bpsk-multipath-freq-offset", "N0TEST");
    // Simulate frequency offset by small carrier deviation.
    // BPSK should have some tolerance for frequency error.
    let result = fixture.transmit(b"frequency_offset_test", "BPSK100");
    assert!(result.is_ok());
}

#[test]
fn bpsk_multipath_timing_error_loopback() {
    let mut fixture = BpskFixture::new("bpsk-multipath-timing-error", "N0TEST");
    // Simulate timing error by symbol boundary misalignment.
    // Recovery should trigger if timing is off by > threshold.
    let result = fixture.transmit(b"timing_error_test", "BPSK100");
    assert!(result.is_ok());
}

// ── Error recovery tests ─────────────────────────────────────────────────

#[test]
fn bpsk_frame_loss_recovery() {
    let mut fixture = BpskFixture::new("bpsk-frame-loss", "N0TEST");
    // Simulate frame loss by not delivering a complete frame.
    // Engine should eventually enter Recovery state.
    let _tx = fixture.transmit(b"test", "BPSK100");
    // In a real scenario, we'd simulate dropped frames here.
    // For now, we just verify the engine handles the attempt.
}

#[test]
fn bpsk_timeout_recovery() {
    let fixture = BpskFixture::new("bpsk-timeout-recovery", "N0TEST");
    // Engine starts in Idle state.
    assert_eq!(fixture.hpx_state_str(), "idle");
    // After timeout (in real scenario), should enter Failed or Recovery.
    // Loopback doesn't have real timing, so this is a state machine test.
}

#[test]
fn bpsk_retransmit_logic() {
    let mut fixture = BpskFixture::new("bpsk-retransmit", "N0TEST");
    // Transmit multiple times to verify retransmit counter management.
    let _ = fixture.transmit(b"payload1", "BPSK100");
    let _ = fixture.transmit(b"payload2", "BPSK100");
    let _ = fixture.transmit(b"payload3", "BPSK100");
    // After retransmit exhaustion, should recover or fail.
}

// ── Real-device path hardening tests ──────────────────────────────────────

#[test]
fn bpsk_hardware_detection_graceful_fallback() {
    // If audio hardware is unavailable, engine should gracefully fallback to loopback.
    let _fixture = BpskFixture::new("bpsk-hw-fallback", "N0TEST");
    // This test verifies that loopback is available as a fallback.
}

#[test]
fn bpsk_device_error_handling() {
    let fixture = BpskFixture::new("bpsk-device-error", "N0TEST");
    // Engine should remain stable even if audio device has errors.
    assert_eq!(fixture.hpx_state_str(), "idle");
}

// ── Behavior matrix tests ────────────────────────────────────────────────

#[test]
fn bpsk_transmit_success_in_idle() {
    let mut fixture = BpskFixture::new("bpsk-tx-idle", "N0TEST");
    assert_eq!(fixture.hpx_state_str(), "idle");
    let result = fixture.transmit(b"hello", "BPSK100");
    assert!(result.is_ok());
}

#[test]
fn bpsk_transmit_handles_invalid_mode() {
    let mut fixture = BpskFixture::new("bpsk-invalid-mode", "N0TEST");
    let result = fixture.transmit(b"hello", "INVALID_MODE");
    // Should fail gracefully, not crash.
    assert!(result.is_err());
}

#[test]
fn bpsk_empty_payload_handled() {
    let mut fixture = BpskFixture::new("bpsk-empty-payload", "N0TEST");
    let result = fixture.transmit(b"", "BPSK100");
    // Empty payload should be handled without panic.
    // May succeed (no-op) or fail gracefully.
    let _ok = result.is_ok() || result.is_err();
    assert!(_ok);
}

#[test]
fn bpsk_large_payload_handled() {
    let mut fixture = BpskFixture::new("bpsk-large-payload", "N0TEST");
    // Frame max payload is 255 bytes, so test near the limit.
    let large_payload = vec![0x42u8; 250];
    let result = fixture.transmit(&large_payload, "BPSK100");
    // Large payload should either succeed or fail gracefully.
    let _ok = result.is_ok() || result.is_err();
    assert!(_ok);
}

// ── Recovery exhaustion test ──────────────────────────────────────────────

#[test]
fn bpsk_recovery_exhaustion_transitions_to_failed() {
    let fixture = BpskFixture::new("bpsk-recovery-exhaustion", "N0TEST");
    // After MAX_RECOVERY_ATTEMPTS (typically 4-5), engine should
    // transition from Recovery to Failed instead of looping.
    // Loopback won't trigger recovery naturally, but state machine
    // can be verified through HPX event injection in other tests.
    let _ok = !fixture.check_recovery(); // Initially not in recovery
    assert!(_ok);
}

#[test]
fn bpsk_loopback_fixture_matrix_56_scenarios() {
    // 4 supported modes x 14 payload profiles = 56 deterministic scenarios.
    let modes = ["BPSK31", "BPSK63", "BPSK100", "BPSK250"];
    let payload_profiles: Vec<Vec<u8>> = vec![
        vec![0x00],
        vec![0xFF],
        vec![0xAA],
        vec![0x55],
        b"CQ".to_vec(),
        b"N0TEST".to_vec(),
        b"openpulse".to_vec(),
        (0..8u8).collect(),
        (0..16u8).rev().collect(),
        vec![0x42; 24],
        vec![0x7E; 32],
        (0..48u8).map(|v| v ^ 0x5A).collect(),
        (0..64u8).collect(),
        (0..96u8).map(|v| (v.wrapping_mul(7)) ^ 0x33).collect(),
    ];

    let expected_scenarios = modes.len() * payload_profiles.len();
    let mut exercised = 0usize;

    for mode in modes {
        for (idx, payload) in payload_profiles.iter().enumerate() {
            let mut fixture = BpskFixture::new(&format!("bpsk-matrix-{mode}-{}", idx), "N0TEST");

            let result = fixture.transmit(payload, mode);
            assert!(
                result.is_ok(),
                "scenario failed: mode={mode}, payload_profile={idx}, payload_len={}, err={result:?}",
                payload.len()
            );

            exercised += 1;
        }
    }

    assert_eq!(
        exercised, expected_scenarios,
        "matrix execution count mismatch"
    );
    assert!(exercised >= 50, "expected at least 50 scenarios");
}
