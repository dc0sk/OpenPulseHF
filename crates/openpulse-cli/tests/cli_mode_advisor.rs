use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn mode_advisor_outputs_expected_levels_for_10_snr_values() {
    let cases = [
        (0.0, "SL2", "BPSK31"),
        (2.5, "SL2", "BPSK31"),
        (3.5, "SL2", "BPSK31"),
        (4.5, "SL3", "BPSK63"),
        (5.5, "SL4", "BPSK250"),
        (8.5, "SL4", "BPSK250"),
        (9.5, "SL5", "QPSK250"),
        (10.5, "SL5", "QPSK250"),
        (12.0, "SL6", "QPSK500"),
        (15.0, "SL7", "8PSK500"),
    ];

    for (snr, level, mode) in cases {
        let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
        // Pin the profile so the assertion is independent of any ambient config file.
        cmd.args([
            "mode-advisor",
            "--snr",
            &snr.to_string(),
            "--profile",
            "hpx_hf",
        ]);

        cmd.assert()
            .success()
            .stdout(contains("profile=hpx_hf"))
            .stdout(contains(format!("recommended_speed_level={level}")))
            .stdout(contains(format!("recommended_mode={mode}")))
            .stdout(contains("reason="));
    }
}

#[test]
fn mode_advisor_selects_ofdm_hom_ladder() {
    // The OFDM higher-order ladder must be reachable via --profile.
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args(["mode-advisor", "--snr", "30", "--profile", "hpx_ofdm_hf"]);
    cmd.assert()
        .success()
        .stdout(contains("profile=hpx_ofdm_hf"))
        .stdout(contains("recommended_speed_level=SL10"))
        .stdout(contains("recommended_mode=OFDM52-64QAM"));

    // Separator/case normalisation also works.
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args(["mode-advisor", "--snr", "0", "--profile", "HPX-OFDM-HF"]);
    cmd.assert()
        .success()
        // Below the lowest rung's floor → the most robust defined rung (OFDM16), not UNMAPPED.
        .stdout(contains("recommended_mode=OFDM16"));
}

#[test]
fn mode_advisor_rejects_unknown_profile() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args(["mode-advisor", "--snr", "20", "--profile", "bogus"]);
    cmd.assert()
        .failure()
        .stderr(contains("unknown session profile"))
        .stderr(contains("hpx_ofdm_hf"));
}
