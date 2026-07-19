//! `run_full_duplex` must honour `config.full_duplex` (audit 2026-07-19, finding #2).
//!
//! `full_duplex` defaults to **false**, and its own doc comment reads "When true, PTT is held for the
//! entire relay session by `run_full_duplex()`". Four methods in the crate check the flag before
//! keying; `run_full_duplex` — the one that actually keys the transmitter for the whole session — did
//! not. Enabling the repeater on a default config therefore held an unbounded dead-air carrier, and
//! double-keyed against the per-frame assert/release that half-duplex relaying already does.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_modem::ModemEngine;
use openpulse_radio::{PttController, PttError};
use openpulse_repeater::{CrossBandRepeater, RepeaterConfig};

/// Records the exact PTT edge sequence so a test can assert ordering, not just totals.
#[derive(Clone, Default)]
struct SpyPtt {
    edges: Arc<Mutex<Vec<&'static str>>>,
    asserted: Arc<AtomicBool>,
    peak_concurrent: Arc<AtomicUsize>,
}

impl SpyPtt {
    fn edges(&self) -> Vec<&'static str> {
        self.edges.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl PttController for SpyPtt {
    fn assert_ptt(&mut self) -> Result<(), PttError> {
        // Catch a double-key: asserting while already asserted is the half-duplex bug's signature.
        if self.asserted.swap(true, Ordering::SeqCst) {
            self.peak_concurrent.store(2, Ordering::SeqCst);
        }
        self.edges
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push("assert");
        Ok(())
    }
    fn release_ptt(&mut self) -> Result<(), PttError> {
        self.asserted.store(false, Ordering::SeqCst);
        self.edges
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push("release");
        Ok(())
    }
    fn is_asserted(&self) -> bool {
        self.asserted.load(Ordering::SeqCst)
    }
}

fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    // Without a plugin the relay loop errors on the first iteration and the session ends before the
    // PTT behaviour under test can be observed.
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register bpsk");
    e
}

/// Run a repeater session briefly, then stop it, and return the observed PTT edges.
///
/// The session's return value is deliberately ignored: on an idle loopback `relay_one_frame` errors
/// ("signal too short") and the loop exits early. That is incidental here — the question this file
/// asks is whether the transmitter was keyed *at session start*, which happens before any relaying,
/// and whether it was left keyed afterwards. Both hold however the session ends.
fn run_session(full_duplex: bool) -> SpyPtt {
    let spy = SpyPtt::default();
    let config = RepeaterConfig {
        enabled: true,
        full_duplex,
        ..Default::default()
    };
    let mut rp = CrossBandRepeater::new(Box::new(spy.clone()), engine(), engine(), config);

    let stop = Arc::new(AtomicBool::new(false));
    let stop_c = stop.clone();
    // Let the loop spin a few times with no traffic, then ask it to stop.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        stop_c.store(true, Ordering::Relaxed);
    });
    let _ = rp.run_full_duplex(stop);
    spy
}

/// THE GATE: with `full_duplex = false` (the default), an idle session must never key the rig.
///
/// No traffic is relayed here, so in half-duplex there is nothing to transmit and therefore no
/// legitimate reason for the transmitter to come up at all.
#[test]
fn half_duplex_session_does_not_hold_ptt() {
    let spy = run_session(false);

    assert_eq!(
        spy.edges(),
        Vec::<&str>::new(),
        "half-duplex idle session keyed the transmitter — an unbounded dead-air carrier on the \
         DEFAULT config (full_duplex defaults to false)"
    );
    assert!(
        !spy.is_asserted(),
        "transmitter left keyed after the session ended"
    );
}

/// Control: `full_duplex = true` must still hold PTT for the session, as its doc comment promises,
/// and must release it at the end. Without this the fix could simply disable the feature.
#[test]
fn full_duplex_session_holds_then_releases_ptt() {
    let spy = run_session(true);

    assert_eq!(
        spy.edges(),
        vec!["assert", "release"],
        "full-duplex must key once for the session and release once at the end"
    );
    assert!(
        !spy.is_asserted(),
        "transmitter left keyed after a full-duplex session"
    );
}

/// Control: a disabled repeater must not key at all, in either mode.
#[test]
fn disabled_repeater_never_keys() {
    for full_duplex in [false, true] {
        let spy = SpyPtt::default();
        let config = RepeaterConfig {
            enabled: false,
            full_duplex,
            ..Default::default()
        };
        let mut rp = CrossBandRepeater::new(Box::new(spy.clone()), engine(), engine(), config);
        let relayed = rp
            .run_full_duplex(Arc::new(AtomicBool::new(false)))
            .expect("a disabled repeater returns Ok(0) immediately");

        assert_eq!(relayed, 0);
        assert_eq!(
            spy.edges(),
            Vec::<&str>::new(),
            "a disabled repeater keyed the transmitter (full_duplex={full_duplex})"
        );
    }
}
