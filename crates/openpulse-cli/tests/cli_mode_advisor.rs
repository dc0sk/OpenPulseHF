use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn mode_advisor_outputs_expected_levels_across_hpx_hf_ladder() {
    // Expected (SNR dB → level, mode) against the fade-aware hpx_hf floors in `profile.rs`:
    // SL2=3 SL3=4 SL4=4.5 SL5=5 SL6=7 SL7=9 SL8=10 SL9=12 SL10=14 SL11=16 SL12=18 SL13=19 SL14=20.
    // The OFDM rungs' floors are plugin symbol-domain SNR (which saturates ~17 dB), not AWGN channel
    // SNR — an AWGN-scale floor there is unreachable and the ladder stalls below it. Every rung is
    // coded and measured to decode on a Watterson moderate_f1 fade.
    let cases = [
        (0.0, "SL1", "MFSK16"), // below BPSK31's 3 dB floor → the MFSK16 sub-floor rung (REQ-WSIG-01)
        (3.5, "SL2", "BPSK31"),
        (4.5, "SL4", "BPSK100"),
        (5.5, "SL5", "BPSK250"),
        (8.5, "SL6", "QPSK250-D"),
        (9.0, "SL7", "OFDM52"),
        (10.0, "SL8", "OFDM52-8PSK"),
        (12.0, "SL9", "OFDM52-16QAM"),
        (14.0, "SL10", "OFDM52-32QAM"),
        (16.0, "SL11", "OFDM52-64QAM"),
        (18.0, "SL12", "OFDM52-16QAM"),
        (20.0, "SL14", "OFDM52-64QAM"),
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
        // clap now lists every accepted profile, sourced from SessionProfile::PROFILE_NAMES,
        // so an operator sees the valid set instead of just being told "no".
        .stderr(contains("hpx_ofdm_hf"))
        .stderr(contains("hpx_pilot_fast_rrc"));
}
