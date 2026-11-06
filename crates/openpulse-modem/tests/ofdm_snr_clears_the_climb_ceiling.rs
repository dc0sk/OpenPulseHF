//! On a clean channel, the SNR the OFDM rungs read on the daemon's receive path clears each rung's
//! climb ceiling in the `fast` profile, so `climb_on_snr` can fire, and SL14's admission (it requires
//! an SNR upgrade candidate) can be met.
//!
//! Before 2026-11-03 the OFDM estimate equalised the whole receive slice, the noise after the frame
//! included, and SL7 read 6.6 dB over a 0.002 floor against a 12 dB ceiling; the fast ladder could
//! climb the OFDM rungs only on evidence, and never reach SL14.

use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::ModemEngine;

fn engine(bk: &LoopbackBackend, level: SpeedLevel) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(bk.clone_shared()));
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
        .unwrap();
    e.start_ota_session(SessionProfile::fast());
    e.ota_lock_level(level);
    e
}

#[test]
fn every_ofdm_rungs_snr_reading_clears_its_climb_ceiling() {
    let profile = SessionProfile::fast();
    for level in [
        SpeedLevel::Sl7,
        SpeedLevel::Sl8,
        SpeedLevel::Sl9,
        SpeedLevel::Sl10,
        SpeedLevel::Sl11,
        SpeedLevel::Sl12,
        SpeedLevel::Sl13,
    ] {
        let ceiling = profile
            .snr_ceiling_for_level(level)
            .expect("an OFDM rung below the top has a ceiling");
        let bk = LoopbackBackend::new();
        let mut tx = engine(&bk, level);
        let mode = tx.ota_tx_mode().unwrap().to_owned();
        let fec = tx.ota_tx_fec();
        tx.transmit_with_fec_mode(b"a clean OFDM ladder frame", &mode, fec, None)
            .unwrap();
        // The daemon's slice: quiet, the frame, then a second of channel noise before the flush.
        let mut audio = vec![0.0f32; 16_000];
        audio.extend(bk.drain_samples());
        audio.extend(vec![0.0f32; 16_000]);
        let mut st = 0x2545_f491_4f6c_dd1d_u64;
        for s in audio.iter_mut() {
            st ^= st << 13;
            st ^= st >> 7;
            st ^= st << 17;
            *s += ((st >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.002 * 3.46;
        }
        let rx_bk = LoopbackBackend::new();
        let mut rx = engine(&rx_bk, level);
        let mut decoded = false;
        for chunk in audio.chunks(800) {
            if let Some(b) = rx
                .accumulate_capture(Some("BPSK250"), chunk.to_vec())
                .unwrap()
            {
                decoded |= rx
                    .ota_decode_burst(&b, "DC0SK", None)
                    .is_ok_and(|r| r.payload.is_some());
            }
        }
        let snr = rx.last_rx_snr_db();
        println!("{level:?} {mode}: SNR {snr:?} against the climb ceiling {ceiling}");
        assert!(decoded, "{level:?}: the clean frame did not decode");
        assert!(
            snr.is_some_and(|s| s >= ceiling),
            "{level:?} read SNR {snr:?} on a clean channel, under its climb ceiling {ceiling} dB: \
             the ladder cannot climb on SNR here (the estimate counts the slice's tail as data?)"
        );
    }
}
