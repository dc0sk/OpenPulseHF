use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use pki_tooling::{build_router, run_migrations, AppState};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

async fn setup_pool() -> Option<sqlx::PgPool> {
    let database_url = match std::env::var("PKI_TEST_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            eprintln!("skipping integration test: PKI_TEST_DATABASE_URL is not set");
            return None;
        }
    };

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("failed to connect to PKI_TEST_DATABASE_URL");

    run_migrations(&pool)
        .await
        .expect("failed to run SQL migrations in integration test setup");

    Some(pool)
}

#[tokio::test]
async fn create_submission_and_read_back_records_audit_event() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    let app = build_router(AppState { db: pool.clone() });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/submissions")
        .header("content-type", "application/json")
        .header("x-request-id", "req-create-001")
        .body(Body::from(
            json!({
                "payload_type": "identity_bundle",
                "payload": { "station_id": "N0CALL" },
                "detached_signature": "fake-signature"
            })
            .to_string(),
        ))
        .expect("failed to build POST /submissions request");

    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::CREATED);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let created: Value = serde_json::from_slice(&body).expect("invalid JSON body");
    assert_eq!(
        created
            .get("submission_state")
            .and_then(Value::as_str)
            .expect("missing submission_state"),
        "pending"
    );
    let submission_id = created
        .get("submission_id")
        .and_then(Value::as_str)
        .expect("missing submission_id")
        .to_string();

    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/submissions/{submission_id}"))
        .body(Body::empty())
        .expect("failed to build GET /submission request");

    let get_res = app
        .clone()
        .oneshot(get_req)
        .await
        .expect("request should succeed");
    assert_eq!(get_res.status(), StatusCode::OK);

    let get_body = to_bytes(get_res.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let submission: Value = serde_json::from_slice(&get_body).expect("invalid JSON body");
    assert_eq!(
        submission
            .get("submission_id")
            .and_then(Value::as_str)
            .expect("missing submission_id"),
        submission_id
    );
    assert_eq!(
        submission
            .get("submission_state")
            .and_then(Value::as_str)
            .expect("missing submission_state"),
        "pending"
    );

    let row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE entity_type = 'submission' AND entity_id = $1 AND event_type = 'submission.created'",
    )
    .bind(&submission_id)
    .fetch_one(&pool)
    .await
    .expect("failed to query audit_events");

    assert_eq!(row_count, 1);

    let request_id: Option<String> = sqlx::query_scalar(
        "SELECT request_id FROM audit_events WHERE entity_type = 'submission' AND entity_id = $1 AND event_type = 'submission.created'",
    )
    .bind(&submission_id)
    .fetch_one(&pool)
    .await
    .expect("failed to query request_id from audit_events");
    assert_eq!(request_id.as_deref(), Some("req-create-001"));
}

#[tokio::test]
async fn moderation_decision_updates_submission_and_records_events() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    let app = build_router(AppState { db: pool.clone() });

    let create_req = Request::builder()
        .method("POST")
        .uri("/api/v1/submissions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "payload_type": "identity_bundle",
                "payload": { "station_id": "K1TEST" },
                "detached_signature": null
            })
            .to_string(),
        ))
        .expect("failed to build POST /submissions request");

    let create_res = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("request should succeed");
    assert_eq!(create_res.status(), StatusCode::CREATED);

    let create_body = to_bytes(create_res.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let created: Value = serde_json::from_slice(&create_body).expect("invalid JSON body");
    assert_eq!(
        created
            .get("submission_state")
            .and_then(Value::as_str)
            .expect("missing submission_state"),
        "pending"
    );
    let submission_id = created
        .get("submission_id")
        .and_then(Value::as_str)
        .expect("missing submission_id")
        .to_string();

    let moderate_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/moderation/{submission_id}/decision"))
        .header("content-type", "application/json")
        .header("x-request-id", "req-moderation-001")
        .body(Body::from(
            json!({
                "decision": "accept",
                "reason_code": "manual_review_ok",
                "reason_text": "accepted by integration test"
            })
            .to_string(),
        ))
        .expect("failed to build POST /moderation decision request");

    let moderate_res = app
        .clone()
        .oneshot(moderate_req)
        .await
        .expect("request should succeed");
    assert_eq!(moderate_res.status(), StatusCode::OK);

    let moderate_body = to_bytes(moderate_res.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let moderated: Value = serde_json::from_slice(&moderate_body).expect("invalid JSON body");
    assert_eq!(
        moderated
            .get("submission_id")
            .and_then(Value::as_str)
            .expect("missing submission_id"),
        submission_id
    );
    assert_eq!(
        moderated
            .get("submission_state")
            .and_then(Value::as_str)
            .expect("missing submission_state"),
        "accepted"
    );
    assert!(
        moderated
            .get("moderation_event_id")
            .and_then(Value::as_str)
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        "missing or empty moderation_event_id"
    );

    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/submissions/{submission_id}"))
        .body(Body::empty())
        .expect("failed to build GET /submission request");

    let get_res = app
        .clone()
        .oneshot(get_req)
        .await
        .expect("request should succeed");
    assert_eq!(get_res.status(), StatusCode::OK);

    let get_body = to_bytes(get_res.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let submission: Value = serde_json::from_slice(&get_body).expect("invalid JSON body");
    assert_eq!(
        submission
            .get("submission_state")
            .and_then(Value::as_str)
            .expect("missing submission_state"),
        "accepted"
    );
    assert_eq!(
        submission
            .get("moderation_reason_code")
            .and_then(Value::as_str)
            .expect("missing moderation_reason_code"),
        "manual_review_ok"
    );

    let moderation_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM moderation_events WHERE submission_id = $1",
    )
    .bind(&submission_id)
    .fetch_one(&pool)
    .await
    .expect("failed to query moderation_events");
    assert_eq!(moderation_count, 1);

    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE entity_type = 'submission' AND entity_id = $1 AND event_type = 'submission.moderated'",
    )
    .bind(&submission_id)
    .fetch_one(&pool)
    .await
    .expect("failed to query audit_events");
    assert_eq!(audit_count, 1);

    let request_id: Option<String> = sqlx::query_scalar(
        "SELECT request_id FROM audit_events WHERE entity_type = 'submission' AND entity_id = $1 AND event_type = 'submission.moderated'",
    )
    .bind(&submission_id)
    .fetch_one(&pool)
    .await
    .expect("failed to query request_id from audit_events");
    assert_eq!(request_id.as_deref(), Some("req-moderation-001"));
}

