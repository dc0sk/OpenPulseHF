//! Every keyed emission on the ARDOP TNC actually keys the transmitter (#1250).
//!
//! Before this, `assert_ptt` had two call sites — the periodic station ID and the host's manual
//! `PTT TRUE` — while the ISS data path, the ARQ path, the IRS ACK/NACK and relay forwarding all
//! transmitted without touching PTT. With `ptt_backend = "rigctld"` that meant the ID keyed the rig
//! and was heard while every data burst played into an unkeyed transceiver: a silent link failure
//! whose one healthy-looking symptom pointed away from the cause.
//!
//! The scan at the bottom is the part that keeps this honest. A wrapper is only a guarantee if a bare
//! call cannot silently return, so the scan fails on any `engine.transmit*` in `bridge.rs` outside
//! the helper — and it is validated against a planted bare call, so it cannot pass vacuously.

use openpulse_ardop::{spawn_worker, ModemBridge};
use openpulse_audio::LoopbackBackend;
use openpulse_core::handshake::InMemoryTrustStore;
use openpulse_modem::ModemEngine;
use openpulse_radio::{PttController, PttError};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
struct SpyPtt {
    asserted: Arc<AtomicBool>,
    asserts: Arc<AtomicUsize>,
    releases: Arc<AtomicUsize>,
}

