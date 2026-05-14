use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "openpulse-cli-{name}-{}-{}",
        std::process::id(),
        nonce
    ))
}

#[test]
fn session_metrics_exports_json_with_perf_fields() {
    let config_dir = unique_temp_dir("session-metrics");
    fs::create_dir_all(&config_dir).expect("create config dir");

    let session_log = r#"[
    {
        "timestamp_ms": 1000,
        "from_state": "activetransfer",
        "to_state": "teardown",
        "event": "transfercomplete",
        "reason_code": "success",
        "reason_string": "snr_db=12.0"
    },
    {
        "timestamp_ms": 1200,
        "from_state": "teardown",
        "to_state": "idle",
        "event": "transfercomplete",
        "reason_code": "success",
        "reason_string": "teardown complete"
    },
    {
        "timestamp_ms": 3000,
        "from_state": "activetransfer",
        "to_state": "failed",
        "event": "transfererror",
        "reason_code": "timeout",
        "reason_string": "snr_db=8.0"
    }
]"#;
    fs::write(config_dir.join("session-log.json"), session_log).expect("write session-log.json");

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.env("OPENPULSE_CONFIG_DIR", &config_dir);
    cmd.args([
        "--backend",
        "loopback",
        "session-metrics",
        "--format",
        "json",
    ]);

    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "\"reason_code\": \"session_metrics_export\"",
        ))
        .stdout(predicate::str::contains("\"throughput_bps\""))
        .stdout(predicate::str::contains("\"transfer_ok\": 1"))
        .stdout(predicate::str::contains("\"transfer_error\": 1"))
        .stdout(predicate::str::contains("\"fer\": 0.5"))
        .stdout(predicate::str::contains("\"latency_ms\": 2000.0"))
        .stdout(predicate::str::contains("\"snr_db_estimate\": 10.0"));

    let _ = fs::remove_dir_all(config_dir);
}
