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

    let row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE entity_type = 'submission' AND entity_id = $1 AND event_type = 'submission.created'",
    )
    .bind(&submission_id)
    .fetch_one(&pool)
    .await
    .expect("failed to query audit_events");

    assert_eq!(row_count, 1);
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
}
