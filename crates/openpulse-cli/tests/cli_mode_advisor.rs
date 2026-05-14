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
        cmd.args(["mode-advisor", "--snr", &snr.to_string()]);

        cmd.assert()
            .success()
            .stdout(contains(format!("recommended_speed_level={level}")))
            .stdout(contains(format!("recommended_mode={mode}")))
            .stdout(contains("reason="));
    }
}