#[tokio::test]
async fn create_submission_rejects_empty_payload_type() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    let app = build_router(AppState { db: pool.clone() });

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/submissions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "payload_type": "   ",
                "payload": { "station_id": "N0FAIL" },
                "detached_signature": null
            })
            .to_string(),
        ))
        .expect("failed to build POST /submissions request");

    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let error: Value = serde_json::from_slice(&body).expect("invalid JSON error body");
    assert_eq!(
        error
            .get("status")
            .and_then(Value::as_str)
            .expect("missing status"),
        "validation_error"
    );
    assert_eq!(
        error
            .get("detail")
            .and_then(Value::as_str)
            .expect("missing detail"),
        "payload_type must not be empty"
    );
}

#[tokio::test]
async fn moderation_rejects_invalid_decision_value() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    let app = build_router(AppState { db: pool.clone() });

    let create_req = Request::builder()
        .method("POST")
        .uri("/api/v1/submissions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "payload_type": "identity_bundle",
                "payload": { "station_id": "K2BAD" },
                "detached_signature": null
            })
            .to_string(),
        ))
        .expect("failed to build POST /submissions request");

    let create_res = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("request should succeed");
    assert_eq!(create_res.status(), StatusCode::CREATED);

    let create_body = to_bytes(create_res.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let created: Value = serde_json::from_slice(&create_body).expect("invalid JSON body");
    let submission_id = created
        .get("submission_id")
        .and_then(Value::as_str)
        .expect("missing submission_id")
        .to_string();

    let moderate_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/moderation/{submission_id}/decision"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "decision": "maybe",
                "reason_code": "invalid",
                "reason_text": "invalid decision"
            })
            .to_string(),
        ))
        .expect("failed to build moderation request");

    let moderate_res = app
        .clone()
        .oneshot(moderate_req)
        .await
        .expect("request should succeed");
    assert_eq!(moderate_res.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(moderate_res.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let error: Value = serde_json::from_slice(&body).expect("invalid JSON error body");
    assert_eq!(
        error
            .get("status")
            .and_then(Value::as_str)
            .expect("missing status"),
        "validation_error"
    );
    assert_eq!(
        error
            .get("detail")
            .and_then(Value::as_str)
            .expect("missing detail"),
        "decision must be accept, reject, or quarantine"
    );
}

#[tokio::test]
async fn moderation_returns_not_found_for_missing_submission() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    let app = build_router(AppState { db: pool.clone() });

    let moderate_req = Request::builder()
        .method("POST")
        .uri("/api/v1/moderation/non-existent-submission/decision")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "decision": "accept",
                "reason_code": "manual_review_ok",
                "reason_text": "attempting to moderate missing submission"
            })
            .to_string(),
        ))
        .expect("failed to build moderation request");

    let moderate_res = app
        .clone()
        .oneshot(moderate_req)
        .await
        .expect("request should succeed");
    assert_eq!(moderate_res.status(), StatusCode::NOT_FOUND);

    let body = to_bytes(moderate_res.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let error: Value = serde_json::from_slice(&body).expect("invalid JSON error body");
    assert_eq!(
        error
            .get("status")
            .and_then(Value::as_str)
            .expect("missing status"),
        "not_found"
    );
    assert_eq!(
        error
            .get("detail")
            .and_then(Value::as_str)
            .expect("missing detail"),
        "submission not found"
    );
}

#[tokio::test]
async fn get_submission_returns_not_found_for_missing_submission() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    let app = build_router(AppState { db: pool.clone() });

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/submissions/non-existent-submission")
        .body(Body::empty())
        .expect("failed to build GET /submission request");

    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let error: Value = serde_json::from_slice(&body).expect("invalid JSON error body");
    assert_eq!(
        error
            .get("status")
            .and_then(Value::as_str)
            .expect("missing status"),
        "not_found"
    );
    assert_eq!(
        error
            .get("detail")
            .and_then(Value::as_str)
            .expect("missing detail"),
        "submission not found"
    );
}

