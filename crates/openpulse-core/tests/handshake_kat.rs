//! Known-answer vectors for the v2 binary handshake (#1147).
//!
//! **What a KAT buys that a round-trip test does not.** A round-trip proves the encoder and decoder
//! agree with *each other* — it passes just as happily if both drift together, which is exactly what
//! a format change does. These vectors pin the encoder's output against a value recorded outside the
//! code, so a layout change, a field reorder, or a different signing span fails here even when every
//! round-trip still passes.
//!
//! **Scope: pre-SAR and pre-whitening.** The vectors are the frame as `create` returns it, before
//! fragmentation and before the wire scrambler. That is deliberate — #1148 changed the whitening
//! keystream, and a KAT taken further down the stack would have been invalidated by a change that has
//! nothing to do with this format.
//!
//! **Why the signature pins too.** Ed25519 signing is deterministic (RFC 8032): no per-signature
//! randomness, so the same key over the same message yields the same 64 bytes on every run and every
//! platform. `ed25519-dalek`'s `rand_core` feature gates KEY GENERATION only, and these vectors use a
//! fixed seed rather than generating one.

use openpulse_core::handshake::{verify_conreq, ConReq, ConReqParams, InMemoryTrustStore};
use openpulse_core::pq_handshake::{
    create_pq_conack, create_pq_conreq, PqConAckParams, PqConReqParams,
};
use openpulse_core::trust::PublicKeyTrustLevel;
use openpulse_core::trust::{PolicyProfile, SigningMode};
use sha2::{Digest, Sha256};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The fully-specified CONREQ the vector below was taken from. Every field is fixed, including the
/// signing seed and the timestamp, so nothing about the frame depends on when or where it runs.
fn kat_conreq() -> Vec<u8> {
    ConReq::create(
        &ConReqParams {
            station_id: "W1AW",
            dst_station: "K2XYZ",
            signing_modes: vec![SigningMode::Normal, SigningMode::Psk],
            session_id: 1_700_000_000_000,
            station_grid: "FN31pr",
            profile_name: "hpx_hf",
            profile_fingerprint: 0x0123_4567_89AB_CDEF,
            timestamp_ms: 1_700_000_000_000,
            kex_pubkey: &[0x42u8; 32],
        },
        &[0x01u8; 32],
    )
    .expect("KAT CONREQ must encode")
}

/// THE VECTOR.
///
/// RE-RECORDED 2026-08-26 (#1191): `caps::STATION_ID` 12 → 18, `dst_station` removed from the
/// CONACK, and `WIRE_VERSION` reset to 0x01 and frozen until 1.0. The frame length is unchanged at
/// 187 B — this fixture's callsign is short, so the wider cap costs it nothing — but the version
/// byte and therefore the signature both moved. Also reproduced in `docs/dev/design/protocol-wire-spec.md` so an independent
/// implementer can check their encoder without building this crate.
const CONREQ_KAT: &str = "\
485343510100740457314157054b3258595a8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f\
6f5c42424242424242424242424242424242424242424242424242424242424242420201020000018bcfe5680006464e\
33317072066870785f68660123456789abcdef0000018bcfe568003a2570b187d50bfd206bd9560e8cd85cae60496ad4\
765ea2577242a4f6b9e0d68c29b95f61566d07849a35738e810c85050d877b1977bbb6002a8b7f552fb20c";

#[test]
fn the_classical_conreq_matches_its_known_answer_vector() {
    let frame = kat_conreq();
    assert_eq!(
        hex(&frame),
        CONREQ_KAT.replace(['\n', ' '], ""),
        "the CONREQ encoder no longer produces the recorded vector. If this is an INTENTIONAL wire \
         change, the version byte must change with it and the vector must be re-recorded (and the \
         copy in protocol-wire-spec.md updated). If it is not intentional, the wire format has \
         drifted and every deployed peer is now incompatible."
    );
    assert_eq!(frame.len(), 187, "frame length changed");
}

