//! Every emission the daemon makes keys the transmitter (#1262).
//!
//! The daemon held five `ptt.keyed` guards, all in `server.rs`, while `lib.rs` transmitted from five
//! further sites with none: both QSY lines, the handshake frame (CONREQ and CONACK), the relay
//! forward, and the non-OTA `SendMessage` fallback. With `ptt_backend = "rigctld"` those emissions
//! played into an unkeyed rig — the handshake that opens a session, the QSY negotiation that escapes
//! interference, and the relay of somebody else's traffic.
//!
//! The code already reasoned about them as on-air emissions: three comments justify the §97.119
//! callsign gate by saying the path "keys the transmitter". It did not.
//!
//! **There is no natural positive control here.** In #1250's ARDOP fix the station-ID path keyed
//! before the fix, so asserting the spy saw it proved the spy was wired. No `lib.rs` site keyed
//! before this change, so each test keys the shared PTT *directly* first and asserts the spy counted
//! it — that rules out the same vacuity (a spy connected to nothing) by a different route.

use openpulse_audio::LoopbackBackend;
use openpulse_daemon::protocol::{ControlCommand, ControlEvent};
use openpulse_daemon::ptt::SharedPtt;
use openpulse_daemon::{apply_command_to_engine, RuntimeControlState};
use openpulse_modem::ModemEngine;
use openpulse_radio::{PttController, PttError, DEFAULT_PTT_MAX};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

#[derive(Clone, Default)]
struct Spy {
    asserted: Arc<AtomicBool>,
    asserts: Arc<AtomicUsize>,
    releases: Arc<AtomicUsize>,
}

impl PttController for Spy {
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

fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register BPSK");
    e
}

/// Runtime state whose PTT is a spy, plus the spy's counters.
fn state_with_spy() -> (RuntimeControlState, Spy) {
    let spy = Spy::default();
    let rs = RuntimeControlState {
        ptt: SharedPtt::new(Some(Box::new(spy.clone())), DEFAULT_PTT_MAX),
        ..Default::default()
    };
    (rs, spy)
}

/// Substitute for the missing positive control: prove the spy is wired to this `SharedPtt` before
/// asserting anything about the code under test. Without it, a spy connected to nothing would make
/// every "did it key?" assertion pass for the wrong reason.
fn assert_spy_is_wired(rs: &RuntimeControlState, spy: &Spy) {
    let before = spy.asserts.load(Ordering::SeqCst);
    {
        let _g = rs.ptt.keyed(None).expect("direct key must succeed");
    }
    assert_eq!(
        spy.asserts.load(Ordering::SeqCst),
        before + 1,
        "the spy is not wired to this SharedPtt — nothing below proves anything"
    );
    assert!(
        !spy.asserted.load(Ordering::SeqCst),
        "guard must release on drop"
    );
}

/// The non-OTA `SendMessage` fallback must key.
#[tokio::test]
async fn a_non_ota_send_message_keys_the_transmitter() {
    let (mut rs, spy) = state_with_spy();
    assert_spy_is_wired(&rs, &spy);

    let mut eng = engine();
    let (tx, _rx) = broadcast::channel::<ControlEvent>(32);
    let ev = Arc::new(tx);
    let mode: Arc<Mutex<String>> = Arc::new(Mutex::new("BPSK250".to_string()));

    let before_frames = eng.frames_transmitted();
    let before_asserts = spy.asserts.load(Ordering::SeqCst);

    apply_command_to_engine(
        &ControlCommand::SendMessage {
            to: "W1AW".into(),
            subject: "t".into(),
            body: "over the air".into(),
        },
        &mut eng,
        &mode,
        &ev,
        None,
        &mut rs,
    )
    .await;

    assert!(
        eng.frames_transmitted() > before_frames,
        "the fixture did not transmit, so the keying assertion below would be vacuous"
    );
    assert!(
        spy.asserts.load(Ordering::SeqCst) > before_asserts,
        "SendMessage transmitted without keying the transmitter"
    );
    assert_eq!(
        spy.asserts.load(Ordering::SeqCst),
        spy.releases.load(Ordering::SeqCst),
        "every key must be matched by a release"
    );
    assert!(
        !spy.asserted.load(Ordering::SeqCst),
        "PTT must be down afterwards"
    );
}

