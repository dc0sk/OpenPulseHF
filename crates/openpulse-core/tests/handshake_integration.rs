use ed25519_dalek::SigningKey;
use openpulse_core::compression::CompressionAlgorithm;
use openpulse_core::fec::FecMode;
use openpulse_core::handshake::{
    verify_conack, verify_conreq, ConAck, ConReq, HandshakeError, InMemoryTrustStore,
};
use openpulse_core::trust::{PolicyProfile, SigningMode};

fn make_seed(b: u8) -> [u8; 32] {
    [b; 32]
}

fn pubkey_for(seed: u8) -> [u8; 32] {
    SigningKey::from_bytes(&make_seed(seed))
        .verifying_key()
        .to_bytes()
}

// ------------------------------------------------------------------
// ConReq verification
// ------------------------------------------------------------------

#[test]
fn valid_conreq_accepted_trusted_peer() {
    let req = ConReq::create(
        "W1AW",
        &make_seed(1),
        vec![SigningMode::Normal],
        "sess-001",
        vec![],
        vec![],
    )
    .unwrap();

    let mut store = InMemoryTrustStore::new();
    store.add_trusted("W1AW", pubkey_for(1));

    let decision = verify_conreq(&req, &store, PolicyProfile::Balanced, SigningMode::Normal)
        .expect("should accept trusted peer");
    assert_eq!(decision.selected_mode, SigningMode::Normal);
}

#[test]
fn valid_conreq_accepted_unknown_peer_permissive() {
    let req = ConReq::create(
        "N0CALL",
        &make_seed(2),
        vec![SigningMode::Normal, SigningMode::Relaxed],
        "sess-002",
        vec![],
        vec![],
    )
    .unwrap();

    let store = InMemoryTrustStore::new(); // peer not in store → Unknown

    let decision = verify_conreq(
        &req,
        &store,
        PolicyProfile::Permissive,
        SigningMode::Relaxed,
    )
    .expect("permissive policy allows unknown key");
    assert_eq!(decision.selected_mode, SigningMode::Normal);
}

#[test]
fn conreq_rejected_invalid_signature() {
    let mut req = ConReq::create(
        "W1AW",
        &make_seed(1),
        vec![SigningMode::Normal],
        "sess-003",
        vec![],
        vec![],
    )
    .unwrap();
    req.signature[0] ^= 0xff; // corrupt

    let store = InMemoryTrustStore::new();
    let result = verify_conreq(&req, &store, PolicyProfile::Balanced, SigningMode::Normal);
    assert!(
        matches!(result, Err(HandshakeError::InvalidSignature)),
        "corrupted signature must be rejected"
    );
}

#[test]
fn conreq_rejected_revoked_key() {
    let req = ConReq::create(
        "W1AW",
        &make_seed(1),
        vec![SigningMode::Normal],
        "sess-004",
        vec![],
        vec![],
    )
    .unwrap();

    let mut store = InMemoryTrustStore::new();
    store.add_revoked("W1AW", pubkey_for(1));

    let result = verify_conreq(&req, &store, PolicyProfile::Balanced, SigningMode::Normal);
    assert!(
        matches!(result, Err(HandshakeError::TrustFailure(_))),
        "revoked key must be rejected"
    );
}

#[test]
fn conreq_rejected_no_mutual_mode_strict() {
    let req = ConReq::create(
        "W1AW",
        &make_seed(1),
        vec![SigningMode::Relaxed], // only offers Relaxed
        "sess-005",
        vec![],
        vec![],
    )
    .unwrap();

    let store = InMemoryTrustStore::new();
    // Strict policy only accepts Normal/Paranoid; Relaxed not allowed
    let result = verify_conreq(&req, &store, PolicyProfile::Strict, SigningMode::Normal);
    assert!(
        matches!(result, Err(HandshakeError::TrustFailure(_))),
        "strict policy must reject Relaxed-only peer"
    );
}