#[tokio::test]
async fn list_revocations_filters_by_record_id_and_issuer() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    sqlx::query(
        "INSERT INTO identity_records (
            record_id,
            station_id,
            callsign,
            publication_state
         ) VALUES ($1, $2, $3, $4)",
    )
    .bind("record-rev-a")
    .bind("STN-REV-A")
    .bind("N0REVA")
    .bind("published")
    .execute(&pool)
    .await
    .expect("failed to insert identity record A");

    sqlx::query(
        "INSERT INTO identity_records (
            record_id,
            station_id,
            callsign,
            publication_state
         ) VALUES ($1, $2, $3, $4)",
    )
    .bind("record-rev-b")
    .bind("STN-REV-B")
    .bind("N0REVB")
    .bind("published")
    .execute(&pool)
    .await
    .expect("failed to insert identity record B");

    sqlx::query(
        "INSERT INTO revocations (
            revocation_id,
            record_id,
            revision_id,
            key_id,
            issuer_identity,
            reason_code,
            effective_at
         ) VALUES ($1, $2, $3, $4, $5, $6, NOW() - INTERVAL '1 day')",
    )
    .bind("revocation-a")
    .bind("record-rev-a")
    .bind(Option::<String>::None)
    .bind(Option::<String>::None)
    .bind("issuer-alpha")
    .bind("key_compromise")
    .execute(&pool)
    .await
    .expect("failed to insert revocation A");

    sqlx::query(
        "INSERT INTO revocations (
            revocation_id,
            record_id,
            revision_id,
            key_id,
            issuer_identity,
            reason_code,
            effective_at
         ) VALUES ($1, $2, $3, $4, $5, $6, NOW())",
    )
    .bind("revocation-b")
    .bind("record-rev-b")
    .bind(Option::<String>::None)
    .bind(Option::<String>::None)
    .bind("issuer-beta")
    .bind("operator_request")
    .execute(&pool)
    .await
    .expect("failed to insert revocation B");

    let app = build_router(AppState { db: pool.clone() });
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/revocations?record_id=record-rev-a&issuer_id=issuer-alpha")
        .body(Body::empty())
        .expect("failed to build GET /revocations request");

    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let rows: Value = serde_json::from_slice(&body).expect("invalid JSON body");
    let array = rows.as_array().expect("revocation response must be an array");
    assert_eq!(array.len(), 1);
    assert_eq!(
        array[0]
            .get("revocation_id")
            .and_then(Value::as_str)
            .expect("missing revocation_id"),
        "revocation-a"
    );
}

#[tokio::test]
async fn list_revocations_rejects_invalid_rfc3339_filters() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    let app = build_router(AppState { db: pool.clone() });
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/revocations?effective_before=not-a-date")
        .body(Body::empty())
        .expect("failed to build GET /revocations request");

    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let error: Value = serde_json::from_slice(&body).expect("invalid JSON error body");
    assert_eq!(
        error
            .get("status")
            .and_then(Value::as_str)
            .expect("missing status"),
        "validation_error"
    );
    assert_eq!(
        error
            .get("detail")
            .and_then(Value::as_str)
            .expect("missing detail"),
        "effective_before must be RFC3339 timestamp"
    );
}

#[tokio::test]
async fn list_revocations_rejects_inverted_time_window() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    let app = build_router(AppState { db: pool.clone() });
    let req = Request::builder()
        .method("GET")
        .uri(
            "/api/v1/revocations?effective_after=2026-01-03T00:00:00Z&effective_before=2026-01-01T00:00:00Z",
        )
        .body(Body::empty())
        .expect("failed to build GET /revocations request");

    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let error: Value = serde_json::from_slice(&body).expect("invalid JSON error body");
    assert_eq!(
        error
            .get("status")
            .and_then(Value::as_str)
            .expect("missing status"),
        "validation_error"
    );
    assert_eq!(
        error
            .get("detail")
            .and_then(Value::as_str)
            .expect("missing detail"),
        "effective_after must be less than or equal to effective_before"
    );
}

