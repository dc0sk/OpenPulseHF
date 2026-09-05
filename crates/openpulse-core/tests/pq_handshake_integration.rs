use openpulse_core::handshake::conreq_hash;
use openpulse_core::pq_handshake::{PqConAckParams, PqConReqParams};
use openpulse_core::sar::{sar_encode, SarReassembler};
use openpulse_core::trust::PublicKeyTrustLevel;
use openpulse_core::{
    create_pq_conack, create_pq_conreq, decode_pq_conack, decode_pq_conreq,
    generate_ml_dsa_44_keypair, generate_ml_kem_768_keypair, kem_decapsulate, verify_pq_conack,
    verify_pq_conreq, InMemoryTrustStore, PolicyProfile, SigningMode, ML_DSA_44_PUBKEY_SIZE,
    ML_DSA_44_SIG_SIZE, ML_KEM_768_CT_SIZE, ML_KEM_768_DK_SIZE, ML_KEM_768_EK_SIZE,
    ML_KEM_768_SS_SIZE,
};
use std::time::Duration;

const TS: u64 = 1_700_000_000_000;

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
    let (pq_sk, _pq_vk) = generate_ml_dsa_44_keypair();

    let (ack_bytes, ss_responder) = create_pq_conack(
        &PqConAckParams {
            station_id: "W1AW",
            pq_signing_key: &pq_sk,
            req_kem_ek: &ek,
            selected_mode: SigningMode::Hybrid,
            conreq_hash: [0u8; 32],
            timestamp_ms: TS,
        },
        &[0u8; 32],
    )
    .expect("create_pq_conack");
    let ack = decode_pq_conack(&ack_bytes).expect("decode_pq_conack");

    let ss_initiator = kem_decapsulate(&dk, &ack.kem_ciphertext).expect("kem_decapsulate");

    assert_eq!(
        ss_responder.len(),
        ML_KEM_768_SS_SIZE,
        "shared secret must be 32 bytes"
    );
    assert_eq!(ack.kem_ciphertext.len(), ML_KEM_768_CT_SIZE);
    assert_eq!(ss_initiator, ss_responder, "shared secrets must match");
}

fn make_trust_store(_station_id: &str) -> InMemoryTrustStore {
    InMemoryTrustStore::new()
}

fn make_trusted_store(station_id: &str, pubkey: [u8; 32]) -> InMemoryTrustStore {
    let mut store = InMemoryTrustStore::new();
    store.add_entry(station_id, pubkey, PublicKeyTrustLevel::Full);
    store
}

/// A signed PQ CONREQ and the keys behind it.
struct Req {
    bytes: Vec<u8>,
    kem_ek: Vec<u8>,
    kem_dk: Vec<u8>,
}

fn make_req(seed: u8, modes: Vec<SigningMode>, dst: &str) -> Req {
    let (pq_sk, _) = generate_ml_dsa_44_keypair();
    let (kem_dk, kem_ek) = generate_ml_kem_768_keypair();
    let bytes = create_pq_conreq(
        &PqConReqParams {
            station_id: "W1AW",
            dst_station: dst,
            pq_signing_key: &pq_sk,
            kem_ek: &kem_ek,
            signing_modes: modes,
            session_id: 1,
            timestamp_ms: TS,
        },
        &[seed; 32],
    )
    .expect("create_pq_conreq");
    Req {
        bytes,
        kem_ek,
        kem_dk,
    }
}

#[test]
fn pq_conreq_hybrid_creates_and_verifies() {
    let r = make_req(
        0x11,
        vec![SigningMode::Hybrid, SigningMode::Normal],
        "K2XYZ",
    );
    let decoded = decode_pq_conreq(&r.bytes).expect("decode");
    assert_eq!(decoded.pq_signature.len(), ML_DSA_44_SIG_SIZE);
    assert_eq!(decoded.classical_signature.len(), 64);
    assert_eq!(decoded.dst_station, "K2XYZ");
    assert_eq!(decoded.timestamp_ms, TS);

    let store = make_trust_store("W1AW");
    let (_req, decision) = verify_pq_conreq(
        &r.bytes,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None,
    )
    .expect("verify_pq_conreq");
    assert_eq!(decision.selected_mode, SigningMode::Hybrid);
}

