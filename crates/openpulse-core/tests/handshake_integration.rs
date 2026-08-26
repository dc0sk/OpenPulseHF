//! Handshake verification, on the #1147 binary wire format.
//!
//! **Every tamper test here mutates BYTES.** The v1 versions mutated struct fields and re-verified,
//! which is vacuous under a sign-the-transmitted-prefix format: re-encoding recomputes the very span
//! the test is trying to corrupt, so the assertion passes whatever the signature covers.
//!
//! **The FEC/compression negotiation tests are GONE, not unported** (#1166). `supported_fec_modes` /
//! `selected_fec_mode` were deleted from the wire because nothing consumed the selection — the
//! daemon sent them empty and hardcoded `None`. The property those tests asserted (a responder
//! cannot select a mode the initiator never offered) dissolves with the field rather than becoming
//! untested. Deleted: `conreq_carries_fec_modes_in_signature`, `negotiate_strongest_mutual_fec_mode`,
//! `fec_no_overlap_falls_back_to_none`, `conack_rejected_when_fec_mode_not_offered`,
//! `short_rs_negotiates_at_highest_strength`. If FEC negotiation is ever wired for real, the
//! membership check must return with it.
//!
//! The signing-mode equivalent of that check is NEW here and does exist — see
//! `conack_rejected_when_mode_not_offered` (F-1147-05).

use ed25519_dalek::SigningKey;
use openpulse_core::handshake::{
    conreq_hash, verify_conack, verify_conreq, ConAck, ConAckParams, ConReq, ConReqParams,
    HandshakeError, InMemoryTrustStore,
};
use openpulse_core::handshake_wire::FRAGMENT_CAPACITY;
use openpulse_core::trust::{PolicyProfile, SigningMode};

fn make_seed(b: u8) -> [u8; 32] {
    [b; 32]
}

fn pubkey_for(seed: u8) -> [u8; 32] {
    SigningKey::from_bytes(&make_seed(seed))
        .verifying_key()
        .to_bytes()
}

const TS: u64 = 1_700_000_000_000;

fn conreq(station: &str, dst: &str, seed: u8, modes: Vec<SigningMode>) -> Vec<u8> {
    ConReq::create(
        &ConReqParams {
            station_id: station,
            dst_station: dst,
            signing_modes: modes,
            session_id: 0x5E55_1000_0000_0001,
            station_grid: "FN31pr",
            profile_name: "hpx_hf",
            profile_fingerprint: 99,
            timestamp_ms: TS,
            kex_pubkey: &[5u8; 32],
        },
        &make_seed(seed),
    )
    .unwrap()
}

fn conack(station: &str, seed: u8, mode: SigningMode, req: &[u8]) -> Vec<u8> {
    ConAck::create(
        &ConAckParams {
            station_id: station,
            selected_mode: mode,
            conreq_hash: conreq_hash(req),
            station_grid: "EM69",
            profile_name: "hpx_hf",
            profile_fingerprint: 99,
            timestamp_ms: TS + 100,
            kex_pubkey: &[6u8; 32],
        },
        &make_seed(seed),
    )
    .unwrap()
}

// ------------------------------------------------------------------
// ConReq verification
// ------------------------------------------------------------------

#[test]
fn valid_conreq_accepted_trusted_peer() {
    let req = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    let mut store = InMemoryTrustStore::new();
    store.add_trusted("W1AW", pubkey_for(1));

    let (decoded, decision) = verify_conreq(
        &req,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None,
    )
    .expect("should accept trusted peer");
    assert_eq!(decision.selected_mode, SigningMode::Normal);
    assert_eq!(decoded.station_id, "W1AW");
    assert_eq!(decoded.dst_station, "K2XYZ");
}

#[test]
fn valid_conreq_accepted_unknown_peer_permissive() {
    let req = conreq("W1AW", "*", 1, vec![SigningMode::Normal]);
    let store = InMemoryTrustStore::new();
    assert!(verify_conreq(
        &req,
        &store,
        PolicyProfile::Permissive,
        SigningMode::Normal,
        None
    )
    .is_ok());
}

/// BYTE tamper: flipping any byte of the signed prefix must break verification.
// VERIFIES: REQ-FUN-10
#[test]
fn conreq_rejected_invalid_signature() {
    let req = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    let mut store = InMemoryTrustStore::new();
    store.add_trusted("W1AW", pubkey_for(1));
    assert!(
        verify_conreq(
            &req,
            &store,
            PolicyProfile::Balanced,
            SigningMode::Normal,
            None
        )
        .is_ok(),
        "control: the untampered frame must verify"
    );

    let mut bad = req.clone();
    let n = bad.len();
    bad[n - 70] ^= 0xFF; // inside the body, before the 64-byte signature
    assert!(matches!(
        verify_conreq(
            &bad,
            &store,
            PolicyProfile::Balanced,
            SigningMode::Normal,
            None
        ),
        Err(HandshakeError::InvalidSignature)
    ));
}