#[tokio::test]
async fn list_revocations_applies_effective_time_filters() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    sqlx::query(
        "INSERT INTO identity_records (
            record_id,
            station_id,
            callsign,
            publication_state
         ) VALUES ($1, $2, $3, $4)",
    )
    .bind("record-rev-filter")
    .bind("STN-REV-F")
    .bind("N0REVF")
    .bind("published")
    .execute(&pool)
    .await
    .expect("failed to insert filter identity record");

    sqlx::query(
        "INSERT INTO revocations (
            revocation_id,
            record_id,
            revision_id,
            key_id,
            issuer_identity,
            reason_code,
            effective_at
         ) VALUES
            ($1, $4, NULL, NULL, $5, $6, $7::timestamptz),
            ($2, $4, NULL, NULL, $5, $6, $8::timestamptz),
            ($3, $4, NULL, NULL, $5, $6, $9::timestamptz)",
    )
    .bind("rev-filter-1")
    .bind("rev-filter-2")
    .bind("rev-filter-3")
    .bind("record-rev-filter")
    .bind("issuer-filter")
    .bind("operator_request")
    .bind("2026-01-01T00:00:00Z")
    .bind("2026-01-02T00:00:00Z")
    .bind("2026-01-03T00:00:00Z")
    .execute(&pool)
    .await
    .expect("failed to insert filter revocations");

    let app = build_router(AppState { db: pool.clone() });
    let req = Request::builder()
        .method("GET")
        .uri(
            "/api/v1/revocations?record_id=record-rev-filter&effective_after=2026-01-02T00:00:00Z&effective_before=2026-01-03T00:00:00Z",
        )
        .body(Body::empty())
        .expect("failed to build GET /revocations request");

    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let rows: Value = serde_json::from_slice(&body).expect("invalid JSON body");
    let array = rows.as_array().expect("revocation response must be an array");
    assert_eq!(array.len(), 2);
    assert_eq!(
        array[0]
            .get("revocation_id")
            .and_then(Value::as_str)
            .expect("missing revocation_id"),
        "rev-filter-3"
    );
    assert_eq!(
        array[1]
            .get("revocation_id")
            .and_then(Value::as_str)
            .expect("missing revocation_id"),
        "rev-filter-2"
    );
}

#[tokio::test]
async fn list_revocations_uses_stable_tiebreak_ordering() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    sqlx::query(
        "INSERT INTO identity_records (
            record_id,
            station_id,
            callsign,
            publication_state
         ) VALUES ($1, $2, $3, $4)",
    )
    .bind("record-rev-order")
    .bind("STN-REV-O")
    .bind("N0REVO")
    .bind("published")
    .execute(&pool)
    .await
    .expect("failed to insert ordering identity record");

    sqlx::query(
        "INSERT INTO revocations (
            revocation_id,
            record_id,
            revision_id,
            key_id,
            issuer_identity,
            reason_code,
            effective_at,
            created_at
         ) VALUES
            ($1, $3, NULL, NULL, $4, $5, $6::timestamptz, $7::timestamptz),
            ($2, $3, NULL, NULL, $4, $5, $6::timestamptz, $7::timestamptz)",
    )
    .bind("rev-order-b")
    .bind("rev-order-a")
    .bind("record-rev-order")
    .bind("issuer-order")
    .bind("operator_request")
    .bind("2026-01-04T00:00:00Z")
    .bind("2026-01-05T00:00:00Z")
    .execute(&pool)
    .await
    .expect("failed to insert ordering revocations");

    let app = build_router(AppState { db: pool.clone() });
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/revocations?record_id=record-rev-order")
        .body(Body::empty())
        .expect("failed to build GET /revocations request");

    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let rows: Value = serde_json::from_slice(&body).expect("invalid JSON body");
    let array = rows.as_array().expect("revocation response must be an array");
    assert_eq!(array.len(), 2);
    assert_eq!(
        array[0]
            .get("revocation_id")
            .and_then(Value::as_str)
            .expect("missing revocation_id"),
        "rev-order-a"
    );
    assert_eq!(
        array[1]
            .get("revocation_id")
            .and_then(Value::as_str)
            .expect("missing revocation_id"),
        "rev-order-b"
    );
}