#[test]
fn pq_conack_hybrid_creates_verifies_and_decapsulates() {
    let r = make_req(
        0x11,
        vec![SigningMode::Hybrid, SigningMode::Normal],
        "K2XYZ",
    );
    let (pq_sk_b, _) = generate_ml_dsa_44_keypair();
    let (ack_bytes, ss_responder) = create_pq_conack(
        &PqConAckParams {
            station_id: "K2XYZ",
            pq_signing_key: &pq_sk_b,
            req_kem_ek: &r.kem_ek,
            selected_mode: SigningMode::Hybrid,
            conreq_hash: conreq_hash(&r.bytes),
            timestamp_ms: TS + 100,
        },
        &[0x22; 32],
    )
    .expect("create_pq_conack");

    let store = make_trust_store("K2XYZ");
    let (ack, decision) = verify_pq_conack(
        &ack_bytes,
        &r.bytes,
        &[SigningMode::Hybrid, SigningMode::Normal],
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None,
    )
    .expect("verify_pq_conack");
    assert_eq!(decision.selected_mode, SigningMode::Hybrid);
    assert_eq!(ack.kem_ciphertext.len(), ML_KEM_768_CT_SIZE);

    let ss_initiator = kem_decapsulate(&r.kem_dk, &ack.kem_ciphertext).expect("decapsulate");
    assert_eq!(ss_initiator, ss_responder, "shared secrets must match");
}

#[test]
fn pq_conreq_pq_only_mode() {
    let r = make_req(0x33, vec![SigningMode::Pq], "K2XYZ");
    let decoded = decode_pq_conreq(&r.bytes).expect("decode");
    assert!(
        decoded.classical_signature.is_empty(),
        "Pq-only must carry no classical signature"
    );
    let store = make_trust_store("W1AW");
    assert!(verify_pq_conreq(
        &r.bytes,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Pq,
        None
    )
    .is_ok());
}

/// The classical-signature presence flag is INSIDE the signed body, and the trailer length is
/// cross-checked against it. Flipping the trailer alone cannot re-split the signatures.
#[test]
fn a_truncated_pq_signature_region_is_rejected() {
    let r = make_req(0x44, vec![SigningMode::Hybrid], "K2XYZ");
    let mut short = r.bytes.clone();
    short.truncate(short.len() - 1);
    assert!(
        decode_pq_conreq(&short).is_err(),
        "a frame whose trailer disagrees with its signed presence flag must be refused"
    );
}

#[test]
fn pq_conreq_tampered_pq_signature_rejected() {
    let r = make_req(0x44, vec![SigningMode::Hybrid], "K2XYZ");
    let mut bad = r.bytes.clone();
    let n = bad.len();
    bad[n - 10] ^= 0xFF;
    let store = make_trust_store("W1AW");
    assert!(verify_pq_conreq(
        &bad,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None
    )
    .is_err());
}

/// BYTE tamper inside the signed body — the PQ equivalent of the classical gate.
#[test]
fn pq_conreq_tampered_body_rejected() {
    let r = make_req(0x55, vec![SigningMode::Hybrid], "K2XYZ");
    let store = make_trust_store("W1AW");
    assert!(
        verify_pq_conreq(
            &r.bytes,
            &store,
            PolicyProfile::Balanced,
            SigningMode::Normal,
            None
        )
        .is_ok(),
        "control: the untampered frame must verify"
    );
    let mut bad = r.bytes.clone();
    bad[10] ^= 0xFF; // inside station_id / dst_station
    assert!(verify_pq_conreq(
        &bad,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None
    )
    .is_err());
}

