use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn transmit_with_ptt_none_succeeds() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--ptt",
        "none",
        "transmit",
        "hello",
        "--mode",
        "BPSK100",
    ]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Transmitted"));
}

#[test]
fn transmit_with_unknown_ptt_backend_errors() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--ptt",
        "invalid_backend",
        "transmit",
        "hello",
        "--mode",
        "BPSK100",
    ]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("unknown PTT backend"));
}

#[test]
fn transmit_default_ptt_is_none() {
    // No --ptt flag; should succeed with the default NoOpPtt.
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "transmit",
        "world",
        "--mode",
        "BPSK100",
    ]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Transmitted"));
}