#[tokio::test]
async fn list_revocations_filters_by_key_fingerprint() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    sqlx::query(
        "INSERT INTO identity_records (
            record_id,
            station_id,
            callsign,
            current_revision_id,
            publication_state
         ) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind("record-fp-1")
    .bind("STN-FP-1")
    .bind("N0FP1")
    .bind("rev-fp-1")
    .bind("published")
    .execute(&pool)
    .await
    .expect("failed to insert fingerprint identity record 1");

    sqlx::query(
        "INSERT INTO identity_records (
            record_id,
            station_id,
            callsign,
            current_revision_id,
            publication_state
         ) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind("record-fp-2")
    .bind("STN-FP-2")
    .bind("N0FP2")
    .bind("rev-fp-2")
    .bind("published")
    .execute(&pool)
    .await
    .expect("failed to insert fingerprint identity record 2");

    sqlx::query(
        "INSERT INTO identity_revisions (
            revision_id,
            record_id,
            revision_number,
            valid_from,
            valid_until,
            submitted_via,
            submission_id,
            algorithms_json
         ) VALUES ($1, $2, $3, $4::timestamptz, $5, $6, $7, $8)",
    )
    .bind("rev-fp-1")
    .bind("record-fp-1")
    .bind(1_i32)
    .bind("2026-01-01T00:00:00Z")
    .bind(Option::<String>::None)
    .bind("api")
    .bind(Option::<String>::None)
    .bind(json!(["ed25519"]))
    .execute(&pool)
    .await
    .expect("failed to insert revision 1");

    sqlx::query(
        "INSERT INTO identity_revisions (
            revision_id,
            record_id,
            revision_number,
            valid_from,
            valid_until,
            submitted_via,
            submission_id,
            algorithms_json
         ) VALUES ($1, $2, $3, $4::timestamptz, $5, $6, $7, $8)",
    )
    .bind("rev-fp-2")
    .bind("record-fp-2")
    .bind(1_i32)
    .bind("2026-01-01T00:00:00Z")
    .bind(Option::<String>::None)
    .bind("api")
    .bind(Option::<String>::None)
    .bind(json!(["ed25519"]))
    .execute(&pool)
    .await
    .expect("failed to insert revision 2");

    sqlx::query(
        "INSERT INTO identity_keys (
            revision_id,
            key_id,
            algorithm,
            public_key,
            fingerprint,
            key_status
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind("rev-fp-1")
    .bind("key-fp-1")
    .bind("ed25519")
    .bind("pk-fp-1")
    .bind("FP:TARGET")
    .bind("active")
    .execute(&pool)
    .await
    .expect("failed to insert key 1");

    sqlx::query(
        "INSERT INTO identity_keys (
            revision_id,
            key_id,
            algorithm,
            public_key,
            fingerprint,
            key_status
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind("rev-fp-2")
    .bind("key-fp-2")
    .bind("ed25519")
    .bind("pk-fp-2")
    .bind("FP:OTHER")
    .bind("active")
    .execute(&pool)
    .await
    .expect("failed to insert key 2");

    sqlx::query(
        "INSERT INTO revocations (
            revocation_id,
            record_id,
            revision_id,
            key_id,
            issuer_identity,
            reason_code,
            effective_at
         ) VALUES
            ($1, $4, $6, $2, $7, $8, $9::timestamptz),
            ($3, $5, $10, $11, $7, $8, $12::timestamptz)",
    )
    .bind("rev-fp-match")
    .bind("key-fp-1")
    .bind("rev-fp-nonmatch")
    .bind("record-fp-1")
    .bind("record-fp-2")
    .bind("rev-fp-1")
    .bind("issuer-fp")
    .bind("key_compromise")
    .bind("2026-01-06T00:00:00Z")
    .bind("rev-fp-2")
    .bind("key-fp-2")
    .bind("2026-01-07T00:00:00Z")
    .execute(&pool)
    .await
    .expect("failed to insert fingerprint revocations");

    let app = build_router(AppState { db: pool.clone() });
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/revocations?fingerprint=FP:TARGET")
        .body(Body::empty())
        .expect("failed to build GET /revocations request");

    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let rows: Value = serde_json::from_slice(&body).expect("invalid JSON body");
    let array = rows.as_array().expect("revocation response must be an array");
    assert_eq!(array.len(), 1);
    assert_eq!(
        array[0]
            .get("revocation_id")
            .and_then(Value::as_str)
            .expect("missing revocation_id"),
        "rev-fp-match"
    );
}

#[tokio::test]
async fn list_revocations_clamps_limit_boundaries() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    sqlx::query(
        "INSERT INTO identity_records (
            record_id,
            station_id,
            callsign,
            publication_state
         ) VALUES ($1, $2, $3, $4)",
    )
    .bind("record-rev-limit")
    .bind("STN-REV-L")
    .bind("N0REVL")
    .bind("published")
    .execute(&pool)
    .await
    .expect("failed to insert limit identity record");

    sqlx::query(
        "INSERT INTO revocations (
            revocation_id,
            record_id,
            revision_id,
            key_id,
            issuer_identity,
            reason_code,
            effective_at
         ) VALUES
            ($1, $3, NULL, NULL, $4, $5, $6::timestamptz),
            ($2, $3, NULL, NULL, $4, $5, $7::timestamptz)",
    )
    .bind("rev-limit-1")
    .bind("rev-limit-2")
    .bind("record-rev-limit")
    .bind("issuer-limit")
    .bind("operator_request")
    .bind("2026-01-08T00:00:00Z")
    .bind("2026-01-09T00:00:00Z")
    .execute(&pool)
    .await
    .expect("failed to insert limit revocations");

    let app = build_router(AppState { db: pool.clone() });

    let min_req = Request::builder()
        .method("GET")
        .uri("/api/v1/revocations?record_id=record-rev-limit&limit=0")
        .body(Body::empty())
        .expect("failed to build min limit request");
    let min_res = app
        .clone()
        .oneshot(min_req)
        .await
        .expect("request should succeed");
    assert_eq!(min_res.status(), StatusCode::OK);
    let min_body = to_bytes(min_res.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let min_rows: Value = serde_json::from_slice(&min_body).expect("invalid JSON body");
    assert_eq!(min_rows.as_array().expect("expected array").len(), 1);

    let max_req = Request::builder()
        .method("GET")
        .uri("/api/v1/revocations?record_id=record-rev-limit&limit=999")
        .body(Body::empty())
        .expect("failed to build max limit request");
    let max_res = app
        .clone()
        .oneshot(max_req)
        .await
        .expect("request should succeed");
    assert_eq!(max_res.status(), StatusCode::OK);
    let max_body = to_bytes(max_res.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let max_rows: Value = serde_json::from_slice(&max_body).expect("invalid JSON body");
    assert_eq!(max_rows.as_array().expect("expected array").len(), 2);
}

#[tokio::test]
async fn list_revocations_defaults_limit_to_100() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    sqlx::query(
        "INSERT INTO identity_records (
            record_id,
            station_id,
            callsign,
            publication_state
         ) VALUES ($1, $2, $3, $4)",
    )
    .bind("record-rev-default")
    .bind("STN-REV-D")
    .bind("N0REVD")
    .bind("published")
    .execute(&pool)
    .await
    .expect("failed to insert default-limit identity record");

    sqlx::query(
        "INSERT INTO revocations (
            revocation_id,
            record_id,
            revision_id,
            key_id,
            issuer_identity,
            reason_code,
            effective_at
        )
        SELECT
            'rev-default-' || gs::text,
            $1,
            NULL,
            NULL,
            'issuer-default',
            'operator_request',
            TIMESTAMPTZ '2026-02-01T00:00:00Z' + (gs || ' minutes')::interval
        FROM generate_series(1, 101) AS gs",
    )
    .bind("record-rev-default")
    .execute(&pool)
    .await
    .expect("failed to insert default-limit revocations");

    let app = build_router(AppState { db: pool.clone() });
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/revocations?record_id=record-rev-default")
        .body(Body::empty())
        .expect("failed to build default-limit request");

    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let rows: Value = serde_json::from_slice(&body).expect("invalid JSON body");
    let array = rows.as_array().expect("expected array");
    assert_eq!(array.len(), 100);
    assert_eq!(
        array[0]
            .get("revocation_id")
            .and_then(Value::as_str)
            .expect("missing revocation_id"),
        "rev-default-101"
    );
}