/// Replaces the session-id mismatch test: v2 binds by hash over the transmitted CONREQ.
#[test]
fn pq_conack_bound_to_another_conreq_rejected() {
    let ours = make_req(0x66, vec![SigningMode::Hybrid], "K2XYZ");
    let theirs = make_req(0x67, vec![SigningMode::Hybrid], "K2XYZ");
    let (pq_sk_b, _) = generate_ml_dsa_44_keypair();
    let (ack_bytes, _) = create_pq_conack(
        &PqConAckParams {
            station_id: "K2XYZ",
            pq_signing_key: &pq_sk_b,
            req_kem_ek: &ours.kem_ek,
            selected_mode: SigningMode::Hybrid,
            conreq_hash: conreq_hash(&theirs.bytes),
            timestamp_ms: TS + 100,
        },
        &[0x22; 32],
    )
    .unwrap();
    let store = make_trust_store("K2XYZ");
    assert!(verify_pq_conack(
        &ack_bytes,
        &ours.bytes,
        &[SigningMode::Hybrid],
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None
    )
    .is_err());
}

#[test]
fn pq_conreq_pubkey_mismatch_rejected() {
    let r = make_req(0x77, vec![SigningMode::Hybrid], "K2XYZ");
    // Trusted under a DIFFERENT key than the frame carries.
    let store = make_trusted_store("W1AW", [0xAB; 32]);
    assert!(verify_pq_conreq(
        &r.bytes,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None
    )
    .is_err());
}

#[test]
fn pq_conack_unauthorized_mode_rejected() {
    let r = make_req(0x88, vec![SigningMode::Normal], "K2XYZ");
    let (pq_sk_b, _) = generate_ml_dsa_44_keypair();
    let (ack_bytes, _) = create_pq_conack(
        &PqConAckParams {
            station_id: "K2XYZ",
            pq_signing_key: &pq_sk_b,
            req_kem_ek: &r.kem_ek,
            selected_mode: SigningMode::Hybrid,
            conreq_hash: conreq_hash(&r.bytes),
            timestamp_ms: TS + 100,
        },
        &[0x22; 32],
    )
    .unwrap();
    let store = make_trust_store("K2XYZ");
    assert!(verify_pq_conack(
        &ack_bytes,
        &r.bytes,
        &[SigningMode::Normal],
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None
    )
    .is_err());
}

/// REPLAY FRESHNESS ON THE PQ PATH — a property that did not exist before #1147, because the PQ
/// bodies carried no timestamp at all.
#[test]
fn a_stale_pq_conreq_is_rejected() {
    let r = make_req(0x99, vec![SigningMode::Hybrid], "K2XYZ");
    let store = make_trust_store("W1AW");
    let fresh = openpulse_core::handshake::Freshness {
        now_ms: TS + 60_000,
        max_skew_ms: 120_000,
    };
    assert!(
        verify_pq_conreq(
            &r.bytes,
            &store,
            PolicyProfile::Balanced,
            SigningMode::Normal,
            Some(fresh)
        )
        .is_ok(),
        "control: a fresh frame inside the window must verify"
    );
    let stale = openpulse_core::handshake::Freshness {
        now_ms: TS + 500_000,
        max_skew_ms: 120_000,
    };
    assert!(
        verify_pq_conreq(
            &r.bytes,
            &store,
            PolicyProfile::Balanced,
            SigningMode::Normal,
            Some(stale)
        )
        .is_err(),
        "a stale PQ CONREQ must be rejected — before #1147 there was no timestamp to check"
    );
}

/// #1178 on the PQ path too: unaddressed cannot be spelled by omission.
#[test]
fn an_empty_pq_dst_station_is_refused() {
    let (pq_sk, _) = generate_ml_dsa_44_keypair();
    let (_dk, kem_ek) = generate_ml_kem_768_keypair();
    assert!(create_pq_conreq(
        &PqConReqParams {
            station_id: "W1AW",
            dst_station: "",
            pq_signing_key: &pq_sk,
            kem_ek: &kem_ek,
            signing_modes: vec![SigningMode::Hybrid],
            session_id: 1,
            timestamp_ms: TS,
        },
        &[1u8; 32],
    )
    .is_err());
}

