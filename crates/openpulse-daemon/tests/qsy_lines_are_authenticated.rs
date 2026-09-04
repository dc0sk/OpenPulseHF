//! QSY lines are authenticated to the link peer, and stale ones are refused (#1252).
//!
//! `openpulse-qsy` has shipped Ed25519 line signing since it was written — `sign_line`,
//! `verify_line`, `encode_signed`, `decode_signed` — and the daemon called none of it. It imported
//! `decode_unsigned`/`encode_unsigned`, so `verify_line` had zero callers outside the crate and the
//! QSY control plane was unauthenticated on air. Roughly a dozen documents said otherwise, including
//! a code review that "confirmed" it after citing four sites, all inside the implementing crate.
//!
//! **What this is worth, stated no more strongly than it is.** QSY lines are now authenticated to
//! the link peer established by the signed handshake, and subject to `allow_trustlevels`. An on-air
//! adversary can still jam the negotiation; they can no longer *steer* it, abort it, or move a
//! station under the peer's identity. Replay is bounded by the per-session token and the signed
//! timestamp. It does not make QSY "secure", and it does not touch jamming.
//!
//! The steering case is why this matters more than the abort case: the token travels in cleartext in
//! the REQ, so an attacker in earshot has it, and a forged `QSY_ACK` carrying it used to retune the
//! peer's rig to any bandplan-permitted frequency.

use ed25519_dalek::SigningKey;
use openpulse_audio::LoopbackBackend;
use openpulse_daemon::protocol::ControlEvent;
use openpulse_daemon::{process_received_bytes, RuntimeControlState, VerifiedPeer};
use openpulse_modem::ModemEngine;
use openpulse_qsy::frame::{encode_signed, QsyFrame};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

const PEER_SEED: [u8; 32] = [3u8; 32];
const STRANGER_SEED: [u8; 32] = [4u8; 32];

fn pubkey(seed: &[u8; 32]) -> Vec<u8> {
    SigningKey::from_bytes(seed)
        .verifying_key()
        .to_bytes()
        .to_vec()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn engine() -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(bpsk_plugin::BpskPlugin::new()))
        .expect("register BPSK");
    e
}

/// A station that has verified `PEER_SEED`'s owner through the handshake.
fn station() -> RuntimeControlState {
    let mut rs = RuntimeControlState {
        local_callsign: "W1AW".into(),
        ..RuntimeControlState::default()
    };
    rs.verified_peers.insert(
        "K1PEER".into(),
        VerifiedPeer {
            callsign: "K1PEER".into(),
            grid: String::new(),
            pubkey: pubkey(&PEER_SEED),
            profile_compatible: None,
        },
    );
    rs.last_verified_callsign = Some("K1PEER".into());
    rs
}

fn req(token: &str) -> QsyFrame {
    QsyFrame::Req {
        token: token.into(),
        n_candidates: 2,
    }
}

async fn feed(rs: &mut RuntimeControlState, line: &str, eng: &mut ModemEngine) -> u64 {
    let mode: Arc<Mutex<String>> = Arc::new(Mutex::new("BPSK250".into()));
    let (tx, _rx) = broadcast::channel::<ControlEvent>(32);
    let ev = Arc::new(tx);
    let before = eng.frames_transmitted();
    process_received_bytes(line.as_bytes(), rs, None, &ev, &mode, eng).await;
    eng.frames_transmitted() - before
}