#[tokio::test]
async fn list_revocations_combines_fingerprint_issuer_and_time_filters() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    sqlx::query(
        "INSERT INTO identity_records (
            record_id,
            station_id,
            callsign,
            current_revision_id,
            publication_state
         ) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind("record-rev-combined")
    .bind("STN-REV-C")
    .bind("N0REVC")
    .bind("rev-combined-1")
    .bind("published")
    .execute(&pool)
    .await
    .expect("failed to insert combined-filter identity record");

    sqlx::query(
        "INSERT INTO identity_revisions (
            revision_id,
            record_id,
            revision_number,
            valid_from,
            valid_until,
            submitted_via,
            submission_id,
            algorithms_json
         ) VALUES ($1, $2, $3, $4::timestamptz, $5, $6, $7, $8)",
    )
    .bind("rev-combined-1")
    .bind("record-rev-combined")
    .bind(1_i32)
    .bind("2026-03-01T00:00:00Z")
    .bind(Option::<String>::None)
    .bind("api")
    .bind(Option::<String>::None)
    .bind(json!(["ed25519"]))
    .execute(&pool)
    .await
    .expect("failed to insert combined-filter revision");

    sqlx::query(
        "INSERT INTO identity_keys (
            revision_id,
            key_id,
            algorithm,
            public_key,
            fingerprint,
            key_status
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind("rev-combined-1")
    .bind("key-combined-1")
    .bind("ed25519")
    .bind("pk-combined-1")
    .bind("FP:COMBINED")
    .bind("active")
    .execute(&pool)
    .await
    .expect("failed to insert combined-filter key");

    sqlx::query(
        "INSERT INTO revocations (
            revocation_id,
            record_id,
            revision_id,
            key_id,
            issuer_identity,
            reason_code,
            effective_at
         ) VALUES
            ($1, $4, $5, $2, $6, $7, $8::timestamptz),
            ($3, $4, $5, $2, $9, $7, $10::timestamptz),
            ($11, $4, $5, $2, $6, $7, $12::timestamptz)",
    )
    .bind("rev-combined-match")
    .bind("key-combined-1")
    .bind("rev-combined-wrong-issuer")
    .bind("record-rev-combined")
    .bind("rev-combined-1")
    .bind("issuer-combined")
    .bind("key_compromise")
    .bind("2026-03-02T00:00:00Z")
    .bind("issuer-other")
    .bind("2026-03-02T12:00:00Z")
    .bind("rev-combined-outside-window")
    .bind("2026-03-05T00:00:00Z")
    .execute(&pool)
    .await
    .expect("failed to insert combined-filter revocations");

    let app = build_router(AppState { db: pool.clone() });
    let req = Request::builder()
        .method("GET")
        .uri(
            "/api/v1/revocations?fingerprint=FP:COMBINED&issuer_id=issuer-combined&effective_after=2026-03-01T00:00:00Z&effective_before=2026-03-03T00:00:00Z",
        )
        .body(Body::empty())
        .expect("failed to build combined-filter request");

    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let rows: Value = serde_json::from_slice(&body).expect("invalid JSON body");
    let array = rows.as_array().expect("expected array");
    assert_eq!(array.len(), 1);
    assert_eq!(
        array[0]
            .get("revocation_id")
            .and_then(Value::as_str)
            .expect("missing revocation_id"),
        "rev-combined-match"
    );
}