// VERIFIES: REQ-FUN-12
#[test]
fn conreq_rejected_revoked_key() {
    let req = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    let mut store = InMemoryTrustStore::new();
    store.add_revoked("W1AW", pubkey_for(1));
    assert!(verify_conreq(
        &req,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None
    )
    .is_err());
}

#[test]
fn conreq_rejected_no_mutual_mode_strict() {
    let req = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Relaxed]);
    let mut store = InMemoryTrustStore::new();
    store.add_trusted("W1AW", pubkey_for(1));
    assert!(verify_conreq(
        &req,
        &store,
        PolicyProfile::Strict,
        SigningMode::Paranoid,
        None
    )
    .is_err());
}

/// A CONREQ addressed to another station still VERIFIES — addressing is a routing decision for the
/// daemon, not a cryptographic one. Pinned so the two concerns are not conflated: the RF-saving
/// refusal lives in the daemon (`a_conreq_addressed_elsewhere_does_not_key_the_transmitter`).
#[test]
fn addressing_is_not_a_verification_failure() {
    let req = conreq("W1AW", "DL9ZZZ", 1, vec![SigningMode::Normal]);
    let mut store = InMemoryTrustStore::new();
    store.add_trusted("W1AW", pubkey_for(1));
    let (decoded, _) = verify_conreq(
        &req,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None,
    )
    .expect("a well-signed CONREQ verifies regardless of who it is addressed to");
    assert!(!decoded.is_addressed_to("K2XYZ"));
    assert!(decoded.is_addressed_to("DL9ZZZ"));
}

// ------------------------------------------------------------------
// ConAck verification
// ------------------------------------------------------------------

#[test]
fn valid_conack_accepted() {
    let req = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    let ack = conack("K2XYZ", 2, SigningMode::Normal, &req);
    let mut store = InMemoryTrustStore::new();
    store.add_trusted("K2XYZ", pubkey_for(2));

    let (decoded, decision) = verify_conack(
        &ack,
        &req,
        &[SigningMode::Normal],
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None,
    )
    .expect("should accept");
    assert_eq!(decision.selected_mode, SigningMode::Normal);
    assert_eq!(decoded.conreq_hash, conreq_hash(&req));
}

/// Replaces `conack_rejected_session_id_mismatch`: v2 binds by hash over the transmitted CONREQ.
#[test]
fn conack_rejected_when_bound_to_a_different_conreq() {
    let ours = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    let theirs = conreq("W1AW", "DL9ZZZ", 1, vec![SigningMode::Normal]);
    assert_ne!(conreq_hash(&ours), conreq_hash(&theirs));

    let ack = conack("K2XYZ", 2, SigningMode::Normal, &theirs);
    let mut store = InMemoryTrustStore::new();
    store.add_trusted("K2XYZ", pubkey_for(2));
    assert!(matches!(
        verify_conack(
            &ack,
            &ours,
            &[SigningMode::Normal],
            &store,
            PolicyProfile::Balanced,
            SigningMode::Normal,
            None
        ),
        Err(HandshakeError::SessionIdMismatch { .. })
    ));
}

/// F-1147-05, the check v1 did not have: `selected_mode` must be one the CONREQ offered. v1
/// evaluated it against LOCAL policy only, so a responder could select a mode never proposed.
#[test]
fn conack_rejected_when_mode_not_offered() {
    let req = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    let ack = conack("K2XYZ", 2, SigningMode::Paranoid, &req);
    let mut store = InMemoryTrustStore::new();
    store.add_trusted("K2XYZ", pubkey_for(2));
    assert!(matches!(
        verify_conack(
            &ack,
            &req,
            &[SigningMode::Normal],
            &store,
            PolicyProfile::Balanced,
            SigningMode::Normal,
            None
        ),
        Err(HandshakeError::UnofferedSigningMode)
    ));
}

#[test]
fn conack_rejected_invalid_signature() {
    let req = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    let ack = conack("K2XYZ", 2, SigningMode::Normal, &req);
    let mut store = InMemoryTrustStore::new();
    store.add_trusted("K2XYZ", pubkey_for(2));

    let mut bad = ack.clone();
    let n = bad.len();
    bad[n - 70] ^= 0xFF;
    assert!(matches!(
        verify_conack(
            &bad,
            &req,
            &[SigningMode::Normal],
            &store,
            PolicyProfile::Balanced,
            SigningMode::Normal,
            None
        ),
        Err(HandshakeError::InvalidSignature)
    ));
}

