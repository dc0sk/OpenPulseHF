//! `receive_with_ack_hint` — the one-shot receive of the ARDOP adaptive IRS and the CLI `arq`
//! command — decodes a frame at its own carrier estimate, and learns nothing from a failed one.
//!
//! Found at the #1062 flag day. The path used to demodulate at the stored AFC correction after
//! moving it by a tenth of this frame's estimate, so a first frame was decoded at its raw offset.
//! The alternating preamble tolerated that up to about 40 Hz on BPSK250; PN-63's longer coherent
//! timing correlation tolerates about 5 Hz. And a failed receive kept its estimate: on silence the
//! estimator reads −400 Hz, so one failure parked the receiver 40 Hz off and the next clean frame
//! failed (`two_way_arq_nack_then_retransmit_succeeds`). Measured on the old path, clean frames,
//! fresh receiver: PN-63 BPSK250 failed at +10 Hz, BPSK63 at +5 Hz.

use bpsk_plugin::BpskPlugin;
use fsk4_plugin::Fsk4Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::channel_sim::ChannelSimHarness;
use openpulse_modem::engine::ModemEngine;

fn harness() -> ChannelSimHarness {
    let mut h = ChannelSimHarness::new();
    for eng in [&mut h.tx_engine, &mut h.rx_engine] {
        eng.register_plugin(Box::new(BpskPlugin::new())).unwrap();
        eng.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
    }
    h
}

/// REQ-PHY-03's ±50 Hz, on every BPSK rung, through the ARDOP/CLI receive.
#[test]
fn a_first_frame_decodes_off_frequency_on_every_bpsk_rung() {
    let payload = b"legacy receive carrier offset";
    for mode in ["BPSK31", "BPSK63", "BPSK100", "BPSK250"] {
        for offset in [-50.0f32, -20.0, 20.0, 50.0] {
            let mut h = harness();
            h.tx_engine.transmit(payload, mode, None).unwrap();
            assert!(h.route_with_cfo(offset) > 0, "nothing routed");
            let got = h.rx_engine.receive_with_ack_hint(mode, None);
            assert_eq!(
                got.as_ref().map(|(p, _)| p.as_slice()).ok(),
                Some(payload.as_slice()),
                "{mode} at {offset:+} Hz: {:?}",
                got.as_ref().err()
            );
        }
    }
}

/// A failed receive leaves the AFC correction where it was, and the next clean frame decodes.
#[test]
fn a_failed_receive_on_silence_moves_no_afc() {
    let payload = b"after a failed receive";
    for mode in ["BPSK100", "BPSK250"] {
        let engine = || {
            let lb = LoopbackBackend::new();
            let mut e = ModemEngine::new(Box::new(lb.clone_shared()));
            e.register_plugin(Box::new(BpskPlugin::new())).unwrap();
            e.register_plugin(Box::new(Fsk4Plugin::new())).unwrap();
            (e, lb)
        };
        let (mut tx, tx_lb) = engine();
        let (mut rx, rx_lb) = engine();
        rx_lb.fill_samples(&vec![0.0f32; 8_000]);
        assert!(rx.receive_with_ack_hint(mode, None).is_err());
        assert_eq!(
            rx.afc_correction_hz(),
            0.0,
            "{mode}: a failed receive on silence moved the AFC correction"
        );
        rx_lb.drain_samples();
        tx.transmit(payload, mode, None).unwrap();
        rx_lb.fill_samples(&tx_lb.drain_samples());
        let got = rx.receive_with_ack_hint(mode, None);
        assert_eq!(
            got.as_ref().map(|(p, _)| p.as_slice()).ok(),
            Some(payload.as_slice()),
            "{mode}: the frame after a failed receive: {:?}",
            got.as_ref().err()
        );
    }
}