#[tokio::test]
async fn trust_bundle_endpoints_return_current_and_specific_bundle() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    sqlx::query(
        "INSERT INTO trust_bundles (
            bundle_id,
            schema_version,
            generated_at,
            issuer_instance_id,
            signing_algorithms,
            records,
            bundle_signature,
            is_current
         ) VALUES ($1, $2, NOW() - INTERVAL '1 hour', $3, $4, $5, $6, $7)",
    )
    .bind("bundle-old")
    .bind("1.0.0")
    .bind("issuer-node-a")
    .bind(json!(["ed25519"]))
    .bind(json!([]))
    .bind("sig-old")
    .bind(false)
    .execute(&pool)
    .await
    .expect("failed to insert old trust bundle");

    sqlx::query(
        "INSERT INTO trust_bundles (
            bundle_id,
            schema_version,
            generated_at,
            issuer_instance_id,
            signing_algorithms,
            records,
            bundle_signature,
            is_current
         ) VALUES ($1, $2, NOW(), $3, $4, $5, $6, $7)",
    )
    .bind("bundle-current")
    .bind("1.0.0")
    .bind("issuer-node-a")
    .bind(json!(["ed25519", "dilithium3"]))
    .bind(json!([
        {
            "record_id": "record-1",
            "station_id": "STN-1",
            "callsign": "N0CALL",
            "trust_state": "trusted",
            "revocation_state": "active",
            "algorithms": ["ed25519"],
            "keys": [],
            "hybrid_policy": "recommended",
            "valid_from": "2026-01-01T00:00:00Z",
            "valid_until": null,
            "evidence_summary": []
        }
    ]))
    .bind("sig-current")
    .bind(true)
    .execute(&pool)
    .await
    .expect("failed to insert current trust bundle");

    let app = build_router(AppState { db: pool.clone() });

    let current_req = Request::builder()
        .method("GET")
        .uri("/api/v1/trust-bundles/current")
        .body(Body::empty())
        .expect("failed to build GET /trust-bundles/current request");

    let current_res = app
        .clone()
        .oneshot(current_req)
        .await
        .expect("request should succeed");
    assert_eq!(current_res.status(), StatusCode::OK);

    let current_body = to_bytes(current_res.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let current: Value = serde_json::from_slice(&current_body).expect("invalid JSON body");
    assert_eq!(
        current
            .get("bundle_id")
            .and_then(Value::as_str)
            .expect("missing bundle_id"),
        "bundle-current"
    );
    assert_eq!(
        current
            .get("bundle_signature")
            .and_then(Value::as_str)
            .expect("missing bundle_signature"),
        "sig-current"
    );

    let by_id_req = Request::builder()
        .method("GET")
        .uri("/api/v1/trust-bundles/bundle-old")
        .body(Body::empty())
        .expect("failed to build GET /trust-bundles/{bundle_id} request");

    let by_id_res = app
        .clone()
        .oneshot(by_id_req)
        .await
        .expect("request should succeed");
    assert_eq!(by_id_res.status(), StatusCode::OK);

    let by_id_body = to_bytes(by_id_res.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let bundle: Value = serde_json::from_slice(&by_id_body).expect("invalid JSON body");
    assert_eq!(
        bundle
            .get("bundle_id")
            .and_then(Value::as_str)
            .expect("missing bundle_id"),
        "bundle-old"
    );

    let missing_req = Request::builder()
        .method("GET")
        .uri("/api/v1/trust-bundles/missing-bundle")
        .body(Body::empty())
        .expect("failed to build missing bundle request");

    let missing_res = app
        .clone()
        .oneshot(missing_req)
        .await
        .expect("request should succeed");
    assert_eq!(missing_res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_revocations_returns_empty_when_composed_filter_excludes_all() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    sqlx::query(
        "INSERT INTO identity_records (
            record_id,
            station_id,
            callsign,
            current_revision_id,
            publication_state
         ) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind("record-empty-filter")
    .bind("STN-EMPTY-F")
    .bind("N0EMPTYF")
    .bind("rev-empty-1")
    .bind("published")
    .execute(&pool)
    .await
    .expect("failed to insert empty-filter identity record");

    sqlx::query(
        "INSERT INTO identity_revisions (
            revision_id,
            record_id,
            revision_number,
            valid_from,
            valid_until,
            submitted_via,
            submission_id,
            algorithms_json
         ) VALUES ($1, $2, $3, $4::timestamptz, $5, $6, $7, $8)",
    )
    .bind("rev-empty-1")
    .bind("record-empty-filter")
    .bind(1_i32)
    .bind("2026-03-01T00:00:00Z")
    .bind(Option::<String>::None)
    .bind("api")
    .bind(Option::<String>::None)
    .bind(json!(["ed25519"]))
    .execute(&pool)
    .await
    .expect("failed to insert empty-filter revision");

    sqlx::query(
        "INSERT INTO identity_keys (
            revision_id,
            key_id,
            algorithm,
            public_key,
            fingerprint,
            key_status
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind("rev-empty-1")
    .bind("key-empty-1")
    .bind("ed25519")
    .bind("pk-empty-1")
    .bind("FP:EMPTY-MATCH")
    .bind("active")
    .execute(&pool)
    .await
    .expect("failed to insert empty-filter key");

    sqlx::query(
        "INSERT INTO revocations (
            revocation_id,
            record_id,
            revision_id,
            key_id,
            issuer_identity,
            reason_code,
            effective_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7::timestamptz)",
    )
    .bind("rev-empty-wrong-issuer")
    .bind("record-empty-filter")
    .bind("rev-empty-1")
    .bind("key-empty-1")
    .bind("issuer-different")
    .bind("key_compromise")
    .bind("2026-03-02T00:00:00Z")
    .execute(&pool)
    .await
    .expect("failed to insert empty-filter revocation with wrong issuer");

    let app = build_router(AppState { db: pool.clone() });

    // Query with fingerprint and time window matching, but wrong issuer_id
    let req = Request::builder()
        .method("GET")
        .uri(
            "/api/v1/revocations?fingerprint=FP:EMPTY-MATCH&issuer_id=issuer-expected&effective_after=2026-03-01T00:00:00Z&effective_before=2026-03-03T00:00:00Z",
        )
        .body(Body::empty())
        .expect("failed to build empty-filter request");

    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK, "should return 200 even with no matches");

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let rows: Value = serde_json::from_slice(&body).expect("invalid JSON body");
    let array = rows.as_array().expect("expected array response");
    assert_eq!(array.len(), 0, "composed filters should exclude all when one filter has no matches");
}