// ------------------------------------------------------------------
// ConAck verification
// ------------------------------------------------------------------

#[test]
fn valid_conack_accepted() {
    let ack = ConAck::create(
        "KD9XYZ",
        &make_seed(2),
        SigningMode::Normal,
        "sess-010",
        CompressionAlgorithm::None,
        FecMode::None,
    )
    .unwrap();

    let mut store = InMemoryTrustStore::new();
    store.add_trusted("KD9XYZ", pubkey_for(2));

    let decision = verify_conack(
        &ack,
        "sess-010",
        &[],
        &[],
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    )
    .expect("valid CONACK should be accepted");
    assert_eq!(decision.selected_mode, SigningMode::Normal);
}

#[test]
fn conack_rejected_session_id_mismatch() {
    let ack = ConAck::create(
        "KD9XYZ",
        &make_seed(2),
        SigningMode::Normal,
        "sess-010",
        CompressionAlgorithm::None,
        FecMode::None,
    )
    .unwrap();
    let store = InMemoryTrustStore::new();

    let result = verify_conack(
        &ack,
        "sess-WRONG",
        &[],
        &[],
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    );
    assert!(
        matches!(result, Err(HandshakeError::SessionIdMismatch { .. })),
        "mismatched session ID must be rejected"
    );
}

#[test]
fn conack_rejected_invalid_signature() {
    let mut ack = ConAck::create(
        "KD9XYZ",
        &make_seed(2),
        SigningMode::Normal,
        "sess-010",
        CompressionAlgorithm::None,
        FecMode::None,
    )
    .unwrap();
    ack.signature[63] ^= 0x01; // corrupt last byte

    let store = InMemoryTrustStore::new();
    let result = verify_conack(
        &ack,
        "sess-010",
        &[],
        &[],
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    );
    assert!(matches!(result, Err(HandshakeError::InvalidSignature)));
}

// ------------------------------------------------------------------
// End-to-end handshake round-trip
// ------------------------------------------------------------------

#[test]
fn full_handshake_round_trip() {
    // Alice initiates
    let req = ConReq::create(
        "W1AW",
        &make_seed(1),
        vec![SigningMode::Normal, SigningMode::Paranoid],
        "sess-100",
        vec![],
        vec![],
    )
    .unwrap();

    // Bob verifies CONREQ and responds
    let mut bob_store = InMemoryTrustStore::new();
    bob_store.add_trusted("W1AW", pubkey_for(1));

    let req_decision = verify_conreq(&req, &bob_store, PolicyProfile::Strict, SigningMode::Normal)
        .expect("Bob should accept Alice's CONREQ");

    let ack = ConAck::create(
        "KD9XYZ",
        &make_seed(2),
        req_decision.selected_mode,
        &req.session_id,
        CompressionAlgorithm::None,
        FecMode::None,
    )
    .unwrap();

    // Alice verifies CONACK
    let mut alice_store = InMemoryTrustStore::new();
    alice_store.add_trusted("KD9XYZ", pubkey_for(2));

    let ack_decision = verify_conack(
        &ack,
        &req.session_id,
        &req.supported_compression,
        &req.supported_fec_modes,
        &alice_store,
        PolicyProfile::Strict,
        SigningMode::Normal,
    )
    .expect("Alice should accept Bob's CONACK");

    assert_eq!(req_decision.selected_mode, ack_decision.selected_mode);
}

// ------------------------------------------------------------------
// Wire encode/decode
// ------------------------------------------------------------------

#[test]
fn conreq_encode_decode_round_trip() {
    let req = ConReq::create(
        "W1AW",
        &make_seed(1),
        vec![SigningMode::Normal],
        "s1",
        vec![],
        vec![],
    )
    .unwrap();
    let bytes = req.encode().expect("encode");
    let decoded = ConReq::decode(&bytes).expect("decode");
    assert_eq!(decoded.station_id, req.station_id);
    assert_eq!(decoded.session_id, req.session_id);
    assert_eq!(decoded.signature, req.signature);
}

