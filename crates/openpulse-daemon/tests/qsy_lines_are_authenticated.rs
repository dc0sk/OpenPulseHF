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
use openpulse_core::frame::Frame;
use openpulse_daemon::protocol::ControlEvent;
use openpulse_daemon::{process_received_bytes, RuntimeControlState, VerifiedPeer};
use openpulse_modem::ModemEngine;
use openpulse_qsy::frame::{encode_signed, QsyFrame, MAX_QSY_LINE_BYTES};
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

/// `MAX_QSY_LINE_BYTES` is the ceiling `Frame` actually enforces, not a number typed twice.
///
/// The encoder refuses above this; `Frame::new` refuses above its own limit. If either moves without
/// the other, an over-long line would be accepted at encode and fail at transmit again — the exact
/// wedge #1252 removed. Asserted from both sides so a drift in either direction fails here.
#[test]
fn the_line_ceiling_is_the_frame_ceiling() {
    assert!(
        Frame::new(0, vec![0u8; MAX_QSY_LINE_BYTES]).is_ok(),
        "a line at MAX_QSY_LINE_BYTES must fit one Frame"
    );
    assert!(
        Frame::new(0, vec![0u8; MAX_QSY_LINE_BYTES + 1]).is_err(),
        "MAX_QSY_LINE_BYTES is below Frame's real limit — the encoder is refusing lines that would \
         have fit, and the measured candidate ceilings are wrong"
    );
}
