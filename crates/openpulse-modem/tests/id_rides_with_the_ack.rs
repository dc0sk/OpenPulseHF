//! The receiver's station ID rides in front of its ACK, in the same keying (review 2026-11-02): the
//! sender's ACK listen must still find the ACK behind it, at every `robust` rung's ACK waveform
//! (FSK4, and the K=3 MFSK16-ACK with its leading FSK4 copy when recommending SL1).

use openpulse_audio::LoopbackBackend;
use openpulse_core::ack::{AckFrame, AckType};
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::ModemEngine;

fn engine() -> (ModemEngine, LoopbackBackend) {
    let bk = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(bk.clone_shared()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(fsk4_plugin::Fsk4Plugin::new()))
        .unwrap();
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .unwrap();
    e.start_ota_session(SessionProfile::robust());
    (e, bk)
}

#[test]
fn an_ack_behind_the_station_id_is_heard() {
    for level in [SpeedLevel::Sl1, SpeedLevel::Sl3, SpeedLevel::Sl6] {
        let (mut irs, irs_bk) = engine();
        let (mut iss, iss_bk) = engine();
        let ack = AckFrame::new(AckType::AckOk, "DD2ZM").with_recommended_level(level);
        irs.transmit(b"DE DD2ZM", "BPSK250", None).expect("id");
        irs.transmit_ota_ack(&ack, None).expect("ack");
        iss_bk.fill_samples(&irs_bk.drain_samples());
        let got = iss
            .receive_ota_ack_within(None, iss.ota_ack_timeout_ms(), None)
            .unwrap_or_else(|e| panic!("{level:?}: the ACK behind the ID was not heard: {e}"));
        assert_eq!(got.recommended_level, Some(level), "{level:?}");
    }
}