/// The vector is not just bytes: it is a frame that actually verifies. Without this the KAT could
/// pin a well-formed-looking frame that no receiver would accept.
#[test]
fn the_vector_is_a_frame_that_verifies() {
    let frame = kat_conreq();
    let decoded = ConReq::decode(&frame).expect("decode");
    let mut store = InMemoryTrustStore::new();
    let mut k = [0u8; 32];
    k.copy_from_slice(&decoded.pubkey);
    store.add_entry("W1AW", k, PublicKeyTrustLevel::Full);
    let (req, _) = verify_conreq(
        &frame,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None,
    )
    .expect("the KAT frame must verify");
    assert_eq!(req.station_id, "W1AW");
    assert_eq!(req.dst_station, "K2XYZ");
    assert_eq!(req.timestamp_ms, 1_700_000_000_000);
    assert_eq!(req.profile_fingerprint, 0x0123_4567_89AB_CDEF);
}

/// Signing the same message twice must give the same bytes — the property the vector depends on.
/// Asserted rather than cited, because "Ed25519 is deterministic" is a claim about this build's
/// dependency, not only about the RFC.
#[test]
fn ed25519_signing_is_deterministic_in_this_build() {
    assert_eq!(
        kat_conreq(),
        kat_conreq(),
        "two encodes of the same fully-specified CONREQ differ, so something in the signing path \
         is drawing randomness and the vector above cannot be a known answer"
    );
}

/// PQ CONREQ vector, pinned as a digest rather than 5 kB of inline hex.
///
/// **What is and is not pinnable here, measured rather than assumed.** ML-DSA-44 signing in this
/// build is DETERMINISTIC — verified by `pq_signing_is_deterministic_in_this_build` below — so the
/// whole frame pins. The PQ **CONACK** does not, and deliberately has no vector: it carries a
/// `kem_ciphertext` from `encapsulate()`, which is randomised by design, so only its encoder layout
/// could be pinned and not its bytes.
const PQ_CONREQ_KAT_SHA256: &str =
    "6281f74a0e15edb601a6f224e771fd7b9713eac336c8f4ce305fb1d74cb8f638";
const PQ_CONREQ_KAT_LEN: usize = 5049;

fn kat_pq_conreq() -> Vec<u8> {
    create_pq_conreq(
        &PqConReqParams {
            station_id: "W1AW",
            dst_station: "K2XYZ",
            pq_signing_key: &[0x02u8; 32],
            // A fixed byte pattern rather than a generated key, so the vector is reproducible.
            //
            // **CORRECTED.** This comment previously claimed such a frame "will not pass
            // `verify_pq_conreq`, which does validate it". That is FALSE, and it was written
            // without running it: 0x33 bytes unpack to 12-bit coefficients 0x333 = 819 < q = 3329,
            // so the FIPS 203 modulus check passes and `EncapsulationKey::new` accepts the key. The
            // frame verifies — see `the_pq_vector_is_a_frame_that_verifies` below, which now asserts
            // it rather than leaving the claim as prose.
            kem_ek: &[0x33u8; 1184],
            signing_modes: vec![SigningMode::Hybrid],
            session_id: 1_700_000_000_000,
            timestamp_ms: 1_700_000_000_000,
        },
        &[0x01u8; 32],
    )
    .expect("KAT PQ CONREQ must encode")
}

#[test]
fn the_pq_conreq_matches_its_known_answer_vector() {
    let frame = kat_pq_conreq();
    assert_eq!(frame.len(), PQ_CONREQ_KAT_LEN, "PQ frame length changed");
    let mut h = Sha256::new();
    h.update(&frame);
    let digest: [u8; 32] = h.finalize().into();
    assert_eq!(
        hex(&digest),
        PQ_CONREQ_KAT_SHA256,
        "the PQ CONREQ encoder no longer produces the recorded vector"
    );
}

