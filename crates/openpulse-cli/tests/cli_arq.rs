//! CLI wiring for the `arq` subcommands. The full two-way exchange needs two
//! stations and is covered at the engine level by `two_way_arq.rs`; these tests
//! exercise argument parsing and profile resolution.

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn arq_send_rejects_unknown_profile() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args(["arq", "send", "--payload", "hi", "--profile", "bogus"]);
    cmd.assert()
        .failure()
        .stderr(contains("unknown session profile"));
}

#[test]
fn arq_listen_rejects_unknown_profile() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args(["arq", "listen", "--profile", "bogus", "--frames", "1"]);
    cmd.assert()
        .failure()
        .stderr(contains("unknown session profile"));
}

/// `listen --frames 0` does no I/O and exits cleanly — confirms the command is wired.
#[test]
fn arq_listen_zero_frames_succeeds() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args(["arq", "listen", "--frames", "0"]);
    cmd.assert().success();
}
