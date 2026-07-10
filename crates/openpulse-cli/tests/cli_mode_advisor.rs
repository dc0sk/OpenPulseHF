use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn mode_advisor_outputs_expected_levels_across_hpx_hf_ladder() {
    // Expected (SNR dB → level, mode) against the finer hpx_hf floors in `profile.rs` (research #2):
    // SL2=3 SL3=4 SL4=4.5 SL5=5 SL6=7 SL7=9 SL8=11 SL9=12 SL10=13 SL11=14 SL12=16 SL13=17 SL14=19
    // SL15=22. The advisor picks the highest rung whose floor is met, so a probe *at* a floor lands on
    // that rung. SL6/SL7 are QPSK250 at different FEC (coded gap-filler vs uncoded).
    let cases = [
        (0.0, "SL2", "BPSK31"),
        (3.5, "SL2", "BPSK31"),
        (4.5, "SL4", "BPSK100"),
        (5.5, "SL5", "BPSK250"),
        (8.5, "SL6", "QPSK250"),
        (9.5, "SL7", "QPSK250"),
        (11.5, "SL8", "QPSK500"),
        (12.0, "SL9", "8PSK500"),
        (13.0, "SL10", "SCFDMA26-32QAM"),
        (14.0, "SL11", "OFDM52-8PSK"),
        (16.0, "SL12", "OFDM52-16QAM"),
        (17.0, "SL13", "OFDM52-32QAM"),
        (19.0, "SL13", "OFDM52-32QAM"),
        (22.0, "SL14", "OFDM52-64QAM"),
        (23.0, "SL15", "OFDM52-16QAM"),
        (30.0, "SL17", "OFDM52-64QAM"),
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