// ------------------------------------------------------------------
// Wire format
// ------------------------------------------------------------------

#[test]
fn full_handshake_round_trip() {
    let req = conreq(
        "W1AW",
        "K2XYZ",
        1,
        vec![SigningMode::Normal, SigningMode::Psk],
    );
    let ack = conack("K2XYZ", 2, SigningMode::Psk, &req);

    let mut store = InMemoryTrustStore::new();
    store.add_trusted("W1AW", pubkey_for(1));
    store.add_trusted("K2XYZ", pubkey_for(2));

    let (r, _) = verify_conreq(
        &req,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None,
    )
    .unwrap();
    let (a, _) = verify_conack(
        &ack,
        &req,
        &r.signing_modes,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None,
    )
    .unwrap();
    assert_eq!(a.selected_mode, SigningMode::Psk);
    assert!(r.signing_modes.contains(&a.selected_mode));
}

#[test]
fn conreq_encode_decode_round_trip() {
    let req = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    let d = ConReq::decode(&req).unwrap();
    assert_eq!(d.station_id, "W1AW");
    assert_eq!(d.session_id, 0x5E55_1000_0000_0001);
    assert_eq!(d.station_grid, "FN31pr");
    assert_eq!(d.profile_name, "hpx_hf");
    assert_eq!(d.profile_fingerprint, 99);
    assert_eq!(d.timestamp_ms, TS);
    assert_eq!(d.kex_pubkey, vec![5u8; 32]);
}

#[test]
fn conack_encode_decode_round_trip() {
    let req = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    let d = ConAck::decode(&conack("K2XYZ", 2, SigningMode::Normal, &req)).unwrap();
    assert_eq!(d.station_id, "K2XYZ");
    assert_eq!(d.station_grid, "EM69");
    assert_eq!(d.timestamp_ms, TS + 100);
}

#[test]
fn conreq_decode_rejects_wrong_magic() {
    let mut req = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    req[0] = b'X';
    assert!(ConReq::decode(&req).is_err());
}

/// The headline property of #1147: both frames fit ONE SAR fragment, so a handshake costs one
/// acquisition rather than three. A v1 CONREQ was 752 B on the wire.
#[test]
fn both_handshake_frames_fit_one_sar_fragment() {
    let req = conreq(
        "W1AW",
        "K2XYZ",
        1,
        vec![SigningMode::Normal, SigningMode::Psk],
    );
    let ack = conack("K2XYZ", 2, SigningMode::Psk, &req);
    assert!(
        req.len() <= FRAGMENT_CAPACITY,
        "CONREQ is {} B, over one fragment",
        req.len()
    );
    assert!(
        ack.len() <= FRAGMENT_CAPACITY,
        "CONACK is {} B, over one fragment",
        ack.len()
    );
}

/// THE REGRESSION THIS FILE OWES #1147's FOLLOW-UP.
///
/// A cap must be justified against the GENERATOR, not against an example. `session_id` was capped at
/// 24 bytes, sized from a 6-character callsign, while `station_id` allows 12 — and the daemon built
/// the id as `"{callsign}-{unix_ms}"`. An 11-character compound callsign (`3DA0/DL1ABC`, entirely
/// legal) produced 25 bytes, `ConReq::create` failed, and the daemon logged a warning and carried on
/// with NO signed handshake: a silent downgrade to an unverified session, with no `CommandError` and
/// nothing user-visible.
///
/// `session_id` is now a fixed `u64`, so there is no cap to overflow. This test pins the property
/// that made the old design fragile — that a maximal-length callsign still produces a legal frame —
/// so a future change reintroducing a variable-length id fails here rather than on the air.
#[test]
fn a_maximal_length_callsign_still_produces_a_legal_frame() {
    let max_call = "A".repeat(12); // the station_id cap
    let f = ConReq::create(
        &ConReqParams {
            station_id: &max_call,
            dst_station: &max_call,
            signing_modes: vec![
                SigningMode::Normal,
                SigningMode::Psk,
                SigningMode::Relaxed,
                SigningMode::Paranoid,
            ],
            session_id: u64::MAX,
            station_grid: "JO62qm99",
            profile_name: &"p".repeat(24),
            profile_fingerprint: u64::MAX,
            timestamp_ms: u64::MAX,
            kex_pubkey: &[9u8; 32],
        },
        &make_seed(1),
    )
    .expect("a station at every cap must still be able to handshake");
    assert!(
        f.len() <= FRAGMENT_CAPACITY,
        "the maximal frame is {} B, over one fragment",
        f.len()
    );
}

