//! The receiver notch, on the daemon's capture path, protects every rung the OTA receiver will try,
//! not only the mode the caller names (`docs/dev/design/notch-protects-the-ladder.md`).
//!
//! On air (task 22, run f1b) the daemon passed its base mode, BPSK250, so the notch protected 500 Hz
//! around the centre and cut the OFDM rungs' edge subcarriers at 500 and 2500 Hz: B read SL8 at a
//! median 5.0 dB for a frame that reads 12.8 dB offline. Measured on this path before the fix, at
//! 0.02 RMS noise: SL8 20.5 dB with the notch off, 5.9 with it on.

use openpulse_audio::LoopbackBackend;
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

const PAYLOAD: &[u8] = b"an OFDM ladder frame through the receive notch, sixty-four bytes!";

/// The SNR the engine reads for one ladder frame at `level` over a 0.02-RMS floor, received through
/// `accumulate_capture` with the daemon's base mode, with the notch as configured on air or off.
fn snr_at(level: SpeedLevel, notch: bool) -> Option<f32> {
    let bk = LoopbackBackend::new();
    let mut tx = engine(&bk, level);
    let mode = tx.ota_tx_mode().unwrap().to_owned();
    let fec = tx.ota_tx_fec();
    tx.transmit_with_fec_mode(PAYLOAD, &mode, fec, None)
        .unwrap();
    let mut audio = vec![0.0f32; 16_000];
    audio.extend(bk.drain_samples());
    audio.extend(vec![0.0f32; 16_000]);
    let mut st = 0x1234_5678_9abc_def1_u64;
    for s in audio.iter_mut() {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        *s += ((st >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.02 * 3.46;
    }
    let rx_bk = LoopbackBackend::new();
    let mut rx = engine(&rx_bk, level);
    if notch {
        // The stations' configuration (`notch_max` 10, `notch_q` 25) and the daemon's fallback.
        rx.configure_notch(10, 25.0, 2000.0);
        rx.enable_notch();
    }
    let mut decoded = false;
    for chunk in audio.chunks(800) {
        if let Some(b) = rx
            .accumulate_capture(Some("BPSK250"), chunk.to_vec())
            .unwrap()
        {
            if let Ok(r) = rx.ota_decode_burst(&b, "DC0SK", None) {
                decoded |= r.payload.as_deref() == Some(PAYLOAD);
            }
        }
    }
    assert!(decoded, "{level:?} (notch {notch}) did not decode");
    if notch {
        assert!(
            rx.notch_blocks_processed() > 0,
            "the notch never ran on this path"
        );
    }
    rx.last_rx_snr_db()
}

#[test]
fn the_notch_leaves_every_ofdm_rung_its_snr() {
    for level in [
        SpeedLevel::Sl7,
        SpeedLevel::Sl8,
        SpeedLevel::Sl9,
        SpeedLevel::Sl10,
    ] {
        let off = snr_at(level, false).expect("an SNR reading without the notch");
        let on = snr_at(level, true).expect("an SNR reading with the notch");
        println!("{level:?}: notch off {off:.1} dB, on {on:.1} dB");
        assert!(
            on >= off - 1.0,
            "{level:?}: the notch cut the SNR reading from {off:.1} to {on:.1} dB"
        );
    }
}