#[tokio::test]
async fn list_revocations_limit_one_returns_deterministic_first_result() {
    let Some(pool) = setup_pool().await else {
        return;
    };

    sqlx::query(
        "INSERT INTO identity_records (
            record_id,
            station_id,
            callsign,
            current_revision_id,
            publication_state
         ) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind("record-limit-one")
    .bind("STN-LIMIT-1")
    .bind("N0LIMIT1")
    .bind("rev-limit-1")
    .bind("published")
    .execute(&pool)
    .await
    .expect("failed to insert limit-one identity record");

    sqlx::query(
        "INSERT INTO identity_revisions (
            revision_id,
            record_id,
            revision_number,
            valid_from,
            valid_until,
            submitted_via,
            submission_id,
            algorithms_json
         ) VALUES ($1, $2, $3, $4::timestamptz, $5, $6, $7, $8)",
    )
    .bind("rev-limit-1")
    .bind("record-limit-one")
    .bind(1_i32)
    .bind("2026-03-01T00:00:00Z")
    .bind(Option::<String>::None)
    .bind("api")
    .bind(Option::<String>::None)
    .bind(json!(["ed25519"]))
    .execute(&pool)
    .await
    .expect("failed to insert limit-one revision");

    sqlx::query(
        "INSERT INTO identity_keys (
            revision_id,
            key_id,
            algorithm,
            public_key,
            fingerprint,
            key_status
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind("rev-limit-1")
    .bind("key-limit-1")
    .bind("ed25519")
    .bind("pk-limit-1")
    .bind("FP:LIMIT-ONE")
    .bind("active")
    .execute(&pool)
    .await
    .expect("failed to insert limit-one key");

    sqlx::query(
        "INSERT INTO revocations (
            revocation_id,
            record_id,
            revision_id,
            key_id,
            issuer_identity,
            reason_code,
            effective_at
         ) VALUES
            ($1, $4, $5, $2, $6, $7, $8::timestamptz),
            ($3, $4, $5, $2, $6, $7, $9::timestamptz),
            ($10, $4, $5, $2, $6, $7, $11::timestamptz)",
    )
    .bind("rev-limit-oldest")
    .bind("key-limit-1")
    .bind("rev-limit-middle")
    .bind("record-limit-one")
    .bind("rev-limit-1")
    .bind("issuer-limit-one")
    .bind("key_compromise")
    .bind("2026-03-01T08:00:00Z")
    .bind("2026-03-02T10:00:00Z")
    .bind("rev-limit-newest")
    .bind("2026-03-03T12:00:00Z")
    .execute(&pool)
    .await
    .expect("failed to insert limit-one revocations");

    let app = build_router(AppState { db: pool.clone() });

    // Query with limit=1 and composed filters
    let req = Request::builder()
        .method("GET")
        .uri(
            "/api/v1/revocations?fingerprint=FP:LIMIT-ONE&issuer_id=issuer-limit-one&effective_after=2026-03-01T00:00:00Z&effective_before=2026-03-04T00:00:00Z&limit=1",
        )
        .body(Body::empty())
        .expect("failed to build limit-one request");

    let response = app
        .clone()
        .oneshot(req)
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let rows: Value = serde_json::from_slice(&body).expect("invalid JSON body");
    let array = rows.as_array().expect("expected array response");
    assert_eq!(array.len(), 1, "limit=1 should return exactly one result");

    let result = array[0].as_object().expect("expected object");
    let revocation_id = result
        .get("revocation_id")
        .and_then(Value::as_str)
        .expect("missing revocation_id");

    // Should be the newest (highest effective_at: 2026-03-03T12:00:00Z)
    assert_eq!(
        revocation_id, "rev-limit-newest",
        "limit=1 with DESC ordering should return newest effective_at first"
    );
}
