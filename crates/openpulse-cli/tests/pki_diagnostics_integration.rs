use assert_cmd::Command;
use mockito::{Matcher, Server};
use predicates::prelude::*;

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
