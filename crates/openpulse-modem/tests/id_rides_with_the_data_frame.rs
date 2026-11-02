//! A station ID sent in the same keying as a data frame must not cost the receiver the data frame
//! (review 2026-11-02: the ID rides in the station's own transmission). It goes AFTER the data frame:
//! in front of it, the receiver's uncoded fallback decodes the ID, returns, and never finds the ladder
//! frame behind it (measured: the data frame was lost at every `robust` rung).

use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::ModemEngine;

const ID_MODE: &str = "BPSK250";
const PAYLOAD: &[u8] = b"data frame keyed with the station ID .............................";

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
    e.start_ota_session(SessionProfile::robust());
    e.ota_lock_level(level);
    e
}

fn burst_for(level: SpeedLevel) -> Vec<f32> {
    let bk = LoopbackBackend::new();
    let mut tx = engine(&bk, level);
    let mode = tx.ota_tx_mode().expect("mode").to_owned();
    let fec = tx.ota_tx_fec();
    tx.transmit_with_fec_mode(PAYLOAD, &mode, fec, None)
        .expect("data");
    tx.transmit(b"DE DC0SK", ID_MODE, None).expect("id");
    let mut audio = vec![0.0f32; 2_400];
    audio.extend(bk.drain_samples());
    audio.extend(vec![0.0f32; 16_000]);
    audio
}

#[test]
fn a_data_frame_followed_by_an_id_decodes_at_every_robust_rung() {
    let mut failed = Vec::new();
    for level in [
        SpeedLevel::Sl2,
        SpeedLevel::Sl3,
        SpeedLevel::Sl4,
        SpeedLevel::Sl5,
        SpeedLevel::Sl6,
    ] {
        let audio = burst_for(level);
        let rx_bk = LoopbackBackend::new();
        let mut rx = engine(&rx_bk, level);
        let mut got = false;
        for chunk in audio.chunks(800) {
            if let Some(b) = rx
                .accumulate_capture(Some(ID_MODE), chunk.to_vec())
                .unwrap()
            {
                let res = rx.ota_decode_burst(&b, "DC0SK", Some(ID_MODE)).unwrap();
                // A ladder decode carries an ACK; the uncoded fallback (non-ladder traffic) does not.
                let ladder = res.ack.is_some();
                let all: Vec<Vec<u8>> = res.payload.into_iter().chain(res.more).collect();
                println!(
                    "{level:?}: burst {} samples, frames {:?}",
                    b.samples.len(),
                    all.iter().map(|p| p.len()).collect::<Vec<_>>()
                );
                got |= ladder && all.iter().any(|p| p.as_slice() == PAYLOAD);
            }
        }
        if !got {
            failed.push(level);
        }
    }
    assert!(
        failed.is_empty(),
        "the data frame keyed with the station ID did not decode at {failed:?}"
    );
}
