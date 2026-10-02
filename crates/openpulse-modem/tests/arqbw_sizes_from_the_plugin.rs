//! ARDOP `ARQBW` maps a host bandwidth cap to a ladder level using the registered plugins' own
//! occupied bandwidth, not the bandplan's hand-kept table.
//!
//! The table was a stale twin: it lacked MFSK16, QPSK250-D and every OFDM52-* variant, and listed
//! OFDM52 at 3200 Hz against the plugin's 2031 Hz. A mode it could not size was dropped, so
//! QPSK250-D (SL6) was unreachable at every `ARQBW`, and `fast` stopped at SL5 for any cap of
//! 2032 Hz or more. When nothing fitted it answered "no cap", which uncapped the ladder.

use openpulse_audio::LoopbackBackend;
use openpulse_core::profile::SessionProfile;
use openpulse_core::rate::SpeedLevel::{self, *};
use openpulse_modem::ModemEngine;

fn engine(profile: SessionProfile) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(qpsk_plugin::QpskPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(ofdm_plugin::OfdmPlugin::new()))
        .unwrap();
    e.register_plugin(Box::new(mfsk16_plugin::Mfsk16Plugin::new()))
        .unwrap();
    e.start_adaptive_session(profile);
    e
}

fn cap(e: &ModemEngine, hz: u32) -> SpeedLevel {
    e.arq_max_tx_level_for_bandwidth(hz)
        .expect("an active session")
}

#[test]
fn fast_maps_each_arqbw_to_the_widest_rung_that_fits() {
    let e = engine(SessionProfile::fast());
    // BPSK31/63/100/250 occupy 62/126/200/500 Hz (2 x baud); MFSK16 and QPSK250-D 500; OFDM52 ~2031.
    assert_eq!(cap(&e, 200), Sl4, "BPSK100");
    assert_eq!(
        cap(&e, 500),
        Sl6,
        "QPSK250-D fits 500 Hz — the table never let it"
    );
    assert_eq!(cap(&e, 2000), Sl6, "OFDM52 needs ~2031 Hz");
    assert_eq!(cap(&e, 2500), Sl14, "a 2.5 kHz host reaches the ladder top");
}

#[test]
fn a_cap_narrower_than_every_rung_is_the_lowest_rung_not_no_cap() {
    let e = engine(SessionProfile::fast());
    assert_eq!(cap(&e, 50), Sl1);
}

#[test]
fn robust_never_sizes_past_its_cap() {
    let e = engine(SessionProfile::robust());
    assert_eq!(cap(&e, 2500), Sl6);
}

#[test]
fn no_session_no_answer() {
    let e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    assert_eq!(e.arq_max_tx_level_for_bandwidth(2500), None);
}
