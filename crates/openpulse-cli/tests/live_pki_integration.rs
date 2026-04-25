/// Live PKI integration tests for the `openpulse` CLI.
///
/// These tests spin up the real `pki-tooling` axum router on a random TCP port,
/// pre-seed the Postgres database with test records, run CLI subprocesses against
/// the live server, and assert both CLI output and DB side-effects.
///
/// **Prerequisites**: set `PKI_TEST_DATABASE_URL` to a writable Postgres DSN.
/// Tests skip gracefully when the variable is absent.
use assert_cmd::Command;
use pki_tooling::{build_router, run_migrations, AppState};
use predicates::prelude::*;
use sqlx::postgres::PgPoolOptions;

// ── helpers ──────────────────────────────────────────────────────────────────

/// Connect to Postgres, run migrations, and return the pool.
/// Returns `None` (causing the test to skip) when `PKI_TEST_DATABASE_URL` is unset.
async fn setup_pool() -> Option<sqlx::PgPool> {
    let url = match std::env::var("PKI_TEST_DATABASE_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("skipping live PKI test: PKI_TEST_DATABASE_URL not set");
            return None;
        }
    };

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("failed to connect to PKI_TEST_DATABASE_URL");

    run_migrations(&pool)
        .await
        .expect("failed to run migrations");

    Some(pool)
}

/// Insert a published identity record directly into the DB.
/// Using `record_id == station_id == callsign` so that both the direct-ID lookup
/// and the station-id fallback in the CLI will resolve to the same record.
async fn seed_published_identity(pool: &sqlx::PgPool, id: &str) {
    sqlx::query(
        "INSERT INTO identity_records (record_id, station_id, callsign, publication_state)
         VALUES ($1, $2, $3, 'published')
         ON CONFLICT (record_id) DO NOTHING",
    )
    .bind(id)
    .bind(id)
    .bind(id)
    .execute(pool)
    .await
    .expect("failed to seed identity record");
}

/// Bind a random-port TCP listener, spawn the PKI router in the background,
/// and return the base URL for the server.
async fn spawn_live_server(pool: sqlx::PgPool) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind TCP listener");
    let addr = listener.local_addr().expect("no local addr");
    let router = build_router(AppState { db: pool });
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("axum server failed");
    });
    format!("http://127.0.0.1:{}", addr.port())
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// `trust show` against a live PKI server returns the published identity.
#[tokio::test]
async fn live_trust_show_returns_published_identity() {
    let Some(pool) = setup_pool().await else {
        return;
    };
    let peer_id = format!("LIVE-TRUST-{}", uuid_fragment());
    seed_published_identity(&pool, &peer_id).await;
    let base_url = spawn_live_server(pool).await;

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--pki-url",
        &base_url,
        "trust",
        "show",
        &peer_id,
        "--format",
        "json",
    ]);

    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("\"status\": \"ok\""))
        .stdout(predicate::str::contains(
            "\"publication_state\": \"published\"",
        ));
}

/// `session start` against a live PKI server succeeds and persists an audit event
/// in the `audit_events` table.
#[tokio::test]
async fn live_session_start_persists_audit_event() {
    let Some(pool) = setup_pool().await else {
        return;
    };
    let peer_id = format!("LIVE-SESS-{}", uuid_fragment());
    seed_published_identity(&pool, &peer_id).await;
    let base_url = spawn_live_server(pool.clone()).await;

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--pki-url",
        &base_url,
        "session",
        "start",
        "--peer",
        &peer_id,
        "--format",
        "json",
    ]);

    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("\"status\": \"ok\""))
        .stdout(predicate::str::contains("\"hpx_state\""));

    // Verify that the session audit event landed in the DB.
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE event_type = 'session.audit_recorded'
           AND entity_type = 'session'
           AND actor_identity = 'openpulse-cli'",
    )
    .fetch_one(&pool)
    .await
    .expect("audit_events query failed");

    assert!(
        count >= 1,
        "expected at least one session.audit_recorded event; got {count}"
    );
}

/// `diagnose handshake` against a live PKI server returns a verified handshake
/// and persists an audit event for the session.
#[tokio::test]
async fn live_diagnose_handshake_persists_audit_event() {
    let Some(pool) = setup_pool().await else {
        return;
    };
    let peer_id = format!("LIVE-DX-{}", uuid_fragment());
    seed_published_identity(&pool, &peer_id).await;
    let base_url = spawn_live_server(pool.clone()).await;

    let audit_count_before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE event_type = 'session.audit_recorded'",
    )
    .fetch_one(&pool)
    .await
    .expect("pre-count query failed");

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--pki-url",
        &base_url,
        "diagnose",
        "handshake",
        &peer_id,
        "--format",
        "json",
    ]);

    cmd.assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains(
            "\"reason_code\": \"verified_out_of_band\"",
        ));

    let audit_count_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE event_type = 'session.audit_recorded'",
    )
    .fetch_one(&pool)
    .await
    .expect("post-count query failed");

    assert!(
        audit_count_after > audit_count_before,
        "expected a new session.audit_recorded event after diagnose handshake; before={audit_count_before} after={audit_count_after}"
    );
}

/// `trust show` for a completely unknown peer returns exit code 2.
#[tokio::test]
async fn live_trust_show_unknown_peer_returns_exit_2() {
    let Some(pool) = setup_pool().await else {
        return;
    };
    let base_url = spawn_live_server(pool).await;
    let peer_id = format!("LIVE-UNKNOWN-{}", uuid_fragment());

    let mut cmd = Command::cargo_bin("openpulse").expect("binary should build");
    cmd.args([
        "--backend",
        "loopback",
        "--pki-url",
        &base_url,
        "trust",
        "show",
        &peer_id,
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

// ── utilities ─────────────────────────────────────────────────────────────────

/// Generate a short unique fragment to keep test-seeded IDs distinct per run.
fn uuid_fragment() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    // Mix in thread id for parallelism safety
    let tid = format!("{:?}", std::thread::current().id());
    format!("{ns:08X}-{}", &tid[..tid.len().min(6)])
}
