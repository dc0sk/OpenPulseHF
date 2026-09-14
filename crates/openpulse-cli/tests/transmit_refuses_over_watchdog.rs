//! `openpulse transmit` refuses a frame that would outlast the PTT watchdog, BEFORE keying (#1299).
//!
//! Moving the CLI onto `SharedPtt` gave it a watchdog it never had. That watchdog force-releases at
//! `DEFAULT_PTT_MAX` (180 s), and a single *legitimate* frame can run longer: measured on BPSK31,
//! `Concatenated` reaches ~265 s at a 223 B payload. Transmitting one anyway would key the rig, get
//! released ~85 s in, and leave the engine writing the rest of the frame into an unkeyed
//! transmitter — airtime spent for a frame nobody can decode.
//!
//! End-to-end through the real binary rather than against `transmit::run`, because the thing worth
//! proving is that the refusal reaches the operator with something they can act on.

use assert_cmd::Command;
use predicates::prelude::*;

/// 223 bytes: the payload that maximises a `Concatenated` frame on BPSK31, because 223 +
/// `Frame::WIRE_OVERHEAD` tips into a SECOND 255-byte RS block. At 200 B the same command is 134 s
/// and must succeed — that control is the second test below.
fn payload(n: usize) -> String {
    "x".repeat(n)
}

#[test]
fn a_frame_longer_than_the_watchdog_is_refused_before_keying() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--ptt",
        "none",
        "transmit",
        &payload(223),
        "--mode",
        "BPSK31",
        "--fec",
        "concatenated",
    ]);
    cmd.assert()
        .failure()
        // The operator is told the numbers and what to do, not just "refused".
        .stderr(predicate::str::contains("continuous keying"))
        .stderr(predicate::str::contains("was not keyed"))
        .stderr(predicate::str::contains("--fec rs"))
        // And it must not claim to have sent anything.
        .stdout(predicate::str::contains("Transmitted").not());
}

#[test]
fn the_same_rung_under_the_deadline_still_transmits() {
    // The control that stops the test above passing for the wrong reason. If BPSK31+Concatenated
    // were refused at EVERY length — a mode-level refusal rather than an airtime one — this fails.
    // 200 B is one RS block: 134 s, inside the 180 s deadline.
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--ptt",
        "none",
        "transmit",
        &payload(200),
        "--mode",
        "BPSK31",
        "--fec",
        "concatenated",
    ]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Transmitted 200 bytes"));
}
