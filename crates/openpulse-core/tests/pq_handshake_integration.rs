use openpulse_core::sar::{sar_encode, SarReassembler};
use openpulse_core::{
    create_pq_conack, create_pq_conreq, decode_pq_conreq, encode_pq_conreq,
    generate_ml_dsa_44_keypair, generate_ml_kem_768_keypair, kem_decapsulate, verify_pq_conack,
    verify_pq_conreq, InMemoryTrustStore, PolicyProfile, SigningMode, ML_DSA_44_PUBKEY_SIZE,
    ML_DSA_44_SIG_SIZE, ML_KEM_768_CT_SIZE, ML_KEM_768_DK_SIZE, ML_KEM_768_EK_SIZE,
    ML_KEM_768_SS_SIZE,
};
use std::time::Duration;

// ------------------------------------------------------------------
// Group 1: Key generation and KEM
// ------------------------------------------------------------------

#[test]
fn ml_dsa_44_keypair_sizes_are_correct() {
    let (sk, vk) = generate_ml_dsa_44_keypair();
    assert_eq!(sk.len(), 32, "ML-DSA-44 signing key seed must be 32 bytes");
    assert_eq!(
        vk.len(),
        ML_DSA_44_PUBKEY_SIZE,
        "ML-DSA-44 verifying key must be {ML_DSA_44_PUBKEY_SIZE} bytes"
    );
}

#[test]
fn ml_kem_768_keypair_sizes_are_correct() {
    let (dk, ek) = generate_ml_kem_768_keypair();
    assert_eq!(
        dk.len(),
        ML_KEM_768_DK_SIZE,
        "ML-KEM-768 DK seed must be {ML_KEM_768_DK_SIZE} bytes"
    );
    assert_eq!(
        ek.len(),
        ML_KEM_768_EK_SIZE,
        "ML-KEM-768 EK must be {ML_KEM_768_EK_SIZE} bytes"
    );
}

#[test]
fn kem_shared_secret_matches_after_encapsulate_decapsulate() {
    let (dk, ek) = generate_ml_kem_768_keypair();
    let classical_seed = [0u8; 32];
    let (pq_sk, _pq_vk) = generate_ml_dsa_44_keypair();

    let (ack, ss_responder) = create_pq_conack(
        "W1AW",
        &classical_seed,
        &pq_sk,
        &ek,
        SigningMode::Hybrid,
        "session-kem-test",
    )
    .expect("create_pq_conack");

    let ss_initiator = kem_decapsulate(&dk, &ack.kem_ciphertext).expect("kem_decapsulate");

    assert_eq!(
        ss_responder.len(),
        ML_KEM_768_SS_SIZE,
        "shared secret must be 32 bytes"
    );
    assert_eq!(
        ack.kem_ciphertext.len(),
        ML_KEM_768_CT_SIZE,
        "ciphertext must be {ML_KEM_768_CT_SIZE} bytes"
    );
    assert_eq!(ss_initiator, ss_responder, "shared secrets must match");
}

// ------------------------------------------------------------------
// Group 2: Hybrid handshake round-trips
// ------------------------------------------------------------------

fn make_trust_store(_station_id: &str) -> InMemoryTrustStore {
    InMemoryTrustStore::new()
}

fn make_trusted_store(station_id: &str, pubkey: [u8; 32]) -> InMemoryTrustStore {
    let mut store = InMemoryTrustStore::new();
    store.add_trusted(station_id, pubkey);
    store
}