#[test]
fn conack_encode_decode_round_trip() {
    let ack = ConAck::create(
        "KD9XYZ",
        &make_seed(2),
        SigningMode::Normal,
        "s1",
        CompressionAlgorithm::None,
        FecMode::None,
    )
    .unwrap();
    let bytes = ack.encode().expect("encode");
    let decoded = ConAck::decode(&bytes).expect("decode");
    assert_eq!(decoded.station_id, ack.station_id);
    assert_eq!(decoded.selected_mode, ack.selected_mode);
    assert_eq!(decoded.signature, ack.signature);
}

#[test]
fn conreq_decode_rejects_wrong_magic() {
    let req = ConReq::create(
        "W1AW",
        &make_seed(1),
        vec![SigningMode::Normal],
        "s1",
        vec![],
        vec![],
    )
    .unwrap();
    let mut bytes = req.encode().expect("encode");
    bytes[0] = b'X'; // corrupt magic
    assert!(ConReq::decode(&bytes).is_err());
}

// ------------------------------------------------------------------
// FecMode negotiation
// ------------------------------------------------------------------

#[test]
fn conreq_carries_fec_modes_in_signature() {
    let req = ConReq::create(
        "W1AW",
        &make_seed(20),
        vec![SigningMode::Normal],
        "sess-fec-1",
        vec![],
        vec![FecMode::Rs, FecMode::Concatenated],
    )
    .unwrap();

    assert_eq!(
        req.supported_fec_modes,
        vec![FecMode::Rs, FecMode::Concatenated]
    );

    let store = InMemoryTrustStore::new();
    verify_conreq(&req, &store, PolicyProfile::Balanced, SigningMode::Normal)
        .expect("ConReq with FEC modes should verify");
}

#[test]
fn negotiate_strongest_mutual_fec_mode() {
    // Alice offers [Rs, Concatenated]; Bob accepts [RsInterleaved, Concatenated].
    // Strongest mutual mode is Concatenated (strength 3).
    let offered = [FecMode::Rs, FecMode::Concatenated];
    let accepted = [FecMode::RsInterleaved, FecMode::Concatenated];
    assert_eq!(
        FecMode::negotiate(&offered, &accepted),
        FecMode::Concatenated
    );
}

#[test]
fn fec_no_overlap_falls_back_to_none() {
    // Alice offers [Rs]; Bob accepts [RsInterleaved] — no overlap.
    let offered = [FecMode::Rs];
    let accepted = [FecMode::RsInterleaved];
    assert_eq!(FecMode::negotiate(&offered, &accepted), FecMode::None);
}

#[test]
fn conack_rejected_when_fec_mode_not_offered() {
    // Initiator advertises no FEC; responder selects Concatenated anyway.
    let ack = ConAck::create(
        "KD9XYZ",
        &make_seed(30),
        SigningMode::Normal,
        "sess-fec-rej",
        CompressionAlgorithm::None,
        FecMode::Concatenated,
    )
    .unwrap();

    let store = InMemoryTrustStore::new();
    let result = verify_conack(
        &ack,
        "sess-fec-rej",
        &[],
        &[], // initiator offered no FEC modes
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    );
    assert!(
        matches!(result, Err(HandshakeError::UnsupportedFecMode)),
        "selecting an unoffered FEC mode must be rejected"
    );
}

#[test]
fn short_rs_negotiates_at_highest_strength() {
    // Both peers offer [Rs, ShortRs]; negotiate() must select ShortRs (strength 4).
    let offered = [FecMode::Rs, FecMode::ShortRs];
    let accepted = [FecMode::Rs, FecMode::ShortRs, FecMode::RsInterleaved];
    let selected = FecMode::negotiate(&offered, &accepted);
    assert_eq!(
        selected,
        FecMode::ShortRs,
        "ShortRs (strength 4) should win negotiation over Rs (strength 1)"
    );
}
