//! Wall-clock cost of an OFDM OTA burst decode on the machine it runs on: a clean frame 18 Hz off
//! carrier, and a frame faded below its rung's floor that fails (the case that pays the whole
//! carrier search on every attempt, `docs/dev/design/ofdm-carrier-offset.md`). A measurement for the
//! station Pis, not a gate: `#[ignore]`, run with
//! `cargo test --release -p openpulse-modem --no-default-features --features instruments --test ofdm_decode_cost -- --ignored --nocapture`.

use std::time::Instant;

use openpulse_audio::LoopbackBackend;
use openpulse_channel::cfo::{CfoChannel, CfoConfig};
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::ModemEngine;

fn engine(bk: &LoopbackBackend, level: SpeedLevel) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(bk.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
        .unwrap();
    e.start_ota_session(SessionProfile::fast());
    e.ota_lock_level(level);
    e
}

/// Decodes `audio` through the daemon's entry; returns (decoded, attempts, seconds in decode).
fn decode(level: SpeedLevel, audio: &[f32]) -> (bool, u64, f64) {
    let bk = LoopbackBackend::new();
    let mut rx = engine(&bk, level);
    let before = rx.decode_attempts();
    let (mut ok, mut secs) = (false, 0.0);
    for chunk in audio.chunks(800) {
        if let Some(b) = rx
            .accumulate_capture(Some("BPSK250"), chunk.to_vec())
            .unwrap()
        {
            let t = Instant::now();
            ok |= rx
                .ota_decode_burst(&b, "DC0SK", None)
                .is_ok_and(|r| r.payload.is_some());
            secs += t.elapsed().as_secs_f64();
        }
    }
    (ok, rx.decode_attempts() - before, secs)
}

#[test]
#[ignore = "a measurement for the station Pis, not a gate"]
fn ofdm_decode_cost_on_this_machine() {
    for level in [SpeedLevel::Sl7, SpeedLevel::Sl10, SpeedLevel::Sl14] {
        let bk = LoopbackBackend::new();
        let mut tx = engine(&bk, level);
        let mode = tx.ota_tx_mode().unwrap().to_owned();
        let fec = tx.ota_tx_fec();
        tx.transmit_with_fec_mode(&[0x5a; 200], &mode, fec, None)
            .unwrap();
        let frame = bk.drain_samples();
        let framed = |sig: Vec<f32>| {
            let mut a = vec![0.0f32; 16_000];
            a.extend(sig);
            a.extend(vec![0.0f32; 16_000]);
            let mut st = 0x9e37_79b9_7f4a_7c15_u64;
            for s in a.iter_mut() {
                st ^= st << 13;
                st ^= st >> 7;
                st ^= st << 17;
                *s += ((st >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.002 * 3.46;
            }
            a
        };
        let clean = CfoChannel::new(CfoConfig::new(18.0, 8000.0))
            .unwrap()
            .apply(&framed(frame.clone()));
        let mut cfg = WattersonConfig::moderate_f1(Some(4));
        cfg.snr_db = 0.0;
        let failing = framed(WattersonChannel::new(cfg).unwrap().apply(&frame));
        for (name, audio) in [
            ("clean, 18 Hz off", clean),
            ("faded below its floor", failing),
        ] {
            let (ok, attempts, secs) = decode(level, &audio);
            println!(
                "COST {level:?} {mode} {name}: decoded {ok}, {attempts} attempts, {:.0} ms in decode",
                secs * 1000.0
            );
        }
    }
}