#[test]
fn pq_conreq_hybrid_creates_and_verifies() {
    let classical_seed = [0x11u8; 32];
    let (pq_sk, _pq_vk) = generate_ml_dsa_44_keypair();
    let (_dk, kem_ek) = generate_ml_kem_768_keypair();

    let req = create_pq_conreq(
        "W1AW",
        &classical_seed,
        &pq_sk,
        &kem_ek,
        vec![SigningMode::Hybrid, SigningMode::Normal],
        "session-1",
    )
    .expect("create_pq_conreq");

    assert_eq!(req.pq_signature.len(), ML_DSA_44_SIG_SIZE);
    assert_eq!(req.classical_signature.len(), 64);

    let store = make_trust_store("W1AW");
    let decision = verify_pq_conreq(&req, &store, PolicyProfile::Balanced, SigningMode::Normal)
        .expect("verify_pq_conreq");
    assert_eq!(decision.selected_mode, SigningMode::Hybrid);
}

#[test]
fn pq_conack_hybrid_creates_verifies_and_decapsulates() {
    let initiator_classical = [0x22u8; 32];
    let responder_classical = [0x33u8; 32];
    let (initiator_pq_sk, _) = generate_ml_dsa_44_keypair();
    let (responder_pq_sk, _) = generate_ml_dsa_44_keypair();
    let (dk, kem_ek) = generate_ml_kem_768_keypair();

    let req = create_pq_conreq(
        "W1AW",
        &initiator_classical,
        &initiator_pq_sk,
        &kem_ek,
        vec![SigningMode::Hybrid],
        "session-2",
    )
    .expect("create_pq_conreq");

    let (ack, ss_resp) = create_pq_conack(
        "KD9XYZ",
        &responder_classical,
        &responder_pq_sk,
        &req.kem_pubkey,
        SigningMode::Hybrid,
        "session-2",
    )
    .expect("create_pq_conack");

    let store = make_trust_store("KD9XYZ");
    verify_pq_conack(
        &ack,
        "session-2",
        &req.signing_modes,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    )
    .expect("verify_pq_conack");

    let ss_init = kem_decapsulate(&dk, &ack.kem_ciphertext).expect("kem_decapsulate");
    assert_eq!(ss_init, ss_resp, "KEM shared secrets must match");
}

#[test]
fn pq_conreq_pq_only_mode() {
    let classical_seed = [0x44u8; 32];
    let (pq_sk, _) = generate_ml_dsa_44_keypair();
    let (_dk, kem_ek) = generate_ml_kem_768_keypair();

    let req = create_pq_conreq(
        "W1AW",
        &classical_seed,
        &pq_sk,
        &kem_ek,
        vec![SigningMode::Pq],
        "session-pq",
    )
    .expect("create_pq_conreq in Pq mode");

    assert!(
        req.classical_signature.is_empty(),
        "Pq-only mode must have empty classical_signature"
    );
    assert_eq!(req.pq_signature.len(), ML_DSA_44_SIG_SIZE);

    let store = make_trust_store("W1AW");
    let decision = verify_pq_conreq(&req, &store, PolicyProfile::Balanced, SigningMode::Pq)
        .expect("verify_pq_conreq Pq mode");
    assert_eq!(decision.selected_mode, SigningMode::Pq);
}

#[test]
fn hybrid_preferred_over_pq_in_mode_negotiation() {
    let decision = openpulse_core::select_signing_mode(
        PolicyProfile::Balanced,
        SigningMode::Normal,
        &[SigningMode::Pq, SigningMode::Hybrid, SigningMode::Normal],
    )
    .expect("select_signing_mode");
    assert_eq!(
        decision,
        SigningMode::Hybrid,
        "Hybrid has higher strength than Pq and should be selected"
    );
}

// ------------------------------------------------------------------
// Group 3: Security / rejection tests
// ------------------------------------------------------------------

#[test]
fn pq_conreq_tampered_pq_signature_rejected() {
    let classical_seed = [0x55u8; 32];
    let (pq_sk, _) = generate_ml_dsa_44_keypair();
    let (_dk, kem_ek) = generate_ml_kem_768_keypair();

    let mut req = create_pq_conreq(
        "W1AW",
        &classical_seed,
        &pq_sk,
        &kem_ek,
        vec![SigningMode::Hybrid],
        "session-tamper",
    )
    .expect("create_pq_conreq");

    req.pq_signature[0] ^= 0xFF;

    let store = make_trust_store("W1AW");
    let result = verify_pq_conreq(&req, &store, PolicyProfile::Balanced, SigningMode::Normal);
    assert!(result.is_err(), "tampered PQ signature must be rejected");
}

