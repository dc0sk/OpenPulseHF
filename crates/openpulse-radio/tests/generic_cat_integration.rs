//! Integration tests for `GenericSerialCat` using `MockTransport`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use openpulse_radio::cat_controller::CatController;
use openpulse_radio::error::RadioError;
use openpulse_radio::generic_cat::{GenericSerialCat, MockTransport};
use openpulse_radio::rig_definition::{RigDefinition, RigMeta};
use openpulse_radio::rig_mode::RigMode;
use openpulse_radio::PttController;

// ── Helpers ────────────────────────────────────────────────────────────────────

fn ic7300_def() -> RigDefinition {
    RigDefinition::from_toml(include_str!("../rigs/icom-ic7300.toml")).unwrap()
}

fn ft817_def() -> RigDefinition {
    RigDefinition::from_toml(include_str!("../rigs/yaesu-ft817.toml")).unwrap()
}

fn make_ic7300_cat(read_bytes: Vec<u8>) -> GenericSerialCat {
    GenericSerialCat::with_transport(Box::new(MockTransport::new(read_bytes)), ic7300_def())
}

fn make_ft817_cat(read_bytes: Vec<u8>) -> GenericSerialCat {
    GenericSerialCat::with_transport(Box::new(MockTransport::new(read_bytes)), ft817_def())
}

/// Build a CAT controller alongside a handle to the bytes it writes.
///
/// The `*_sends_correct_bytes` tests below could not previously see those bytes at all — the
/// transport was boxed into the controller and its log went with it — so they asserted only that
/// the call returned `Ok`.
fn ic7300_cat_with_log(read_bytes: Vec<u8>) -> (GenericSerialCat, Arc<Mutex<Vec<u8>>>) {
    let t = MockTransport::new(read_bytes);
    let log = t.log_handle();
    (
        GenericSerialCat::with_transport(Box::new(t), ic7300_def()),
        log,
    )
}

fn ft817_cat_with_log(read_bytes: Vec<u8>) -> (GenericSerialCat, Arc<Mutex<Vec<u8>>>) {
    let t = MockTransport::new(read_bytes);
    let log = t.log_handle();
    (
        GenericSerialCat::with_transport(Box::new(t), ft817_def()),
        log,
    )
}

fn written(log: &Arc<Mutex<Vec<u8>>>) -> Vec<u8> {
    log.lock().expect("write log").clone()
}

// ── IC-7300 PTT ────────────────────────────────────────────────────────────────

#[test]
fn icom_ic7300_ptt_on_sends_correct_bytes() {
    // ptt_on: "FE FE {addr} {ctrl} 1C 00 01 FD" with addr=0x94, ctrl=0xE0
    // Response: 6 bytes (any content is fine for mock)
    let response = vec![0xFE, 0xFE, 0xE0, 0x94, 0x1C, 0xFB];
    let (mut cat, log) = ic7300_cat_with_log(response);
    cat.assert_ptt().unwrap();
    assert!(cat.is_asserted());
    assert_eq!(
        written(&log),
        vec![0xFE, 0xFE, 0x94, 0xE0, 0x1C, 0x00, 0x01, 0xFD],
        "IC-7300 ptt_on frame"
    );
}

#[test]
fn icom_ic7300_ptt_off_sends_correct_bytes() {
    let response = vec![0xFE, 0xFE, 0xE0, 0x94, 0x1C, 0xFB];
    let (mut cat, log) = ic7300_cat_with_log(response);
    cat.release_ptt().unwrap();
    assert!(!cat.is_asserted());
    assert_eq!(
        written(&log),
        vec![0xFE, 0xFE, 0x94, 0xE0, 0x1C, 0x00, 0x00, 0xFD],
        "IC-7300 ptt_off frame"
    );
}

// ── IC-7300 set_frequency ──────────────────────────────────────────────────────

#[test]
fn icom_ic7300_set_frequency_14074khz() {
    // set_frequency: "FE FE {addr} {ctrl} 00 {freq_bcd_le5} FD"
    // bcd_le5(14074000) = [0x00, 0x40, 0x07, 0x14, 0x00]
    // Byte correctness is verified in rig_definition unit tests; here we test the round-trip.
    let ack = vec![0xFE, 0xFE, 0xE0, 0x94, 0x00, 0xFB];
    let (mut cat, log) = ic7300_cat_with_log(ack);
    cat.set_frequency(14_074_000).unwrap();
    assert_eq!(
        written(&log),
        vec![0xFE, 0xFE, 0x94, 0xE0, 0x00, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD],
        "IC-7300 set_frequency(14.074 MHz) frame — bcd_le5(14074000)"
    );
}

