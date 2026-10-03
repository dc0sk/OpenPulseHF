//! An off-frequency frame the uncoded fallback recovers is non-ladder traffic, as it is on frequency
//! (#1123). Phase 2 of the OTA decode carried the fallback mode as an `Sl1` candidate, so a station
//! ID or file fragment that needed the acquisition pass was reported as an SL1 ladder decode: the
//! rate controller moved and the daemon keyed an ACK at it. It also yielded the first frame of a
//! multi-fragment keying only (#1461).
//!
//! Driven through `accumulate_capture` and `ota_decode_burst` on real recorded idle, with a `fast`
//! session at its SL2 entry rung, where the fallback mode (BPSK250) is not itself a ladder candidate.
//! Not locked: a locked controller never moves, which would make the rung assertion vacuous.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::loopback::LoopbackBackend;
use openpulse_core::fec::FecMode;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel;
use openpulse_modem::capture_replay::load_corpus;
use openpulse_modem::channel_sim::ChannelSimHarness;
use openpulse_modem::engine::{ModemEngine, OtaRxResult};

const FALLBACK: &str = "BPSK250";
const FIRST: &[u8] = b"off-frequency station ID";
const SECOND: &[u8] = b"second fragment, same keying";
/// Outside every decision arm's native reach, so only phase 2 can recover it
/// (`daemon_frequency_acquisition::ACQUISITION_OFFSET_HZ`).
const OFFSET_HZ: f32 = 100.0;
const TICK: usize = 400;

/// `frames`, sent uncoded at the fallback mode in one keying, shifted by `OFFSET_HZ` and embedded in
/// recorded idle.
fn keying(frames: &[&[u8]]) -> Vec<f32> {
    let idle = load_corpus("ic9700-idle-hot.wav").expect("corpus idle");
    let mut tx = ChannelSimHarness::new();
    tx.tx_engine
        .register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    for f in frames {
        tx.tx_engine
            .transmit_with_fec_mode(f, FALLBACK, FecMode::None, None)
            .expect("transmit");
    }
    let mut cfo = openpulse_channel::cfo::CfoChannel::new(openpulse_channel::cfo::CfoConfig::new(
        OFFSET_HZ, 8000.0,
    ))
    .expect("finite offset");
    let (_, shifted) = tx.route_tapped(&mut cfo);
    let mut buf = idle.cycled(0, 4_032);
    buf.extend_from_slice(&shifted);
    buf.extend(idle.cycled(4_032, 1_600));
    buf
}

/// Every result the receiver produced for `samples`, its confirmed level before and after, and the
/// settle attempts it spent.
fn receive(samples: &[f32]) -> (Vec<OtaRxResult>, [Option<SpeedLevel>; 2], u64) {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register");
    e.start_ota_session(SessionProfile::fast());
    let before = e.ota_rx_confirmed_level();
    let mut out = Vec::new();
    let tail = vec![0.0; 8 * TICK];
    for chunk in samples.chunks(TICK).chain(tail.chunks(TICK)) {
        if let Ok(Some(b)) = e.accumulate_capture(Some(FALLBACK), chunk.to_vec()) {
            if let Ok(r) = e.ota_decode_burst(&b, "peer", Some(FALLBACK)) {
                out.push(r);
            }
        }
    }
    (
        out,
        [before, e.ota_rx_confirmed_level()],
        e.afc_settle_attempts(),
    )
}

#[test]
fn an_off_frequency_fallback_frame_keys_no_ack_and_moves_no_rung() {
    let (results, [before, after], settles) = receive(&keying(&[FIRST]));
    let hit = results
        .iter()
        .find(|r| r.payload.as_deref() == Some(FIRST))
        .expect("the off-frequency frame did not decode at all");
    assert!(
        settles > 0,
        "decoded without the acquisition pass; the fixture no longer reaches phase 2"
    );
    assert_eq!(
        before,
        Some(SpeedLevel::Sl2),
        "the session no longer enters at SL2"
    );
    assert_eq!(
        after, before,
        "non-ladder traffic moved the rate controller"
    );
    assert!(
        hit.ack.is_none(),
        "an off-frequency non-ladder frame was answered with an ACK (the #1123 failure mode)"
    );
}

#[test]
fn an_off_frequency_multi_fragment_keying_yields_every_frame() {
    let (results, _, settles) = receive(&keying(&[FIRST, SECOND]));
    let hit = results
        .iter()
        .find(|r| r.payload.as_deref() == Some(FIRST))
        .expect("the first frame did not decode");
    assert!(
        settles > 0,
        "decoded without the acquisition pass; the fixture no longer reaches phase 2"
    );
    assert!(
        hit.ack.is_none(),
        "a non-ladder keying was answered with an ACK"
    );
    assert_eq!(
        hit.more,
        vec![SECOND.to_vec()],
        "the second frame of the keying was lost"
    );
}