/// A line signed by a station we have NOT verified opens nothing — and, critically, keys nothing.
///
/// Measured on the transmit counter because the cost of the old behaviour was spent RF: any REQ from
/// anyone drew a keyed reply. The positive control in the same test is the identical exchange signed
/// by the verified peer, which must both open a session and be allowed to reply.
#[tokio::test]
async fn a_line_from_an_unverified_station_opens_no_session_and_keys_nothing() {
    // VERIFIES: REQ-SEC-14
    // Positive control first: the real peer's line works, so a failure below is about the signer.
    let mut rs = station();
    let mut eng = engine();
    let good = encode_signed(&req("tok-good"), now_ms(), &PEER_SEED).expect("sign");
    feed(&mut rs, &good, &mut eng).await;
    assert!(
        rs.qsy_session.is_some(),
        "control: the verified peer's signed REQ must open a responder session"
    );

    // The stranger holds a valid key of their own — authenticated identity, wrong identity.
    let mut rs = station();
    let mut eng = engine();
    let forged = encode_signed(&req("tok-forged"), now_ms(), &STRANGER_SEED).expect("sign");
    let keyed = feed(&mut rs, &forged, &mut eng).await;
    assert!(
        rs.qsy_session.is_none(),
        "a station we never verified opened a QSY session"
    );
    assert_eq!(
        keyed, 0,
        "the daemon transmitted a reply to a QSY line it could not authenticate — spent RF (#1178)"
    );
    assert!(
        rs.qsy_lines_refused > 0,
        "the refusal tripwire did not fire, so this test cannot tell a refusal from a no-op"
    );
}

/// An unsigned line is refused outright. This is the behaviour that used to be the defect.
#[tokio::test]
async fn an_unsigned_line_is_refused() {
    // VERIFIES: REQ-SEC-14
    let mut rs = station();
    let mut eng = engine();
    // The signable payload, with no `|SIG:` trailer — exactly what the daemon used to accept.
    let unsigned = openpulse_qsy::frame::encode_unsigned(&req("tok-plain"), now_ms());
    let keyed = feed(&mut rs, &unsigned, &mut eng).await;
    assert!(rs.qsy_session.is_none(), "an unsigned REQ opened a session");
    assert_eq!(keyed, 0, "an unsigned REQ drew a keyed reply");
}

/// A captured transcript does not replay: the signature is valid forever, the timestamp is not.
///
/// Without the timestamp an attacker could replay a whole signed REQ/LIST/ACK exchange and retune
/// the victim to a stale frequency — the signature proves authorship, never recency.
#[tokio::test]
async fn a_stale_line_is_refused_even_with_a_valid_signature() {
    // VERIFIES: REQ-SEC-14
    let stale_ms = now_ms() - 121_000; // just past HANDSHAKE_MAX_SKEW_MS
    let mut rs = station();
    let mut eng = engine();
    let stale = encode_signed(&req("tok-stale"), stale_ms, &PEER_SEED).expect("sign");
    feed(&mut rs, &stale, &mut eng).await;
    assert!(
        rs.qsy_session.is_none(),
        "a correctly-signed but stale line opened a session — a captured transcript replays"
    );

    // Control: the identical frame, stamped now, is accepted. Without this the assertion above
    // would also pass if the line were being refused for some unrelated reason.
    let mut rs = station();
    let mut eng = engine();
    let fresh = encode_signed(&req("tok-stale"), now_ms(), &PEER_SEED).expect("sign");
    feed(&mut rs, &fresh, &mut eng).await;
    assert!(
        rs.qsy_session.is_some(),
        "control: the same frame stamped now must be accepted"
    );
}

/// The signature and timestamp must not push a legal line past one modem frame.
///
/// A QSY line is transmitted with no SAR, so it must fit `Frame`'s 255-byte payload. The signature is
/// 93 characters and the timestamp up to 20, and before #1252 an over-long candidate list failed at
/// *transmit* — after the REQ had already gone out — wedging the peer until its session TTL. Now the
/// encoder refuses, where the caller can see it.
///
/// The ceilings below are MEASURED, not estimated: with the daemon's 8-hex-character token
/// (`random_token`) a `QSY_LIST` holds **6** candidates; at the codec's 64-character token maximum it
/// holds **3**. `[qsy] candidate_freqs_hz` is unbounded in config, so this is the bound an operator
/// can actually exceed.
#[test]
fn a_maximal_line_fits_one_frame() {
    for (token_len, expected_max) in [(8usize, 6usize), (64, 3)] {
        let token = "T".repeat(token_len);
        let fits = |n: usize| {
            let f = QsyFrame::List {
                token: token.clone(),
                // Worst case per pair: an 8-digit frequency and a 7-character SNR.
                candidates: (0..n)
                    .map(|i| (14_000_000 + i as u64 * 1_000, -100.00f32))
                    .collect(),
            };
            encode_signed(&f, u64::MAX, &PEER_SEED).is_ok()
        };
        assert!(
            fits(expected_max),
            "a {expected_max}-candidate list with a {token_len}-char token must fit one frame"
        );
        assert!(
            !fits(expected_max + 1),
            "a {}-candidate list with a {token_len}-char token must be REFUSED at encode, not \
             discovered at transmit",
            expected_max + 1
        );
    }
}