#[test]
fn pq_conreq_tampered_classical_signature_rejected() {
    let classical_seed = [0x66u8; 32];
    let (pq_sk, _) = generate_ml_dsa_44_keypair();
    let (_dk, kem_ek) = generate_ml_kem_768_keypair();

    let mut req = create_pq_conreq(
        "W1AW",
        &classical_seed,
        &pq_sk,
        &kem_ek,
        vec![SigningMode::Hybrid],
        "session-tamper-ed",
    )
    .expect("create_pq_conreq");

    req.classical_signature[0] ^= 0xFF;

    let store = make_trust_store("W1AW");
    let result = verify_pq_conreq(&req, &store, PolicyProfile::Balanced, SigningMode::Normal);
    assert!(
        result.is_err(),
        "tampered classical signature must be rejected"
    );
}

#[test]
fn pq_conack_session_id_mismatch_rejected() {
    let classical_seed = [0x77u8; 32];
    let (pq_sk, _) = generate_ml_dsa_44_keypair();
    let (_dk, kem_ek) = generate_ml_kem_768_keypair();

    let (ack, _ss) = create_pq_conack(
        "KD9XYZ",
        &classical_seed,
        &pq_sk,
        &kem_ek,
        SigningMode::Hybrid,
        "session-correct",
    )
    .expect("create_pq_conack");

    let store = make_trust_store("KD9XYZ");
    let result = verify_pq_conack(
        &ack,
        "session-WRONG",
        &[SigningMode::Hybrid],
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    );
    assert!(result.is_err(), "mismatched session_id must be rejected");
}

#[test]
fn pq_conreq_pubkey_mismatch_rejected() {
    let classical_seed = [0xAAu8; 32];
    let (pq_sk, _) = generate_ml_dsa_44_keypair();
    let (_dk, kem_ek) = generate_ml_kem_768_keypair();

    let req = create_pq_conreq(
        "W1AW",
        &classical_seed,
        &pq_sk,
        &kem_ek,
        vec![SigningMode::Hybrid],
        "session-pk-mismatch",
    )
    .expect("create_pq_conreq");

    // Store a *different* pubkey for the same station ID.
    let wrong_pubkey = [0xBBu8; 32];
    let store = make_trusted_store("W1AW", wrong_pubkey);
    let result = verify_pq_conreq(&req, &store, PolicyProfile::Balanced, SigningMode::Normal);
    assert!(
        result.is_err(),
        "pubkey in frame must match stored trusted key"
    );
}

#[test]
fn pq_conack_pubkey_mismatch_rejected() {
    let classical_seed = [0xCCu8; 32];
    let (pq_sk, _) = generate_ml_dsa_44_keypair();
    let (_dk, kem_ek) = generate_ml_kem_768_keypair();

    let (ack, _ss) = create_pq_conack(
        "KD9XYZ",
        &classical_seed,
        &pq_sk,
        &kem_ek,
        SigningMode::Hybrid,
        "session-ack-pk-mismatch",
    )
    .expect("create_pq_conack");

    let wrong_pubkey = [0xDDu8; 32];
    let store = make_trusted_store("KD9XYZ", wrong_pubkey);
    let result = verify_pq_conack(
        &ack,
        "session-ack-pk-mismatch",
        &[SigningMode::Hybrid],
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    );
    assert!(
        result.is_err(),
        "pubkey in ACK must match stored trusted key"
    );
}

