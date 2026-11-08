//! The decoded frame's airtime reaches the OTA rate controller (`ota-climb-memory.md`, change 2).
//!
//! On air a climb from SL2 on evidence took three 66.56 s frames, 3.5 minutes. With the frame's own
//! coded airtime passed in, one SL2 decode proves the rung. The controller's unit tests prove the
//! rule; this proves the engine feeds it, through `ota_decode_burst`, the daemon's entry.

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::ota_rate::RateDecision;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::engine::ModemEngine;
use openpulse_modem::pipeline::AudioSamples;
use openpulse_modem::EngineEvent;

const SESSION: &str = "sess-airtime";
const MODE: &str = "BPSK250";

fn engine(lb: &LoopbackBackend) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
    e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
    e.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    e.start_ota_session(SessionProfile::fast());
    e
}

#[test]
fn the_decoded_frames_airtime_reaches_the_controller() {
    let tx_lb = LoopbackBackend::new();
    let mut tx = engine(&tx_lb);
    let payload = b"one SL2 frame proves the rung";
    let mode = tx.ota_tx_mode().expect("session").to_owned();
    assert_eq!(mode, "BPSK31", "the fast profile enters on SL2");
    let fec = tx.ota_tx_fec();
    tx.transmit_with_fec_mode(payload, &mode, fec, None)
        .expect("transmit");
    let mut samples = vec![0.0; 8_000];
    samples.extend(tx_lb.drain_samples());
    samples.extend(vec![0.0; 8_000]);

    let rx_lb = LoopbackBackend::new();
    let mut rx = engine(&rx_lb);
    // A reading below SL2's 6 dB climb ceiling, as on air (−16 to +6 dB at SL2): no climb on SNR.
    rx.set_rx_snr_estimate(Some(0.0));
    let mut events = rx.subscribe();
    let res = rx
        .ota_decode_burst(&AudioSamples { samples }, SESSION, Some(MODE))
        .expect("decode");
    assert_eq!(
        res.payload.as_deref(),
        Some(&payload[..]),
        "the frame must decode"
    );

    let mut decisions = Vec::new();
    while let Ok(ev) = events.try_recv() {
        if let EngineEvent::OtaRateDecision { decision, to, .. } = ev {
            decisions.push((decision, to));
        }
    }
    assert_eq!(
        decisions,
        [(RateDecision::ClimbOnEvidence, SpeedLevel::Sl3)],
        "one decoded SL2 frame (66.56 s of airtime) proves the rung"
    );
}