/// The PQ frames are far too large for one fragment — recorded honestly rather than implied away.
/// ~5 kB is ~2.7 min at BPSK250. Scoping PQ into #1147 buys a FINISHED FORMAT (so wiring PQ later is
/// not a second wire break), not a deployable feature.
#[test]
fn pq_conreq_is_multi_fragment_and_that_is_expected() {
    let r = make_req(0xAA, vec![SigningMode::Hybrid], "K2XYZ");
    assert!(
        r.bytes.len() < 64_005,
        "PQ CONREQ {} B exceeds SAR capacity",
        r.bytes.len()
    );
    let frags = sar_encode(0, &r.bytes).expect("sar_encode");
    assert!(
        frags.len() > 1,
        "a PQ CONREQ is expected to span fragments; if this ever becomes 1, the airtime claim in \
         the #1147 design is out of date and should be re-derived"
    );
}

/// A1: the classical-signature presence flag must agree with what the signed modes imply.
///
/// The hazard is specific. `verify_pq_conreq` decides whether to CHECK the classical signature from
/// `is_pq_only(modes)` — not from the flag. So a hand-built frame with `flag = 1` and PQ-only modes
/// makes the decoder split 64 trailing bytes off as a "classical signature" that nothing ever
/// verifies: attacker-mutable content inside a frame the receiver reports as verified, and a frame
/// that is malleable, which breaks `conreq_hash` determinism and hence CONACK binding.
///
/// The flag is redundant with the modes by construction (`create` derives it), so enforcing the
/// equality costs nothing and removes the disagreement entirely.
#[test]
fn a_pq_frame_whose_flag_disagrees_with_its_modes_is_rejected() {
    // Pq-only: create emits NO classical signature, so the flag is 0.
    let r = make_req(0xC1, vec![SigningMode::Pq], "K2XYZ");
    assert!(
        decode_pq_conreq(&r.bytes).is_ok(),
        "control: the honest frame must decode"
    );
    let decoded = decode_pq_conreq(&r.bytes).unwrap();
    assert!(
        decoded.classical_signature.is_empty(),
        "control: Pq-only must carry no classical signature"
    );

    // Flip the flag to 1 and append 64 bytes of attacker content, so the trailer length matches
    // what the flipped flag claims. Before the fix this parsed, and those 64 bytes were never
    // verified by anything.
    let body_end = r.bytes.len() - 2420; // ML-DSA signature is the whole trailer here
    let mut forged = r.bytes[..body_end].to_vec();
    let flag_idx = body_end - 1;
    forged[flag_idx] = 1;
    forged.extend_from_slice(&[0xAAu8; 64]); // the bytes nothing would have checked
    forged.extend_from_slice(&r.bytes[body_end..]);

    assert!(
        decode_pq_conreq(&forged).is_err(),
        "a frame whose flag disagrees with its signed modes must be refused at decode, or it \
         carries 64 bytes that no verifier ever looks at"
    );
}

#[test]
fn sar_roundtrip_of_pq_conreq() {
    let r = make_req(0xBB, vec![SigningMode::Hybrid], "K2XYZ");
    let frags = sar_encode(0, &r.bytes).expect("sar_encode");
    let mut re = SarReassembler::new(Duration::from_secs(30));
    let mut out = None;
    for f in &frags {
        if let Ok(done) = re.ingest("pq-sar", f) {
            if let Some(d) = done.into_iter().next() {
                out = Some(d);
            }
        }
    }
    let reassembled = out.expect("reassembled");
    assert_eq!(reassembled, r.bytes);
    let store = make_trust_store("W1AW");
    assert!(verify_pq_conreq(
        &reassembled,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None
    )
    .is_ok());
}
