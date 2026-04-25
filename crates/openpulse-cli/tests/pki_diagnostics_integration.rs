use assert_cmd::Command;
use mockito::{Matcher, Server};
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
fn trust_show_returns_fail_on_revoked_identity() {
    let mut server = Server::new();

    let _identity = server
        .mock("GET", "/api/v1/identities/W1ABC")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "record_id": "rec-1",
                "station_id": "W1ABC",
                "callsign": "W1ABC",
                "publication_state": "published",
                "current_revision_id": "rev-1"
            }"#,
        )
        .create();

    let _revocations = server
        .mock("GET", "/api/v1/revocations")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("record_id".into(), "rec-1".into()),
            Matcher::UrlEncoded("limit".into(), "50".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[
                {
                    "revocation_id": "rvk-1",
                    "record_id": "rec-1",
                    "reason_code": "compromised_key",
                    "effective_at": "2026-04-25T00:00:00Z"
                }
            ]"#,
        )
        .create();

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--pki-url",
        &server.url(),
        "trust",
        "show",
        "W1ABC",
        "--format",
        "json",
    ]);

    cmd.assert()
        .failure()
        .code(2)
        .stdout(predicate::str::contains(
            "\"reason_code\": \"policy_rejected\"",
        ));
}

#[test]
fn diagnose_manifest_returns_invalid_schema_when_no_current_bundle() {
    let mut server = Server::new();

    let _bundle = server
        .mock("GET", "/api/v1/trust-bundles/current")
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "status": "not_found",
                "detail": "current trust bundle not found"
            }"#,
        )
        .create();

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--pki-url",
        &server.url(),
        "diagnose",
        "manifest",
        "--session",
        "sess-1",
        "--format",
        "json",
    ]);

    cmd.assert()
        .failure()
        .code(2)
        .stdout(predicate::str::contains(
            "\"reason_code\": \"invalid_manifest_schema\"",
        ));
}

#[test]
fn diagnose_handshake_emits_session_audit_event() {
    let mut server = Server::new();

    let _identity = server
        .mock("GET", "/api/v1/identities/W1ABC")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "record_id": "rec-1",
                "station_id": "W1ABC",
                "callsign": "W1ABC",
                "publication_state": "published",
                "current_revision_id": "rev-1"
            }"#,
        )
        .create();

    let _revocations = server
        .mock("GET", "/api/v1/revocations")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("record_id".into(), "rec-1".into()),
            Matcher::UrlEncoded("limit".into(), "50".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create();

    let _audit = server
        .mock("POST", "/api/v1/session-audit-events")
        .match_header(
            "content-type",
            Matcher::Regex("application/json.*".to_string()),
        )
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "event_id": "evt-1",
                "session_id": "sess-1",
                "event_type": "session.audit_recorded"
            }"#,
        )
        .create();

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--pki-url",
        &server.url(),
        "diagnose",
        "handshake",
        "--peer",
        "W1ABC",
        "--format",
        "json",
    ]);

    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "\"reason_code\": \"verified_out_of_band\"",
        ));
}

#[test]
fn pki_transport_failure_returns_exit_code_3() {
    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--pki-url",
        "http://127.0.0.1:9",
        "trust",
        "show",
        "W1ABC",
        "--format",
        "json",
    ]);

    cmd.assert()
        .failure()
        .code(3)
        .stdout(predicate::str::contains(
            "\"reason_code\": \"pki_service_unreachable\"",
        ));
}

// ── session subcommand tests ──────────────────────────────────────────────────

fn mock_published_identity(
    server: &mut mockito::Server,
    callsign: &str,
    record_id: &str,
) -> mockito::Mock {
    server
        .mock("GET", format!("/api/v1/identities/{callsign}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{
                "record_id": "{record_id}",
                "station_id": "{callsign}",
                "callsign": "{callsign}",
                "publication_state": "published",
                "current_revision_id": "rev-1"
            }}"#,
        ))
        .create()
}

fn mock_empty_revocations(server: &mut mockito::Server, record_id: &str) -> mockito::Mock {
    server
        .mock("GET", "/api/v1/revocations")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("record_id".into(), record_id.into()),
            Matcher::UrlEncoded("limit".into(), "50".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create()
}

fn mock_session_audit_ok(server: &mut mockito::Server) -> mockito::Mock {
    server
        .mock("POST", "/api/v1/session-audit-events")
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"event_id": "evt-1", "session_id": "s1", "event_type": "session.audit_recorded"}"#,
        )
        .create()
}

#[test]
fn session_start_with_published_peer_returns_ok() {
    let mut server = Server::new();

    let _identity = mock_published_identity(&mut server, "W2XYZ", "rec-2");
    let _revocations = mock_empty_revocations(&mut server, "rec-2");
    let _audit = mock_session_audit_ok(&mut server);

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--pki-url",
        &server.url(),
        "session",
        "start",
        "--peer",
        "W2XYZ",
        "--format",
        "json",
    ]);

    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("\"status\": \"ok\""))
        .stdout(predicate::str::contains("\"hpx_state\""));
}

#[test]
fn session_start_with_unknown_peer_returns_exit_2() {
    let mut server = Server::new();

    // Direct lookup returns 404
    server
        .mock("GET", "/api/v1/identities/W9UNKNOWN")
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"not_found","detail":"no identity found"}"#)
        .create();

    // Station-id fallback lookup returns empty list → identity truly unknown
    server
        .mock(
            "GET",
            Matcher::Regex(r"^/api/v1/identities:lookup".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create();

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--pki-url",
        &server.url(),
        "session",
        "start",
        "--peer",
        "W9UNKNOWN",
        "--format",
        "json",
    ]);

    cmd.assert()
        .failure()
        .code(2)
        .stdout(predicate::str::contains(
            "\"reason_code\": \"identity_not_found\"",
        ));
}

#[test]
fn session_state_shows_idle_when_no_session_started() {
    let config_dir = unique_temp_dir("session-state-idle");
    fs::create_dir_all(&config_dir).expect("create config dir");

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.env("OPENPULSE_CONFIG_DIR", &config_dir);
    cmd.args([
        "--backend",
        "loopback",
        "--pki-url",
        "http://127.0.0.1:8787",
        "session",
        "state",
        "--format",
        "json",
    ]);

    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("\"hpx_state\": \"idle\""));

    let _ = fs::remove_dir_all(config_dir);
}

#[test]
fn session_log_returns_empty_transitions_when_no_session() {
    let config_dir = unique_temp_dir("session-log-empty");
    fs::create_dir_all(&config_dir).expect("create config dir");

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.env("OPENPULSE_CONFIG_DIR", &config_dir);
    cmd.args([
        "--backend",
        "loopback",
        "--pki-url",
        "http://127.0.0.1:8787",
        "session",
        "log",
        "--format",
        "json",
    ]);

    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("\"transition_count\": 0"));

    let _ = fs::remove_dir_all(config_dir);
}

#[test]
fn session_end_without_active_session_returns_ok() {
    let config_dir = unique_temp_dir("session-end-idle");
    fs::create_dir_all(&config_dir).expect("create config dir");

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.env("OPENPULSE_CONFIG_DIR", &config_dir);
    cmd.args([
        "--backend",
        "loopback",
        "--pki-url",
        "http://127.0.0.1:8787",
        "session",
        "end",
        "--format",
        "json",
    ]);

    // Ending when already idle succeeds gracefully
    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "\"reason_code\": \"session_ended\"",
        ));

    let _ = fs::remove_dir_all(config_dir);
}
