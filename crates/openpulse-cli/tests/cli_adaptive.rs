use assert_cmd::Command;
use predicates::str::contains;

/// Over a clean channel every frame decodes, so the ladder climbs one rung per
/// ACK-UP: from SL2 (BPSK31), six frames reach SL8 (SCFDMA52-8PSK).
#[test]
fn adaptive_clean_climbs_the_ladder() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "adaptive",
        "--profile",
        "hpx_hf",
        "--channel",
        "clean",
        "--frames",
        "6",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("profile=hpx_hf"))
        .stdout(contains("start: level=SL2 mode=BPSK31"))
        .stdout(contains("→ SL3 (BPSK63)"))
        .stdout(contains("final: level=SL8 mode=SCFDMA52-8PSK"));
}

/// The OFDM higher-order ladder is reachable and climbs to its densest rung.
#[test]
fn adaptive_ofdm_hf_reaches_top_rung() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "adaptive",
        "--profile",
        "hpx_ofdm_hf",
        "--channel",
        "clean",
        "--frames",
        "6",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("start: level=SL5 mode=OFDM16"))
        .stdout(contains("final: level=SL10 mode=OFDM52-64QAM"));
}

#[test]
fn adaptive_json_emits_frames_and_summary() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "adaptive",
        "--profile",
        "hpx_hf",
        "--channel",
        "clean",
        "--frames",
        "3",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"frame\":0"))
        .stdout(contains("\"summary\":true"))
        .stdout(contains("\"profile\":\"hpx_hf\""));
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
        .stderr(contains("unknown session profile"));
}

#[test]
fn adaptive_awgn_requires_snr() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "adaptive",
        "--profile",
        "hpx_hf",
        "--channel",
        "awgn",
        "--frames",
        "2",
    ]);
    cmd.assert().failure().stderr(contains("requires --snr"));
}