// ── IC-7300 get_frequency ──────────────────────────────────────────────────────

#[test]
fn icom_ic7300_get_frequency_parses_bcd_le() {
    // get_frequency response: 11 bytes; BCD LE5 at offset 5, length 5.
    // 14_074_000 Hz as BCD LE5: [0x00, 0x40, 0x07, 0x14, 0x00]
    let response = vec![
        0xFE, 0xFE, 0xE0, 0x94, 0x03, // header (offset 0-4)
        0x00, 0x40, 0x07, 0x14, 0x00, // BCD LE5 at offset 5
        0xFD, // terminator
    ];
    let mut cat = make_ic7300_cat(response);
    assert_eq!(cat.get_frequency().unwrap(), 14_074_000);
}

// ── FT-817 PTT ────────────────────────────────────────────────────────────────

#[test]
fn yaesu_ft817_ptt_on_sends_correct_bytes() {
    // ptt_on: "00 00 00 00 08", response 1 byte
    let (mut cat, log) = ft817_cat_with_log(vec![0x00]);
    cat.assert_ptt().unwrap();
    assert!(cat.is_asserted());
    assert_eq!(
        written(&log),
        vec![0x00, 0x00, 0x00, 0x00, 0x08],
        "FT-817 ptt_on frame"
    );
}

// ── FT-817 set_frequency ──────────────────────────────────────────────────────

#[test]
fn yaesu_ft817_set_frequency_bcd_be() {
    // set_frequency: "{freq_bcd4_be} 01", response 1 byte
    // bcd4_be(14074000) = [0x14, 0x07, 0x40, 0x00]; verified in rig_definition unit tests.
    let (mut cat, log) = ft817_cat_with_log(vec![0x00]);
    cat.set_frequency(14_074_000).unwrap();
    assert_eq!(
        written(&log),
        vec![0x14, 0x07, 0x40, 0x00, 0x01],
        "FT-817 set_frequency(14.074 MHz) frame"
    );
}

// ── Missing command → Unsupported ─────────────────────────────────────────────

#[test]
fn missing_command_returns_unsupported() {
    // FT-817 def has no get_frequency command.
    let mut cat = make_ft817_cat(vec![]);
    let err = cat.get_frequency().unwrap_err();
    assert!(
        matches!(err, RadioError::Unsupported("get_frequency")),
        "expected Unsupported(\"get_frequency\"), got {err:?}"
    );
}

#[test]
fn missing_set_mode_command_returns_unsupported() {
    // FT-817 def has no set_mode_usb command.
    let mut cat = make_ft817_cat(vec![]);
    let err = cat.set_mode(&RigMode::Usb).unwrap_err();
    assert!(
        matches!(err, RadioError::Unsupported("set_mode")),
        "expected Unsupported(\"set_mode\"), got {err:?}"
    );
}

// ── Load from TOML file ────────────────────────────────────────────────────────

#[test]
fn load_ic7300_rig_file_roundtrip() {
    let def = RigDefinition::from_toml(include_str!("../rigs/icom-ic7300.toml")).unwrap();
    assert_eq!(def.rig.model, "icom-ic7300");
    assert_eq!(def.rig.baud, 9600);
    assert!(def.commands.contains_key("ptt_on"));
    assert!(def.commands.contains_key("ptt_off"));
    assert!(def.commands.contains_key("set_frequency"));
    assert!(def.commands.contains_key("get_frequency"));
}

#[test]
fn load_ft817_rig_file_roundtrip() {
    let def = RigDefinition::from_toml(include_str!("../rigs/yaesu-ft817.toml")).unwrap();
    assert_eq!(def.rig.model, "yaesu-ft817");
    assert_eq!(def.rig.baud, 4800);
    assert!(def.commands.contains_key("ptt_on"));
    assert!(def.commands.contains_key("ptt_off"));
    assert!(def.commands.contains_key("set_frequency"));
    assert!(!def.commands.contains_key("get_frequency"));
}

// ── Empty rig def — missing command path ─────────────────────────────────────

#[test]
fn generic_backend_empty_def_returns_unsupported() {
    let def = RigDefinition {
        rig: RigMeta {
            model: "test".into(),
            description: String::new(),
            baud: 9600,
            data_bits: 8,
            stop_bits: 1,
            parity: "none".into(),
        },
        params: HashMap::new(),
        commands: HashMap::new(),
    };
    let mut cat = GenericSerialCat::with_transport(Box::new(MockTransport::new(vec![])), def);
    assert!(matches!(
        cat.get_frequency().unwrap_err(),
        RadioError::Unsupported("get_frequency")
    ));
}
