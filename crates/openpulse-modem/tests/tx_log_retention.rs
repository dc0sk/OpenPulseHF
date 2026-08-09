//! The §97 TX record must be bounded in memory, durable on disk, and never silently short (#1110).
//!
//! **The defect.** `record_tx_frame` appends one `TxMetadata` at every emit seam, `log_frame` was
//! an unconditional `push` with no cap, and `clear_tx_session_log` — the only bound — had no
//! caller. A long-running daemon grew the log without limit.
//!
//! Clearing it on a timer would have been the wrong fix: that log **is** the compliance record, so
//! bounding it by deletion trades a memory leak for a missing record. And the in-memory log was
//! lost on restart anyway, which made it unfit for its stated purpose independently of its size.
//!
//! So disk is the record and memory is a bounded query cache. Two properties matter beyond "it
//! stops growing":
//!
//! - `frame_count()` must report the **true total**, not the retained count. A cap that also
//!   shrinks the reported count is the truncation-that-reads-as-completeness defect — a caller
//!   asking "how many frames did we transmit" would quietly get a smaller answer.
//! - a spill write failure must **never** stop a transmission that is already on the air.

use bpsk_plugin::BpskPlugin;
use openpulse_audio::LoopbackBackend;
use openpulse_core::tx_metadata::{TxMetadata, TxSessionLog};
use openpulse_modem::engine::ModemEngine;

fn engine(callsign: &str) -> ModemEngine {
    let mut e = ModemEngine::new(Box::new(LoopbackBackend::new()));
    e.register_plugin(Box::new(BpskPlugin::new()))
        .expect("register bpsk");
    e.set_callsign(callsign);
    e
}

/// THE GATE: the in-memory log stops growing, and still reports how many frames really went out.
#[test]
fn the_window_is_bounded_but_the_total_is_not_understated() {
    let mut log = TxSessionLog::with_retain("W1AW", 4);
    for seq in 0..20u16 {
        log.log_frame(TxMetadata::with_timestamp(
            "W1AW",
            1_000 + u64::from(seq),
            "BPSK250",
            10.0,
            seq,
        ))
        .expect("log frame");
    }

    assert_eq!(log.retained(), 4, "the window must be capped");
    assert_eq!(
        log.frame_count(),
        20,
        "frame_count must report every frame transmitted, not the window size — a cap that also \
         shrinks the count is a record that lies about completeness"
    );
    assert!(log.is_truncated(), "the caller must be able to tell");
    assert_eq!(
        log.frames.first().map(|m| m.frame_sequence),
        Some(16),
        "eviction must drop the OLDEST frames, keeping the most recent window"
    );
}

/// A rejected frame is not a transmitted frame, so it must not be counted as one.
#[test]
fn a_station_mismatch_is_not_counted() {
    let mut log = TxSessionLog::new("W1AW");
    log.log_frame(TxMetadata::with_timestamp(
        "N0CALL", 1_000, "BPSK250", 10.0, 1,
    ))
    .expect_err("a foreign callsign must be rejected");
    assert_eq!(
        log.frame_count(),
        0,
        "a rejected frame must not inflate the §97 count"
    );
}

/// The record reaches disk, one NDJSON line per transmitted frame, and survives the process that
/// wrote it — which the in-memory log never did.
#[test]
fn every_transmitted_frame_is_appended_to_disk() {
    let dir = std::env::temp_dir().join(format!("openpulse-txlog-{}", std::process::id()));
    let path = dir.join("nested").join("tx-log.ndjson");
    let _ = std::fs::remove_dir_all(&dir);

    let mut e = engine("W1AW");
    e.set_tx_log_path(Some(path.clone()));
    for _ in 0..3 {
        e.transmit(b"compliance record", "BPSK250", None)
            .expect("transmit");
    }

    let body = std::fs::read_to_string(&path).expect("the spill file must exist");
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        e.tx_session_log().frame_count(),
        "one line per logged frame; got {lines:?}"
    );
    for l in &lines {
        let v: serde_json::Value = serde_json::from_str(l).expect("each line must be valid JSON");
        assert_eq!(v["station_id"], "W1AW");
        assert!(
            v.get("power_watts").is_some() && v.get("timestamp_ms").is_some(),
            "the §97 fields FrameTransmitted lacks must be present: {v}"
        );
    }
    assert!(!e.tx_log_failed(), "a healthy write must not latch failure");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A spill failure must not break transmission. The radio keying is the thing that must not be
/// coupled to a disk being full.
#[test]
fn a_failed_spill_never_blocks_a_transmit() {
    let mut e = engine("W1AW");
    // A path whose parent cannot be created — the write must fail, and be swallowed.
    e.set_tx_log_path(Some(std::path::PathBuf::from(
        "/proc/openpulse/tx-log.ndjson",
    )));

    for _ in 0..3 {
        e.transmit(b"still transmitting", "BPSK250", None)
            .expect("a TX-log write failure must never fail the transmit");
    }

    assert!(
        e.tx_log_failed(),
        "the failure must be visible via the tripwire rather than silent"
    );
    assert_eq!(
        e.tx_session_log().frame_count(),
        3,
        "the in-memory record must still be complete when the disk one is not"
    );
}

/// Spilling is off unless configured, so nothing starts writing files by surprise.
#[test]
fn no_path_means_no_file_and_no_failure() {
    let mut e = engine("W1AW");
    e.transmit(b"no spill configured", "BPSK250", None)
        .expect("transmit");
    assert!(!e.tx_log_failed());
    assert_eq!(e.tx_session_log().frame_count(), 1);
}
