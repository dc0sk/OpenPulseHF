use ed25519_dalek::SigningKey;
use openpulse_core::manifest::{verify_manifest, ManifestError, TransferManifest};

fn make_seed(b: u8) -> [u8; 32] {
    [b; 32]
}

fn pubkey_for(seed: u8) -> [u8; 32] {
    SigningKey::from_bytes(&make_seed(seed))
        .verifying_key()
        .to_bytes()
}

#[test]
fn valid_manifest_accepted() {
    let payload = b"OpenPulseHF Phase 2.3 test payload";
    let m = TransferManifest::sign(payload, "W1AW", &make_seed(3)).unwrap();

    assert_eq!(m.payload_size, payload.len() as u64);
    verify_manifest(&m, &pubkey_for(3)).expect("valid manifest should be accepted");
}

#[test]
fn manifest_rejected_wrong_pubkey() {
    let payload = b"data";
    let m = TransferManifest::sign(payload, "W1AW", &make_seed(3)).unwrap();

    let result = verify_manifest(&m, &pubkey_for(4));
    assert!(
        matches!(result, Err(ManifestError::InvalidSignature)),
        "wrong public key must be rejected"
    );
}

#[test]
fn manifest_rejected_tampered_payload_hash() {
    let payload = b"data";
    let mut m = TransferManifest::sign(payload, "W1AW", &make_seed(3)).unwrap();
    m.payload_hash[0] ^= 0xff; // simulate payload modification

    let result = verify_manifest(&m, &pubkey_for(3));
    assert!(
        matches!(result, Err(ManifestError::InvalidSignature)),
        "tampered payload hash must be rejected"
    );
}

#[test]
fn manifest_rejected_tampered_signature() {
    let payload = b"data";
    let mut m = TransferManifest::sign(payload, "W1AW", &make_seed(3)).unwrap();
    m.signature[0] ^= 0xff; // corrupt signature

    let result = verify_manifest(&m, &pubkey_for(3));
    assert!(
        matches!(result, Err(ManifestError::InvalidSignature)),
        "tampered signature must be rejected"
    );
}

#[test]
fn manifest_rejected_tampered_size_field() {
    let payload = b"data";
    let mut m = TransferManifest::sign(payload, "W1AW", &make_seed(3)).unwrap();
    m.payload_size += 1; // sender claimed wrong size

    let result = verify_manifest(&m, &pubkey_for(3));
    assert!(
        matches!(result, Err(ManifestError::InvalidSignature)),
        "tampered size field must be rejected"
    );
}

#[test]
fn manifest_hash_matches_payload() {
    use sha2::{Digest, Sha256};

    let payload = b"hello manifest";
    let m = TransferManifest::sign(payload, "W1AW", &make_seed(3)).unwrap();

    let mut hasher = Sha256::new();
    hasher.update(payload);
    let expected: [u8; 32] = hasher.finalize().into();

    assert_eq!(m.payload_hash.as_slice(), expected.as_slice());
}