/// An INITIATOR's negotiation is bound to the key that was the link peer when it opened.
///
/// Found by the pre-post review of this change, and it was a hole in the fix, not in the write-up.
/// The responder path pinned `qsy_peer_pubkey`; both initiator paths stored the session without it,
/// so the expected key fell back to `last_verified_peer()` on *every inbound line*. That slot is
/// displaceable: `handle_inbound_conreq` runs a permissive profile and answers any CONREQ addressed
/// to our public callsign, and `record_verified_peer` then overwrites the slot — with no guard for a
/// negotiation in flight. The token is cleartext in our own `QSY_REQ`, so the attack needed nothing
/// secret: hear the REQ, send a CONREQ, become the link peer, sign a `QSY_REJECT`.
///
/// The displacement is modelled here by writing the slot directly. That is faithful to what the
/// CONREQ does and keeps this test about the pin; the permissive-CONREQ behaviour itself is a
/// separate finding, not something this test should be able to pass or fail on.
#[tokio::test]
async fn a_stranger_cannot_abort_an_initiated_negotiation_by_displacing_the_link_peer() {
    // VERIFIES: REQ-SEC-14
    for stranger_displaces in [true, false] {
        let mut rs = station();
        let mut eng = engine();
        rs.qsy_candidate_freqs = vec![14_100_000, 14_102_000];
        rs.qsy_pending_token = Some("tok-init".into());

        let (tx, _rx) = broadcast::channel::<ControlEvent>(64);
        let ev_tx = Arc::new(tx);
        let mode: Arc<Mutex<String>> = Arc::new(Mutex::new("BPSK250".to_string()));
        openpulse_daemon::apply_command_to_engine(
            &openpulse_daemon::protocol::ControlCommand::AcceptQsy {
                token: "tok-init".into(),
            },
            &mut eng,
            &mode,
            &ev_tx,
            None,
            &mut rs,
        )
        .await;
        assert!(
            rs.qsy_session.is_some(),
            "precondition: AcceptQsy must open an initiator session"
        );
        assert!(
            !rs.qsy_session.as_ref().expect("session").is_terminal(),
            "precondition: a freshly initiated negotiation is not terminal"
        );

        // The attacker's CONREQ lands and takes the link-peer slot.
        if stranger_displaces {
            rs.verified_peers.insert(
                "K9BAD".into(),
                VerifiedPeer {
                    callsign: "K9BAD".into(),
                    grid: String::new(),
                    pubkey: pubkey(&STRANGER_SEED),
                    profile_compatible: None,
                },
            );
            rs.last_verified_callsign = Some("K9BAD".into());
        }

        let seed = if stranger_displaces {
            STRANGER_SEED
        } else {
            PEER_SEED
        };
        let reject = encode_signed(
            &QsyFrame::Reject {
                token: "tok-init".into(),
                reason: "QRM".into(),
            },
            now_ms(),
            &seed,
        )
        .expect("sign");
        feed(&mut rs, &reject, &mut eng).await;

        let terminal = rs
            .qsy_session
            .as_ref()
            .map(|s| s.is_terminal())
            .unwrap_or(true);
        if stranger_displaces {
            assert!(
                !terminal,
                "a stranger displaced the link peer and aborted our negotiation with a signed \
                 REJECT — the initiator path is not pinned to the key it opened with"
            );
            assert_eq!(
                rs.qsy_lines_refused, 1,
                "the stranger's line must be counted as refused, not silently dropped"
            );
        } else {
            // Positive control: the pinned peer's identical REJECT DOES end it. Without this the
            // assertion above would also pass against a build where REJECT stopped working at all.
            assert!(
                terminal,
                "control: the pinned peer's own signed REJECT must still end the negotiation"
            );
        }
    }
}