/// The PQ vector is a frame that VERIFIES — asserted, after the comment above claimed the opposite
/// without being run. A KAT over a frame no receiver would accept could pin an encoder bug forever.
#[test]
fn the_pq_vector_is_a_frame_that_verifies() {
    let frame = kat_pq_conreq();
    let store = InMemoryTrustStore::new();
    openpulse_core::pq_handshake::verify_pq_conreq(
        &frame,
        &store,
        PolicyProfile::Balanced,
        SigningMode::Normal,
        None,
    )
    .expect("the PQ KAT frame must verify");
}

#[test]
fn pq_signing_is_deterministic_in_this_build() {
    assert_eq!(
        kat_pq_conreq(),
        kat_pq_conreq(),
        "ML-DSA signing is drawing randomness (a hedged build), so the PQ vector above cannot be a \
         known answer and must be reduced to a layout-only assertion"
    );
}

/// The PQ frame is far too large for one fragment, and that is expected — recorded here so the
/// vector's existence is not read as evidence that PQ is deployable. 5 049 B is **21 SAR fragments**
/// at 251 B, against 1 for a classical CONREQ.
///
/// No airtime figure: this said "5060 B ≈ 2.7 min at BPSK250", where the size was 11 bytes stale and
/// the duration was sourced from nothing — the repo records no measured PQ-handshake airtime, as
/// `openpulse-book.md` §2B.7.3 says in as many words. Fragments are derivable; minutes are not.
#[test]
fn the_pq_vector_is_not_evidence_that_pq_is_deployable() {
    // Measured from the frame, not from the constant: comparing two `const`s is an assertion that
    // can never fail at runtime, which clippy catches and which would have made this note decorative.
    let measured = kat_pq_conreq().len();
    assert!(
        measured > 251 * 4,
        "the PQ CONREQ is now small enough to question this note; re-derive the fragment claim \
         against `docs/dev/design/protocol-wire-spec.md` §4 (measured {measured} B). NOT against \
         `handshake-binary-encoding.md`, which is the design record and says of itself that it is \
         not maintained field-for-field."
    );
}

/// The PQ **CONACK**'s length is pinned even though its bytes cannot be.
///
/// `encapsulate()` is randomised, so no fixed byte string exists to hash — which is why there is no
/// byte vector for this frame. Its LENGTH is another matter: every field except `station_id` is
/// fixed-size, so the frame is exactly `4966 + station_id.len()` bytes and does not vary between
/// builds. Pinning that catches a field-size change on the PQ CONACK path, which the CONREQ vector
/// cannot see, and it replaces a figure that until now was derived from the encoder by hand.
#[test]
fn the_pq_conack_length_is_fixed_even_though_its_bytes_are_not() {
    let conreq = kat_pq_conreq();
    let hash = openpulse_core::handshake::conreq_hash(&conreq);
    let build = |station_id: &str| -> usize {
        create_pq_conack(
            &PqConAckParams {
                station_id,
                pq_signing_key: &[0x04u8; 32],
                req_kem_ek: &[0x33u8; 1184],
                selected_mode: SigningMode::Hybrid,
                conreq_hash: hash,
                timestamp_ms: 1_700_000_000_000,
            },
            &[0x05u8; 32],
        )
        .expect("KAT PQ CONACK must encode")
        .0
        .len()
    };

    // One byte per callsign character, and nothing else moving.
    for id in ["W1AW", "K2XYZ", "3DA0/DL1ABC", "AAAAAAAAAAAAAAAAAA"] {
        assert_eq!(
            build(id),
            4966 + id.len(),
            "the PQ CONACK layout changed for station_id {:?} ({} chars)",
            id,
            id.len()
        );
    }

    // The randomised KEM ciphertext must not change the LENGTH, or the pin above is luck.
    assert_eq!(
        build("W1AW"),
        build("W1AW"),
        "two CONACKs built from identical inputs differ in length, so the KEM ciphertext is not \
         fixed-size and this pin cannot hold"
    );

    // And it is the frame this test claims: 20 fragments, one fewer than the CONREQ's 21.
    assert_eq!(build("W1AW").div_ceil(251), 20);
}
