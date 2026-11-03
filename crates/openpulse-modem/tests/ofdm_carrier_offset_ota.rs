//! The OFDM rungs decode through a rig's carrier offset on the daemon's receive path
//! (`accumulate_capture` → `ota_decode_burst`), and the SNR the ladder reads is not cut by it
//! (`docs/dev/design/ofdm-carrier-offset.md`).
//!
//! Measured 2026-11-03 before the fix, on this path: SL14 (64QAM) failed at 2.5 Hz, SL10 at 5 Hz, SL7
//! at 10 Hz. The OFDM receiver refused or ignored a moved centre and nothing corrected the offset;
//! rigs are 4 Hz apart on 2 m after trimming, 54–64 Hz untrimmed, and commonly 10–20 Hz on HF.

use openpulse_audio::LoopbackBackend;
use openpulse_channel::cfo::{CfoChannel, CfoConfig};
use openpulse_channel::ChannelModel;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::ModemEngine;

fn engine(bk: &LoopbackBackend, level: SpeedLevel) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(bk.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
        .unwrap();
    e.start_ota_session(SessionProfile::fast());
    e.ota_lock_level(level);
    e
}

const PAYLOAD: &[u8] = b"an OFDM ladder frame through a rig's carrier offset";

/// Decodes one ladder frame at `level`, shifted by `hz` over a 0.002-RMS floor. Returns whether it
/// decoded, the attempts it took, and the SNR the engine read.
fn decode_at(level: SpeedLevel, hz: f32) -> (bool, u64, Option<f32>) {
    let bk = LoopbackBackend::new();
    let mut tx = engine(&bk, level);
    let mode = tx.ota_tx_mode().unwrap().to_owned();
    let fec = tx.ota_tx_fec();
    tx.transmit_with_fec_mode(PAYLOAD, &mode, fec, None)
        .unwrap();
    let mut audio = vec![0.0f32; 16_000];
    audio.extend(bk.drain_samples());
    audio.extend(vec![0.0f32; 16_000]);
    let mut audio = CfoChannel::new(CfoConfig::new(hz, 8000.0))
        .unwrap()
        .apply(&audio);
    let mut st = 0x1234_5678_9abc_def1_u64;
    for s in audio.iter_mut() {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        *s += ((st >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.002 * 3.46;
    }

    let rx_bk = LoopbackBackend::new();
    let mut rx = engine(&rx_bk, level);
    let before = rx.decode_attempts();
    let mut ok = false;
    for chunk in audio.chunks(800) {
        if let Some(b) = rx
            .accumulate_capture(Some("BPSK250"), chunk.to_vec())
            .unwrap()
        {
            if let Ok(r) = rx.ota_decode_burst(&b, "DC0SK", None) {
                ok |= r.payload.as_deref() == Some(PAYLOAD);
            }
        }
    }
    (ok, rx.decode_attempts() - before, rx.last_rx_snr_db())
}

#[test]
fn every_ofdm_rung_decodes_through_a_carrier_offset() {
    for level in [SpeedLevel::Sl7, SpeedLevel::Sl10, SpeedLevel::Sl14] {
        let (_, _, snr0) = decode_at(level, 0.0);
        for hz in [2.5f32, -7.5, 15.0, -40.0] {
            let (ok, attempts, snr) = decode_at(level, hz);
            println!("{level:?} {hz:+} Hz: decoded {ok}, {attempts} attempts, SNR {snr:?} (0 Hz: {snr0:?})");
            assert!(
                ok,
                "{level:?} did not decode {hz:+} Hz off carrier ({attempts} attempts): the OFDM \
                 receiver did not acquire the offset (ofdm_plugin::acquire)"
            );
            assert!(
                attempts <= 3,
                "{level:?} at {hz:+} Hz took {attempts} attempts (budget 3)"
            );
            if let (Some(a), Some(b)) = (snr0, snr) {
                assert!(
                    b >= a - 3.0,
                    "{level:?} at {hz:+} Hz read SNR {b:.1} dB against {a:.1} on carrier: an \
                     uncorrected offset would hold the ladder down"
                );
            }
        }
    }
}
