use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

/// Over a clean channel every frame decodes, so the ladder climbs one rung per
/// ACK-UP: from SL2 (BPSK31), six frames reach SL8 (OFDM52-8PSK) on the fade-aware ladder.
#[test]
fn adaptive_clean_climbs_the_ladder() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "adaptive",
        "--profile",
        "fast",
        "--channel",
        "clean",
        "--frames",
        "6",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("profile=fast"))
        .stdout(contains("start: level=SL2 mode=BPSK31"))
        .stdout(contains("→ SL3 (BPSK63)"))
        .stdout(contains("final: level=SL8 mode=OFDM52-8PSK"));
}

/// `robust` climbs the same ladder but stops at its SL6 cap (QPSK250-D), never reaching OFDM.
#[test]
fn adaptive_robust_stops_at_its_cap() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "adaptive",
        "--profile",
        "robust",
        "--channel",
        "clean",
        "--frames",
        "8",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("start: level=SL2 mode=BPSK31"))
        .stdout(contains("final: level=SL6 mode=QPSK250-D"))
        .stdout(contains("mode=OFDM").not());
}

#[test]
fn adaptive_json_emits_frames_and_summary() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "adaptive",
        "--profile",
        "fast",
        "--channel",
        "clean",
        "--frames",
        "3",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"frame\":0"))
        // Auto-fed backlog: after frame 0 of 3 × 64 B, 2 × 64 = 128 B remain.
        .stdout(contains("\"backlog\":128"))
        .stdout(contains("\"summary\":true"))
        .stdout(contains("\"profile\":\"fast\""));
}

/// The A2 backlog gate auto-feeds the draining queue, so the final ACK-UP arrives
/// with the queue empty and is withheld — the ladder stops one rung short of the
/// ungated SL8 reached by `adaptive_clean_climbs_the_ladder`.
#[test]
fn adaptive_backlog_gate_holds_final_upgrade() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "adaptive",
        "--profile",
        "fast",
        "--channel",
        "clean",
        "--frames",
        "6",
        "--min-backlog",
        "64",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("min_backlog=64B"))
        .stdout(contains("final: level=SL7"));
}

#[test]
fn adaptive_rejects_unknown_profile() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "adaptive",
        "--profile",
        "bogus",
        "--channel",
        "clean",
        "--frames",
        "2",
    ]);
    cmd.assert()
        .failure()
        .stderr(contains("unknown session profile"))
        .stderr(contains("robust"));
}

#[test]
fn adaptive_awgn_requires_snr() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "adaptive",
        "--profile",
        "fast",
        "--channel",
        "awgn",
        "--frames",
        "2",
    ]);
    cmd.assert().failure().stderr(contains("requires --snr"));
}
