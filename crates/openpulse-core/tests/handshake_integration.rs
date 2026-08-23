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
            session_id: "sess-001",
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

fn conack(station: &str, dst: &str, seed: u8, mode: SigningMode, req: &[u8]) -> Vec<u8> {
    ConAck::create(
        &ConAckParams {
            station_id: station,
            dst_station: dst,
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
    let ack = conack("K2XYZ", "W1AW", 2, SigningMode::Normal, &req);
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

    let ack = conack("K2XYZ", "W1AW", 2, SigningMode::Normal, &theirs);
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
    let ack = conack("K2XYZ", "W1AW", 2, SigningMode::Paranoid, &req);
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
    let ack = conack("K2XYZ", "W1AW", 2, SigningMode::Normal, &req);
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
    let ack = conack("K2XYZ", "W1AW", 2, SigningMode::Psk, &req);

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
    assert_eq!(d.session_id, "sess-001");
    assert_eq!(d.station_grid, "FN31pr");
    assert_eq!(d.profile_name, "hpx_hf");
    assert_eq!(d.profile_fingerprint, 99);
    assert_eq!(d.timestamp_ms, TS);
    assert_eq!(d.kex_pubkey, vec![5u8; 32]);
}

#[test]
fn conack_encode_decode_round_trip() {
    let req = conreq("W1AW", "K2XYZ", 1, vec![SigningMode::Normal]);
    let d = ConAck::decode(&conack("K2XYZ", "W1AW", 2, SigningMode::Normal, &req)).unwrap();
    assert_eq!(d.station_id, "K2XYZ");
    assert_eq!(d.dst_station, "W1AW");
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
    let ack = conack("K2XYZ", "W1AW", 2, SigningMode::Psk, &req);
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
