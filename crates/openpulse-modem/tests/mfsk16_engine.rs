//! MFSK16 through the production engine path (REQ-WSIG-01): register the plugin, `transmit_with_fec` /
//! `receive_with_fec` (RS), and confirm it decodes clean and survives a moderate Watterson fade where its
//! measured advantage lives — proving the plugin works through the real receive seam, not just at the
//! plugin API (the seam-gap lesson).

use mfsk16_plugin::Mfsk16Plugin;
use openpulse_audio::LoopbackBackend;
use openpulse_channel::{watterson::WattersonChannel, ChannelModel, WattersonConfig};
use openpulse_modem::engine::ModemEngine;

const PAYLOAD: &[u8] = b"MFSK16 engine-path round-trip payload, sixty-four bytes AAAAAAAAAA";
const MODE: &str = "MFSK16";

fn engine() -> (ModemEngine, LoopbackBackend) {
    let backend = LoopbackBackend::new();
    let mut e = ModemEngine::new(Box::new(backend.clone_shared()));
    e.register_plugin(Box::new(Mfsk16Plugin::new()))
        .expect("register");
    e.set_center_frequency(1500.0);
    (e, backend)
}

#[test]
fn mfsk16_round_trips_through_the_engine_fec_path() {
    let (mut tx, backend) = engine();
    tx.transmit_with_fec(PAYLOAD, MODE, None).expect("transmit");
    let audio = backend.drain_samples();

    let (mut rx, rxb) = engine();
    rxb.fill_samples(&audio);
    let out = rx.receive_with_fec(MODE, None).expect("receive");
    assert_eq!(
        out, PAYLOAD,
        "MFSK16 must round-trip clean through the engine RS path"
    );
}

#[test]
fn mfsk16_decodes_through_a_moderate_watterson_fade() {
    // Where BPSK31 struggles (moderate_f1) MFSK16's non-coherent detection holds. A modest SNR that the
    // measurement showed comfortably above the crossing (~0 dB), a few seeds — this is a smoke gate, not
    // a sweep.
    let (mut tx, backend) = engine();
    tx.transmit_with_fec(PAYLOAD, MODE, None).expect("transmit");
    let audio = backend.drain_samples();

    let mut ok = 0;
    for seed in 0..6u64 {
        let mut cfg = WattersonConfig::moderate_f1(Some(700 + seed));
        cfg.snr_db = 6.0;
        let faded = WattersonChannel::new(cfg).expect("watterson").apply(&audio);
        let (mut rx, rxb) = engine();
        rxb.fill_samples(&faded);
        if rx
            .receive_with_fec(MODE, None)
            .map(|d| d == PAYLOAD)
            .unwrap_or(false)
        {
            ok += 1;
        }
    }
    assert!(
        ok >= 5,
        "MFSK16 must decode moderate_f1 @6 dB most of the time (got {ok}/6)"
    );
}
