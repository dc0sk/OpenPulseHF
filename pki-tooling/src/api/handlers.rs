use crate::AppState;
use axum::extract::State;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Serialize)]
struct ApiMessage {
    status: &'static str,
    detail: String,
}

#[derive(Deserialize)]
pub struct SubmissionRequest {
    pub payload_type: String,
    pub payload: serde_json::Value,
    pub detached_signature: Option<String>,
}

#[derive(Deserialize)]
pub struct ModerationDecisionRequest {
    pub decision: String,
    pub reason_code: String,
    pub reason_text: String,
}

#[derive(Serialize, FromRow)]
struct IdentityRecordResponse {
    record_id: String,
    station_id: String,
    callsign: String,
    publication_state: String,
    current_revision_id: Option<String>,
}

#[derive(Serialize)]
struct SubmissionResponse {
    submission_id: String,
    submission_state: &'static str,
}

#[derive(Serialize, FromRow)]
struct SubmissionRecordResponse {
    submission_id: String,
    submitter_identity: String,
    submission_state: String,
    artifact_uri: String,
    detached_signature_uri: Option<String>,
    validation_summary: serde_json::Value,
    moderation_reason_code: Option<String>,
}

pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, Json(ApiMessage { status: "ok", detail: "service healthy".to_string() }))
}

pub async fn get_identity(
    State(state): State<AppState>,
    Path(record_id): Path<String>,
) -> impl IntoResponse {
    let result = sqlx::query_as::<_, IdentityRecordResponse>(
        "SELECT record_id, station_id, callsign, publication_state, current_revision_id
         FROM identity_records
         WHERE record_id = $1",
    )
    .bind(record_id)
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some(identity)) => (StatusCode::OK, Json(identity)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiMessage {
                status: "not_found",
                detail: "identity record not found".to_string(),
            }),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("database query failed: {err}"),
            }),
        )
            .into_response(),
    }
}

pub async fn lookup_identity() -> impl IntoResponse {
    not_implemented("identity lookup with filters")
}

pub async fn list_revocations() -> impl IntoResponse {
    not_implemented("list revocation records")
}

pub async fn get_current_trust_bundle() -> impl IntoResponse {
    not_implemented("fetch current trust bundle")
}

pub async fn get_trust_bundle(Path(bundle_id): Path<String>) -> impl IntoResponse {
    not_implemented(&format!("fetch trust bundle by bundle_id={bundle_id}"))
}

pub async fn create_submission(
    State(state): State<AppState>,
    Json(req): Json<SubmissionRequest>,
) -> impl IntoResponse {
    if req.payload_type.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiMessage {
                status: "validation_error",
                detail: "payload_type must not be empty".to_string(),
            }),
        )
            .into_response();
    }

    let submission_id = Uuid::new_v4().to_string();
    let artifact_uri = format!("inline://submission/{submission_id}");
    let detached_signature_uri = req
        .detached_signature
        .as_ref()
        .map(|_| format!("inline://submission/{submission_id}/detached-signature"));

    let validation_summary = serde_json::json!({
        "status": "pending_validation",
        "payload_type": req.payload_type,
        "payload_kind": json_kind(&req.payload),
        "has_detached_signature": req.detached_signature.is_some()
    });

    let insert = sqlx::query(
        "INSERT INTO submissions (
            submission_id,
            submitter_identity,
            submission_state,
            artifact_uri,
            detached_signature_uri,
            validation_summary
        ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&submission_id)
    .bind("api:anonymous")
    .bind("pending")
    .bind(&artifact_uri)
    .bind(detached_signature_uri)
    .bind(validation_summary)
    .execute(&state.db)
    .await;

    match insert {
        Ok(_) => (
            StatusCode::CREATED,
            Json(SubmissionResponse {
                submission_id,
                submission_state: "pending",
            }),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("failed to persist submission: {err}"),
            }),
        )
            .into_response(),
    }
}

pub async fn get_submission(
    State(state): State<AppState>,
    Path(submission_id): Path<String>,
) -> impl IntoResponse {
    let result = sqlx::query_as::<_, SubmissionRecordResponse>(
        "SELECT
            submission_id,
            submitter_identity,
            submission_state,
            artifact_uri,
            detached_signature_uri,
            validation_summary,
            moderation_reason_code
         FROM submissions
         WHERE submission_id = $1",
    )
    .bind(submission_id)
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some(submission)) => (StatusCode::OK, Json(submission)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiMessage {
                status: "not_found",
                detail: "submission not found".to_string(),
            }),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("database query failed: {err}"),
            }),
        )
            .into_response(),
    }
}

pub async fn get_moderation_queue() -> impl IntoResponse {
    not_implemented("get moderation queue")
}

pub async fn post_moderation_decision(
    Path(submission_id): Path<String>,
    Json(_req): Json<ModerationDecisionRequest>,
) -> impl IntoResponse {
    let detail = format!(
        "post moderation decision for submission_id={submission_id} decision={} reason_code={} reason_text_len={}",
        _req.decision,
        _req.reason_code,
        _req.reason_text.len(),
    );
    not_implemented(&detail)
}

fn not_implemented(detail: &str) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ApiMessage {
            status: "not_implemented",
            detail: detail.to_string(),
        }),
    )
}

fn json_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}
