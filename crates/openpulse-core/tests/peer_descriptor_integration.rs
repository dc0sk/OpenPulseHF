use openpulse_core::{verify_peer_descriptor, PeerDescriptor, PeerDescriptorError};

fn seed(b: u8) -> [u8; 32] {
    [b; 32]
}

#[test]
fn sign_and_verify_round_trip() {
    let desc = PeerDescriptor::sign("W1AW", 0x0003, 1_000, &seed(1)).unwrap();
    verify_peer_descriptor(&desc).expect("valid descriptor must verify");
}

#[test]
fn peer_id_equals_verifying_key() {
    // The peer_id IS the Ed25519 verifying key, so two descriptors with the same
    // seed must produce the same peer_id regardless of callsign.
    let desc_a = PeerDescriptor::sign("W1AW", 0x0001, 1_000, &seed(2)).unwrap();
    let desc_b = PeerDescriptor::sign("W1AW/P", 0x0002, 2_000, &seed(2)).unwrap();
    assert_eq!(desc_a.peer_id, desc_b.peer_id);
}

#[test]
fn different_keys_produce_different_peer_ids() {
    let desc_a = PeerDescriptor::sign("W1AW", 0x0001, 1_000, &seed(1)).unwrap();
    let desc_b = PeerDescriptor::sign("W1AW", 0x0001, 1_000, &seed(2)).unwrap();
    assert_ne!(desc_a.peer_id, desc_b.peer_id);
}

#[test]
fn tampered_callsign_rejected() {
    let mut desc = PeerDescriptor::sign("W1AW", 0x0001, 1_000, &seed(1)).unwrap();
    desc.callsign = "EVIL".to_string();
    assert!(matches!(
        verify_peer_descriptor(&desc),
        Err(PeerDescriptorError::InvalidSignature)
    ));
}

#[test]
fn tampered_capability_mask_rejected() {
    let mut desc = PeerDescriptor::sign("W1AW", 0x0001, 1_000, &seed(1)).unwrap();
    desc.capability_mask = 0xFFFF;
    assert!(matches!(
        verify_peer_descriptor(&desc),
        Err(PeerDescriptorError::InvalidSignature)
    ));
}

#[test]
fn tampered_timestamp_rejected() {
    let mut desc = PeerDescriptor::sign("W1AW", 0x0001, 1_000, &seed(1)).unwrap();
    desc.timestamp_ms = 9_999_999;
    assert!(matches!(
        verify_peer_descriptor(&desc),
        Err(PeerDescriptorError::InvalidSignature)
    ));
}

#[test]
fn tampered_peer_id_fails_key_reconstruction() {
    let mut desc = PeerDescriptor::sign("W1AW", 0x0001, 1_000, &seed(1)).unwrap();
    // Corrupt the peer_id so the verifying key reconstruction yields a key that
    // does not match the signature.
    desc.peer_id[0] ^= 0xFF;
    assert!(verify_peer_descriptor(&desc).is_err());
}

#[test]
fn callsign_hash_is_deterministic() {
    let desc = PeerDescriptor::sign("W1AW", 0x0001, 1_000, &seed(1)).unwrap();
    assert_eq!(desc.callsign_hash(), desc.callsign_hash());
}

#[test]
fn callsign_hash_differs_for_different_callsigns() {
    let desc_a = PeerDescriptor::sign("W1AW", 0x0001, 1_000, &seed(1)).unwrap();
    let desc_b = PeerDescriptor::sign("K9XYZ", 0x0001, 1_000, &seed(2)).unwrap();
    assert_ne!(desc_a.callsign_hash(), desc_b.callsign_hash());
}
