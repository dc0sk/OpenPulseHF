use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn calibrate_audio_reports_headroom() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args(["--backend", "loopback", "calibrate", "audio"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("headroom_db"))
        .stdout(predicate::str::contains("\"pass\""));
}

#[test]
fn calibrate_ptt_noop_passes() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args(["--backend", "loopback", "--ptt", "none", "calibrate", "ptt"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("latency_ms"))
        .stdout(predicate::str::contains("\"pass\""));
}

#[test]
fn calibrate_afc_bpsk250_loopback() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args(["--backend", "loopback", "calibrate", "afc"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("afc_offset_hz"));
}

#[test]
fn calibrate_audio_writes_json_output() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let out_path = dir.path().join("result.json");
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args(["--backend", "loopback", "calibrate", "--output"])
        .arg(&out_path)
        .arg("audio");
    cmd.assert().success();
    let content = std::fs::read_to_string(&out_path).expect("output file should exist");
    assert!(content.contains("headroom_db"), "JSON file: {content}");
}

#[test]
fn calibrate_rejects_unknown_backend() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args(["--backend", "nope", "calibrate", "audio"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("unknown backend"));
}
