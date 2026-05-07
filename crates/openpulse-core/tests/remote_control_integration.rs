//! Integration tests for signed remote rig-control commands (Phase 7.5).

use ed25519_dalek::SigningKey;
use openpulse_core::handshake::InMemoryTrustStore;
use openpulse_core::remote_control::{
    create_rig_ctrl_cmd, RemoteControlError, RemoteControlHandler, RigCtrlCmdBody, RigCtrlCmdType,
    ValidatedRigCmd,
};
use openpulse_core::trust::PublicKeyTrustLevel;
use openpulse_core::wire_query::WireMsgType;

fn seed(b: u8) -> [u8; 32] {
    [b; 32]
}

fn pubkey_for(seed_byte: u8) -> [u8; 32] {
    SigningKey::from_bytes(&seed(seed_byte))
        .verifying_key()
        .to_bytes()
}

fn trusted_store(station_id: &str, seed_byte: u8) -> InMemoryTrustStore {
    let mut store = InMemoryTrustStore::new();
    store.add_trusted(station_id, pubkey_for(seed_byte));
    store
}

fn set_freq_body(sender_id: &str, ts_ms: u64) -> RigCtrlCmdBody {
    RigCtrlCmdBody {
        cmd: RigCtrlCmdType::SetFreq,
        rig: "b".into(),
        freq_hz: Some(14_074_000),
        mode: None,
        ts_ms,
        sender_id: sender_id.to_string(),
    }
}

const NOW: u64 = 1_000_000;

#[test]
fn rig_ctrl_cmd_wire_type_is_0x09() {
    assert_eq!(WireMsgType::RigCtrlCmd as u8, 0x09);
    assert_eq!(WireMsgType::from_u8(0x09), Some(WireMsgType::RigCtrlCmd));
}

#[test]
fn signed_command_encode_decode_round_trip() {
    let body = set_freq_body("W1AW", NOW);
    let cmd = create_rig_ctrl_cmd(body, &seed(1)).unwrap();
    let encoded = cmd.encode().unwrap();
    let decoded = openpulse_core::remote_control::RigCtrlCmd::decode(&encoded).unwrap();
    assert_eq!(decoded.body.freq_hz, Some(14_074_000));
    assert_eq!(decoded.body.rig, "b");
    assert_eq!(decoded.sender_pubkey, cmd.sender_pubkey);
    assert_eq!(decoded.signature, cmd.signature);
}

#[test]
fn verified_peer_command_accepted() {
    let store = trusted_store("W1AW", 1);
    let body = set_freq_body("W1AW", NOW);
    let cmd = create_rig_ctrl_cmd(body, &seed(1)).unwrap();

    let mut handler = RemoteControlHandler::new();
    let result = handler.handle(&cmd, &store, NOW);

    assert_eq!(
        result.unwrap(),
        ValidatedRigCmd {
            cmd: RigCtrlCmdType::SetFreq,
            rig: "b".into(),
            freq_hz: Some(14_074_000),
            mode: None,
            sender_id: "W1AW".into(),
        }
    );
}

#[test]
fn unknown_sender_rejected() {
    let store = InMemoryTrustStore::new(); // empty — W1AW not known
    let body = set_freq_body("W1AW", NOW);
    let cmd = create_rig_ctrl_cmd(body, &seed(1)).unwrap();

    let mut handler = RemoteControlHandler::new();
    let result = handler.handle(&cmd, &store, NOW);

    assert!(matches!(result, Err(RemoteControlError::UnknownSender(_))));
}

#[test]
fn reduced_trust_peer_rejected() {
    let mut store = InMemoryTrustStore::new();
    store.add_entry("K1XYZ", pubkey_for(2), PublicKeyTrustLevel::Marginal);
    let body = set_freq_body("K1XYZ", NOW);
    let cmd = create_rig_ctrl_cmd(body, &seed(2)).unwrap();

    let mut handler = RemoteControlHandler::new();
    let result = handler.handle(&cmd, &store, NOW);

    assert!(matches!(
        result,
        Err(RemoteControlError::InsufficientTrust(
            PublicKeyTrustLevel::Marginal
        ))
    ));
}