impl PttController for SpyPtt {
    fn assert_ptt(&mut self) -> Result<(), PttError> {
        self.asserted.store(true, Ordering::SeqCst);
        self.asserts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn release_ptt(&mut self) -> Result<(), PttError> {
        self.asserted.store(false, Ordering::SeqCst);
        self.releases.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn is_asserted(&self) -> bool {
        self.asserted.load(Ordering::SeqCst)
    }
}

struct Rig {
    bridge: Arc<ModemBridge>,
    asserted: Arc<AtomicBool>,
    asserts: Arc<AtomicUsize>,
    releases: Arc<AtomicUsize>,
}

/// A non-loopback bridge with a spy PTT and a valid MYID, worker running.
fn rig() -> Rig {
    let mut engine = ModemEngine::new(Box::new(LoopbackBackend::default()));
    engine
        .register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register BPSK");
    let asserted = Arc::new(AtomicBool::new(false));
    let asserts = Arc::new(AtomicUsize::new(0));
    let releases = Arc::new(AtomicUsize::new(0));
    let spy = SpyPtt {
        asserted: asserted.clone(),
        asserts: asserts.clone(),
        releases: releases.clone(),
    };
    let (bridge, tx_data_rx) = ModemBridge::with_ptt(
        engine,
        "BPSK250".into(),
        false, // non-loopback: the §97.119 gate and the real TX path
        InMemoryTrustStore::default(),
        None,
        Box::new(spy),
    );
    *bridge.callsign.try_write().expect("callsign") = "DC0SK".into();
    spawn_worker(bridge.clone(), tx_data_rx);
    Rig {
        bridge,
        asserted,
        asserts,
        releases,
    }
}

fn wait_for(f: impl Fn() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

/// The defect: a data frame must key the transmitter.
///
/// Carries its own positive control — the station-ID path keyed even before this fix, so asserting
/// that the spy sees the ID proves the spy is wired to something. Without it a spy connected to
/// nothing would pass the data assertion vacuously.
#[test]
fn a_data_frame_keys_the_transmitter_and_releases_after_it() {
    let rig = rig();

    // Positive control FIRST: the ID path keyed on `main` too, so this must hold in both states.
    rig.bridge.id_requested.store(true, Ordering::Relaxed);
    wait_for(
        || rig.asserts.load(Ordering::SeqCst) >= 1,
        "the station ID to key (positive control — if this fails the spy is not wired)",
    );
    let after_id = rig.asserts.load(Ordering::SeqCst);

    // The defect under test: a data frame.
    rig.bridge
        .tx_data_tx
        .send(b"hello over the air".to_vec())
        .expect("queue tx");
    wait_for(
        || rig.asserts.load(Ordering::SeqCst) > after_id,
        "the data frame to key the transmitter",
    );

    wait_for(
        || !rig.asserted.load(Ordering::SeqCst),
        "PTT to drop after the burst",
    );
    assert_eq!(
        rig.asserts.load(Ordering::SeqCst),
        rig.releases.load(Ordering::SeqCst),
        "every key must be matched by a release"
    );
}

/// The transmitter must be DOWN while the ARQ path listens for its ACK — the reason the bridge runs
/// its own retry loop instead of calling `engine.transmit_arq`, which transmits and listens inside
/// one call.
#[test]
fn the_transmitter_is_released_before_each_ack_listen() {
    let rig = rig();
    rig.bridge
        .tx_data_tx
        .send(b"arq payload".to_vec())
        .expect("queue tx");
    wait_for(
        || rig.asserts.load(Ordering::SeqCst) >= 1,
        "the first transmit to key",
    );
    wait_for(
        || !rig.asserted.load(Ordering::SeqCst),
        "PTT to drop before the ACK listen",
    );
    assert!(
        rig.releases.load(Ordering::SeqCst) >= 1,
        "the guard must release between transmit and the ACK listen"
    );
}

/// A wrapper is only a guarantee if a bare call cannot silently return.
///
/// Validated against a planted bare call, so a scan that matched nothing would fail this test rather
/// than pass it — the vacuous-gate trap this repo has hit three times.
#[test]
fn every_transmit_in_the_bridge_is_inside_the_keyed_helper() {
    let src = include_str!("../src/bridge.rs");

    // Control: the scan can see a bare call at all.
    let planted = "        engine.transmit(&data, &mode, None);\n";
    assert!(
        bare_transmit_lines(&format!("{src}{planted}")).len() == bare_transmit_lines(src).len() + 1,
        "the scan does not detect a planted bare engine.transmit call — it proves nothing"
    );

    let bare = bare_transmit_lines(src);
    assert!(
        bare.is_empty(),
        "these `engine.transmit*` calls are outside `keyed_transmit`, so they emit RF without \
         keying the transmitter (#1250):\n{}",
        bare.join("\n")
    );
}

/// Lines calling an engine transmit method that are NOT inside a `keyed_transmit(...)` call.
///
/// Works by blanking every `keyed_transmit( … )` span (balanced parens, newlines preserved so line
/// numbers stay true) and scanning what is left. An earlier version tracked brace depth instead and
/// produced a FALSE POSITIVE the moment `cargo fmt` collapsed a call onto one line — the block
/// opened and closed on the same line, so the exemption was gone before the check ran. Paren
/// balance cannot be disturbed by reformatting.
fn bare_transmit_lines(src: &str) -> Vec<String> {
    let blanked = blank_keyed_transmit_spans(src);
    blanked
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let t = line.trim_start();
            let is_comment = t.starts_with("//");
            let calls = line.contains("engine.transmit")
                || line.contains("engine.emit_cw_id")
                || line.contains(".transmit_ack_with_short_fec(")
                || line.contains(".transmit_with_fec(");
            calls && !is_comment && !line.contains("fn keyed_transmit")
        })
        .map(|(i, line)| format!("  bridge.rs:{}: {}", i + 1, line.trim()))
        .collect()
}

/// Replace the body of every `keyed_transmit( … )` call with spaces, keeping newlines so line
/// numbers in the report still point at the real file.
fn blank_keyed_transmit_spans(src: &str) -> String {
    let bytes: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = bytes.clone();
    let needle: Vec<char> = "keyed_transmit(".chars().collect();
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if bytes[i..i + needle.len()] == needle[..] {
            // Walk from the opening paren to its match, blanking as we go.
            let mut depth = 0i32;
            let mut j = i + needle.len() - 1;
            while j < bytes.len() {
                match bytes[j] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                if bytes[j] != '\n' {
                    out[j] = ' ';
                }
                j += 1;
            }
            i = j.max(i + needle.len());
        } else {
            i += 1;
        }
    }
    out.into_iter().collect()
}
