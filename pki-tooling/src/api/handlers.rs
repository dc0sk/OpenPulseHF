use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

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

pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, Json(ApiMessage { status: "ok", detail: "service healthy".to_string() }))
}

pub async fn get_identity(Path(record_id): Path<String>) -> impl IntoResponse {
    not_implemented(&format!("lookup identity by record_id={record_id}"))
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

pub async fn create_submission(Json(_req): Json<SubmissionRequest>) -> impl IntoResponse {
    let detail = format!(
        "create submission payload_type={} has_detached_signature={} payload_kind={}",
        _req.payload_type,
        _req.detached_signature.is_some(),
        json_kind(&_req.payload),
    );
    not_implemented(&detail)
}

pub async fn get_submission(Path(submission_id): Path<String>) -> impl IntoResponse {
    not_implemented(&format!("get submission by submission_id={submission_id}"))
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
