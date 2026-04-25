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
fn session_state_reads_persisted_snapshot_when_no_live_session() {
    let config_dir = unique_temp_dir("session-persist");
    fs::create_dir_all(&config_dir).expect("create config dir");

    let session_state = r#"{
  "session_id": "sess-abc",
  "peer": "W1ABC",
  "hpx_state": "activetransfer",
  "selected_mode": "normal",
  "trust_level": "verified",
  "policy_profile": "balanced",
  "updated_at_ms": 12345
}"#;
    fs::write(config_dir.join("session-state.json"), session_state)
        .expect("write session-state.json");

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.env("OPENPULSE_CONFIG_DIR", &config_dir);
    cmd.args([
        "--backend",
        "loopback",
        "session",
        "state",
        "--format",
        "json",
    ]);

    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "\"reason_code\": \"persisted_session_snapshot\"",
        ))
        .stdout(predicate::str::contains("\"session_id\": \"sess-abc\""));

    let _ = fs::remove_dir_all(config_dir);
}

#[test]
fn trust_store_import_list_revoke_round_trip() {
    let config_dir = unique_temp_dir("trust-store");
    fs::create_dir_all(&config_dir).expect("create config dir");

    let mut import_cmd = Command::cargo_bin("openpulse").expect("binary should build");
    import_cmd.env("OPENPULSE_CONFIG_DIR", &config_dir);
    import_cmd.args([
        "--backend",
        "loopback",
        "trust",
        "import",
        "--station-id",
        "W1ABC",
        "--key-id",
        "key-1",
        "--trust",
        "full",
        "--source",
        "out_of_band",
        "--format",
        "json",
    ]);

    import_cmd
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "\"reason_code\": \"trust_store_updated\"",
        ));

    let mut list_cmd = Command::cargo_bin("openpulse").expect("binary should build");
    list_cmd.env("OPENPULSE_CONFIG_DIR", &config_dir);
    list_cmd.args(["--backend", "loopback", "trust", "list", "--format", "json"]);

    list_cmd
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "\"reason_code\": \"trust_store_list\"",
        ))
        .stdout(predicate::str::contains("\"station_id\": \"W1ABC\""))
        .stdout(predicate::str::contains("\"status\": \"active\""));

    let mut revoke_cmd = Command::cargo_bin("openpulse").expect("binary should build");
    revoke_cmd.env("OPENPULSE_CONFIG_DIR", &config_dir);
    revoke_cmd.args([
        "--backend",
        "loopback",
        "trust",
        "revoke",
        "--station-or-key",
        "W1ABC",
        "--reason",
        "operator_revoked",
        "--format",
        "json",
    ]);

    revoke_cmd
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "\"reason_code\": \"trust_record_revoked\"",
        ));

    let mut list_after_revoke = Command::cargo_bin("openpulse").expect("binary should build");
    list_after_revoke.env("OPENPULSE_CONFIG_DIR", &config_dir);
    list_after_revoke.args(["--backend", "loopback", "trust", "list", "--format", "json"]);

    list_after_revoke
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("\"station_id\": \"W1ABC\""))
        .stdout(predicate::str::contains("\"status\": \"revoked\""));

    let _ = fs::remove_dir_all(config_dir);
}

#[test]
fn session_list_and_resume_use_persisted_snapshot() {
    let config_dir = unique_temp_dir("session-list-resume");
    fs::create_dir_all(&config_dir).expect("create config dir");

    let session_state = r#"{
  "session_id": "sess-list-1",
  "peer": "W1XYZ",
  "hpx_state": "activetransfer",
  "selected_mode": "normal",
  "trust_level": "verified",
  "policy_profile": "balanced",
  "updated_at_ms": 22222
}"#;
    fs::write(config_dir.join("session-state.json"), session_state)
        .expect("write session-state.json");

    let mut list_cmd = Command::cargo_bin("openpulse").expect("binary should build");
    list_cmd.env("OPENPULSE_CONFIG_DIR", &config_dir);
    list_cmd.args(["--backend", "loopback", "session", "list", "--format", "json"]);

    list_cmd
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("\"reason_code\": \"session_list\""))
        .stdout(predicate::str::contains("\"session_id\": \"sess-list-1\""))
        .stdout(predicate::str::contains("\"source\": \"persisted\""));

    let mut resume_cmd = Command::cargo_bin("openpulse").expect("binary should build");
    resume_cmd.env("OPENPULSE_CONFIG_DIR", &config_dir);
    resume_cmd.args([
        "--backend",
        "loopback",
        "session",
        "resume",
        "--format",
        "json",
    ]);

    resume_cmd
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "\"reason_code\": \"session_snapshot_resumed\"",
        ))
        .stdout(predicate::str::contains("\"session_id\": \"sess-list-1\""));

    let _ = fs::remove_dir_all(config_dir);
}