#[test]
fn unknown_trust_level_rejected() {
    let mut store = InMemoryTrustStore::new();
    store.add_entry("KA1UNK", pubkey_for(3), PublicKeyTrustLevel::Unknown);
    let body = set_freq_body("KA1UNK", NOW);
    let cmd = create_rig_ctrl_cmd(body, &seed(3)).unwrap();

    let mut handler = RemoteControlHandler::new();
    let result = handler.handle(&cmd, &store, NOW);

    assert!(matches!(
        result,
        Err(RemoteControlError::InsufficientTrust(
            PublicKeyTrustLevel::Unknown
        ))
    ));
}

#[test]
fn tampered_signature_rejected() {
    let store = trusted_store("W1AW", 1);
    let body = set_freq_body("W1AW", NOW);
    let mut cmd = create_rig_ctrl_cmd(body, &seed(1)).unwrap();
    cmd.signature[0] ^= 0xff; // corrupt one byte

    let mut handler = RemoteControlHandler::new();
    let result = handler.handle(&cmd, &store, NOW);

    assert!(matches!(result, Err(RemoteControlError::InvalidSignature)));
}

#[test]
fn tampered_body_rejected() {
    let store = trusted_store("W1AW", 1);
    let body = set_freq_body("W1AW", NOW);
    let mut cmd = create_rig_ctrl_cmd(body, &seed(1)).unwrap();
    // Change freq after signing — signature no longer covers new body.
    cmd.body.freq_hz = Some(7_074_000);

    let mut handler = RemoteControlHandler::new();
    let result = handler.handle(&cmd, &store, NOW);

    assert!(matches!(result, Err(RemoteControlError::InvalidSignature)));
}

#[test]
fn replayed_command_rejected() {
    let store = trusted_store("W1AW", 1);
    let body = set_freq_body("W1AW", NOW);
    let cmd = create_rig_ctrl_cmd(body, &seed(1)).unwrap();

    let mut handler = RemoteControlHandler::new();
    handler
        .handle(&cmd, &store, NOW)
        .expect("first call should succeed");
    let result = handler.handle(&cmd, &store, NOW);

    assert!(matches!(result, Err(RemoteControlError::Replayed)));
}

#[test]
fn stale_timestamp_rejected() {
    let store = trusted_store("W1AW", 1);
    // Timestamp is 31 s in the past — outside the 30 s window.
    let stale_ts = NOW - 31_001;
    let body = set_freq_body("W1AW", stale_ts);
    let cmd = create_rig_ctrl_cmd(body, &seed(1)).unwrap();

    let mut handler = RemoteControlHandler::new();
    let result = handler.handle(&cmd, &store, NOW);

    assert!(matches!(
        result,
        Err(RemoteControlError::ReplayWindowExpired(_))
    ));
}

#[test]
fn future_timestamp_within_window_accepted() {
    let store = trusted_store("W1AW", 1);
    // 5 s in the future is acceptable (clock skew tolerance).
    let body = set_freq_body("W1AW", NOW + 5_000);
    let cmd = create_rig_ctrl_cmd(body, &seed(1)).unwrap();

    let mut handler = RemoteControlHandler::new();
    assert!(handler.handle(&cmd, &store, NOW).is_ok());
}

#[test]
fn pubkey_mismatch_rejected() {
    // Trust store has pubkey for seed 1, but cmd is signed with seed 5.
    let store = trusted_store("W1AW", 1);
    let body = set_freq_body("W1AW", NOW);
    let cmd = create_rig_ctrl_cmd(body, &seed(5)).unwrap(); // wrong key

    let mut handler = RemoteControlHandler::new();
    let result = handler.handle(&cmd, &store, NOW);

    // Signature is valid for the different key but pubkey won't match the store.
    assert!(matches!(result, Err(RemoteControlError::PubkeyMismatch(_))));
}

#[test]
fn set_mode_command_accepted() {
    let store = trusted_store("W1AW", 1);
    let body = RigCtrlCmdBody {
        cmd: RigCtrlCmdType::SetMode,
        rig: "a".into(),
        freq_hz: None,
        mode: Some("USB".into()),
        ts_ms: NOW,
        sender_id: "W1AW".into(),
    };
    let cmd = create_rig_ctrl_cmd(body, &seed(1)).unwrap();

    let mut handler = RemoteControlHandler::new();
    let validated = handler.handle(&cmd, &store, NOW).unwrap();

    assert_eq!(validated.cmd, RigCtrlCmdType::SetMode);
    assert_eq!(validated.mode.as_deref(), Some("USB"));
    assert_eq!(validated.rig, "a");
}