#[test]
fn pq_conreq_invalid_kem_pubkey_rejected() {
    let classical_seed = [0xEEu8; 32];
    let (pq_sk, _) = generate_ml_dsa_44_keypair();
    let (_dk, kem_ek) = generate_ml_kem_768_keypair();

    let mut req = create_pq_conreq(
        "W1AW",
        &classical_seed,
        &pq_sk,
        &kem_ek,
        vec![SigningMode::Hybrid],
        "session-bad-kem",
    )
    .expect("create_pq_conreq");

    // Truncate the KEM pubkey to an invalid length.
    req.kem_pubkey.truncate(16);

    let store = make_trust_store("W1AW");
    let result = verify_pq_conreq(&req, &store, PolicyProfile::Balanced, SigningMode::Normal);
    assert!(result.is_err(), "malformed KEM pubkey must be rejected");
}

#[test]
fn pq_conack_unauthorized_mode_rejected() {
    let classical_seed = [0xFFu8; 32];
    let (pq_sk, _) = generate_ml_dsa_44_keypair();
    let (_dk, kem_ek) = generate_ml_kem_768_keypair();

    // Responder selects Hybrid, but initiator only offered Pq.
    let (ack, _ss) = create_pq_conack(
        "KD9XYZ",
        &classical_seed,
        &pq_sk,
        &kem_ek,
        SigningMode::Hybrid,
        "session-unauth-mode",
    )
    .expect("create_pq_conack");

    let store = make_trust_store("KD9XYZ");
    let result = verify_pq_conack(
        &ack,
        "session-unauth-mode",
        &[SigningMode::Pq], // initiator only offered Pq, not Hybrid
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
    );
    assert!(
        result.is_err(),
        "mode not offered by initiator must be rejected"
    );
}

// ------------------------------------------------------------------
// Group 4: SAR transport
// ------------------------------------------------------------------

#[test]
fn pq_conreq_serialized_size_fits_in_sar_capacity() {
    let classical_seed = [0x88u8; 32];
    let (pq_sk, _) = generate_ml_dsa_44_keypair();
    let (_dk, kem_ek) = generate_ml_kem_768_keypair();

    let req = create_pq_conreq(
        "W1AW",
        &classical_seed,
        &pq_sk,
        &kem_ek,
        vec![SigningMode::Hybrid],
        "session-sar",
    )
    .expect("create_pq_conreq");

    let encoded = encode_pq_conreq(&req).expect("encode_pq_conreq");
    // SAR max = 64 005 bytes
    assert!(
        encoded.len() < 64_005,
        "PqConReq encoded size {} exceeds SAR capacity (64 005 B)",
        encoded.len()
    );
}

#[test]
fn sar_roundtrip_of_pq_conreq() {
    let classical_seed = [0x99u8; 32];
    let (pq_sk, _) = generate_ml_dsa_44_keypair();
    let (_dk, kem_ek) = generate_ml_kem_768_keypair();

    let req = create_pq_conreq(
        "W1AW",
        &classical_seed,
        &pq_sk,
        &kem_ek,
        vec![SigningMode::Hybrid],
        "session-sar-rt",
    )
    .expect("create_pq_conreq");

    let payload = encode_pq_conreq(&req).expect("encode_pq_conreq");

    // SAR encode
    let fragments = sar_encode(1, &payload).expect("sar_encode");
    assert!(!fragments.is_empty());

    // SAR reassemble
    let mut reassembler = SarReassembler::new(Duration::from_secs(30));
    let mut result = None;
    for frag in fragments {
        if let Some(data) = reassembler.ingest("session-sar-rt", &frag).expect("ingest") {
            result = Some(data);
            break;
        }
    }
    let reassembled = result.expect("SAR reassembly must complete");

    let decoded = decode_pq_conreq(&reassembled).expect("decode_pq_conreq");
    assert_eq!(decoded.station_id, req.station_id);
    assert_eq!(decoded.session_id, req.session_id);
    assert_eq!(decoded.pq_pubkey, req.pq_pubkey);
    assert_eq!(decoded.pq_signature, req.pq_signature);
    assert_eq!(decoded.classical_signature, req.classical_signature);
}