/// F2: an unknown mode in the OFFER list is skipped, not fatal.
///
/// `SigningMode::from_wire`'s own doc says an unknown discriminant is "a negotiation outcome, not a
/// parse error to guess at" — and `decode` did the opposite, which made the one negotiable enum in
/// the format un-extendable. The day mode `0x07` exists, a station offering `[0x07, Normal]` would
/// have been unreadable by every deployed v2 peer: a de-facto wire break, inside the format whose
/// stated purpose is to be finished.
///
/// The frame is hand-built because no current encoder can emit an unknown mode — which is precisely
/// why this needed fixing inside the break window rather than when 0x07 arrives.
#[test]
fn an_unknown_offered_signing_mode_is_skipped_not_fatal() {
    let base = conreq(
        "W1AW",
        "K2XYZ",
        1,
        vec![SigningMode::Normal, SigningMode::Psk],
    );
    // Rewrite the second offered discriminant (0x02 = Psk) to an unassigned 0x7F, in place, so the
    // body length is unchanged and only that byte differs.
    let idx = base
        .windows(3)
        .position(|w| w == [0x02, 0x01, 0x02])
        .expect("the modes run [count=2, Normal, Psk] must be locatable")
        + 2;
    let mut future = base.clone();
    future[idx] = 0x7F;

    let decoded = ConReq::decode(&future).expect("an unknown OFFERED mode must not be fatal");
    assert_eq!(
        decoded.signing_modes,
        vec![SigningMode::Normal],
        "the known mode must survive and the unknown one must be dropped from the struct view"
    );
    // The unknown byte is still inside the signed body — only the struct view drops it — so the
    // signature over the ORIGINAL bytes no longer matches this mutated frame. That is correct: the
    // tolerance is about parsing, not about accepting unsigned content.
    let mut store = InMemoryTrustStore::new();
    store.add_trusted("W1AW", pubkey_for(1));
    assert!(matches!(
        verify_conreq(
            &future,
            &store,
            PolicyProfile::Balanced,
            SigningMode::Normal,
            None
        ),
        Err(HandshakeError::InvalidSignature)
    ));
}

/// And a frame offering ONLY unknown modes parses, then fails negotiation — the honest outcome.
#[test]
fn a_frame_offering_only_unknown_modes_parses_then_fails_negotiation() {
    let base = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    let idx = base
        .windows(2)
        .position(|w| w == [0x01, 0x01])
        .expect("the modes run [count=1, Normal] must be locatable")
        + 1;
    let mut future = base.clone();
    future[idx] = 0x7F;
    let decoded = ConReq::decode(&future).expect("must parse");
    assert!(
        decoded.signing_modes.is_empty(),
        "no known mode survives, so negotiation has nothing to select"
    );
}

/// F5: an empty `station_id` is refused at both ends.
///
/// It would otherwise verify under a permissive policy — there is no stored key to bind against —
/// and be recorded as a verified peer with an empty callsign: an identity that cannot be revoked,
/// looked up, or usefully logged. `dst_station` was already refused for the sibling reason.
#[test]
fn an_empty_station_id_is_refused_at_both_ends() {
    let e = ConReq::create(
        &ConReqParams {
            station_id: "",
            dst_station: "K2XYZ",
            signing_modes: vec![SigningMode::Normal],
            session_id: 1,
            station_grid: "",
            profile_name: "",
            profile_fingerprint: 0,
            timestamp_ms: TS,
            kex_pubkey: &[5u8; 32],
        },
        &make_seed(1),
    )
    .unwrap_err()
    .to_string();
    assert!(
        e.contains("station_id"),
        "expected a station_id refusal, got: {e}"
    );

    // And a hand-built frame with an empty station_id cannot be decoded either, so the sender-side
    // check is not the only thing standing between the wire and an unnamed verified peer.
    let good = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    let mut hand = good.clone();
    hand[7] = 0; // the station_id length prefix, first byte of the body
    assert!(ConReq::decode(&hand).is_err());
}

#[test]
fn conreq_advertises_profile_and_survives_wire_roundtrip() {
    let req = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    let mut store = InMemoryTrustStore::new();
    store.add_trusted("W1AW", pubkey_for(1));
    let (d, _) = verify_conreq(
        &req,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None,
    )
    .unwrap();
    assert_eq!(d.profile_name, "hpx_hf");
    assert_eq!(d.profile_fingerprint, 99);
}