/// A wrapper is only a guarantee if a bare call cannot silently return.
///
/// Scans **every** `src/*.rs` — #1262 exists because the previous fix was applied per file — and
/// truncates each at its first `#[cfg(test)]`, since test modules legitimately transmit bare. Both
/// the truncation and the pattern are validated against planted inputs, so a scan that matched
/// nothing would fail rather than pass.
#[test]
fn every_daemon_transmit_is_keyed() {
    let files: Vec<(&str, &str)> = vec![
        ("lib.rs", include_str!("../src/lib.rs")),
        ("server.rs", include_str!("../src/server.rs")),
        ("filexfer.rs", include_str!("../src/filexfer.rs")),
        ("monitor.rs", include_str!("../src/monitor.rs")),
        ("twin.rs", include_str!("../src/twin.rs")),
        ("ws.rs", include_str!("../src/ws.rs")),
        ("ptt.rs", include_str!("../src/ptt.rs")),
        ("audit.rs", include_str!("../src/audit.rs")),
        ("logbook.rs", include_str!("../src/logbook.rs")),
        ("protocol.rs", include_str!("../src/protocol.rs")),
    ];

    // Control 1: the pattern detects a bare call.
    let planted = "\nfn planted() { engine.transmit(&payload, &mode, None); }\n";
    assert_eq!(
        bare_transmits(&format!("{}{planted}", production_prefix(files[0].1))).len(),
        bare_transmits(production_prefix(files[0].1)).len() + 1,
        "the scan does not detect a planted bare transmit — it proves nothing"
    );
    // Control 2: truncation has not eaten the production code (the helper must still be in scope).
    assert!(
        production_prefix(files[0].1).contains("fn keyed_transmit"),
        "truncation at #[cfg(test)] removed production code — the scan is scoped wrongly"
    );

    let mut bare = Vec::new();
    for (name, src) in &files {
        for line in bare_transmits(production_prefix(src)) {
            bare.push(format!("  {name}{line}"));
        }
    }
    assert!(
        bare.is_empty(),
        "these transmit calls are outside `keyed_transmit`, so they emit RF without keying (#1262):\n{}",
        bare.join("\n")
    );
}

/// Everything before the first `#[cfg(test)]`. Test modules transmit bare on purpose.
fn production_prefix(src: &str) -> &str {
    match src.find("#[cfg(test)]") {
        Some(i) => &src[..i],
        None => src,
    }
}

/// Transmit call sites not inside a `keyed_transmit( … )` span.
///
/// Blanks each helper call by balanced parens rather than tracking brace depth: a brace-depth
/// version produced a false positive in #1250 the moment `cargo fmt` collapsed a call onto one line.
fn bare_transmits(src: &str) -> Vec<String> {
    let blanked = blank_spans(src, "keyed_transmit(");
    blanked
        .lines()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim_start();
            !t.starts_with("//")
                && !l.contains("fn keyed_transmit")
                && (l.contains("engine.transmit")
                    || l.contains(".transmit_ota_ack(")
                    || l.contains(".transmit_raw_audio(")
                    || l.contains(".transmit_with_fec_mode("))
        })
        .map(|(i, l)| format!(":{}: {}", i + 1, l.trim()))
        .collect()
}

fn blank_spans(src: &str, needle: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = chars.clone();
    let n: Vec<char> = needle.chars().collect();
    let mut i = 0;
    while i + n.len() <= chars.len() {
        if chars[i..i + n.len()] == n[..] {
            let mut depth = 0i32;
            let mut j = i + n.len() - 1;
            while j < chars.len() {
                match chars[j] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                if chars[j] != '\n' {
                    out[j] = ' ';
                }
                j += 1;
            }
            i = j.max(i + n.len());
        } else {
            i += 1;
        }
    }
    out.into_iter().collect()
}
