use crate::AppState;
use axum::http::HeaderMap;
use axum::extract::State;
use axum::extract::Query;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Postgres, QueryBuilder};
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

#[derive(Deserialize)]
pub struct IdentityLookupQuery {
    pub station_id: Option<String>,
    pub callsign: Option<String>,
    pub publication_state: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
pub struct ModerationQueueQuery {
    pub submission_state: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
pub struct RevocationQuery {
    pub record_id: Option<String>,
    pub fingerprint: Option<String>,
    pub issuer_id: Option<String>,
    pub effective_before: Option<String>,
    pub effective_after: Option<String>,
    pub limit: Option<u32>,
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

#[derive(Serialize, FromRow)]
struct ModerationQueueItem {
    submission_id: String,
    submitter_identity: String,
    submission_state: String,
    moderation_reason_code: Option<String>,
}

#[derive(Serialize)]
struct ModerationDecisionResponse {
    submission_id: String,
    submission_state: String,
    moderation_event_id: String,
}

#[derive(Serialize, FromRow)]
struct RevocationResponse {
    revocation_id: String,
    record_id: String,
    revision_id: Option<String>,
    key_id: Option<String>,
    issuer_identity: String,
    reason_code: String,
    effective_at: String,
    created_at: String,
}

#[derive(Serialize, FromRow)]
struct TrustBundleResponse {
    schema_version: String,
    bundle_id: String,
    generated_at: String,
    issuer_instance_id: String,
    signing_algorithms: serde_json::Value,
    records: serde_json::Value,
    bundle_signature: String,
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

pub async fn lookup_identity(
    State(state): State<AppState>,
    Query(query): Query<IdentityLookupQuery>,
) -> impl IntoResponse {
    let mut qb = QueryBuilder::<Postgres>::new(
        "SELECT record_id, station_id, callsign, publication_state, current_revision_id FROM identity_records",
    );

    let mut has_where = false;
    if let Some(station_id) = query.station_id {
        qb.push(if has_where { " AND " } else { " WHERE " });
        has_where = true;
        qb.push("station_id = ").push_bind(station_id);
    }
    if let Some(callsign) = query.callsign {
        qb.push(if has_where { " AND " } else { " WHERE " });
        has_where = true;
        qb.push("callsign = ").push_bind(callsign);
    }
    if let Some(publication_state) = query.publication_state {
        qb.push(if has_where { " AND " } else { " WHERE " });
        qb.push("publication_state = ").push_bind(publication_state);
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 500) as i64;
    qb.push(" ORDER BY created_at DESC LIMIT ").push_bind(limit);

    let built = qb.build_query_as::<IdentityRecordResponse>();
    match built.fetch_all(&state.db).await {
        Ok(records) => (StatusCode::OK, Json(records)).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("identity lookup failed: {err}"),
            }),
        )
            .into_response(),
    }
}

pub async fn list_revocations(
    State(state): State<AppState>,
    Query(query): Query<RevocationQuery>,
) -> impl IntoResponse {
    let effective_before = match parse_rfc3339_query(query.effective_before.as_deref(), "effective_before") {
        Ok(value) => value,
        Err(response) => return response,
    };
    let effective_after = match parse_rfc3339_query(query.effective_after.as_deref(), "effective_after") {
        Ok(value) => value,
        Err(response) => return response,
    };

    if let (Some(after), Some(before)) = (&effective_after, &effective_before) {
        if after > before {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiMessage {
                    status: "validation_error",
                    detail: "effective_after must be less than or equal to effective_before".to_string(),
                }),
            )
                .into_response();
        }
    }

    let mut qb = QueryBuilder::<Postgres>::new(
        "SELECT
            revocation_id,
            record_id,
            revision_id,
            key_id,
            issuer_identity,
            reason_code,
            effective_at::text AS effective_at,
            created_at::text AS created_at
         FROM revocations",
    );

    let mut has_where = false;
    if let Some(record_id) = query.record_id {
        qb.push(if has_where { " AND " } else { " WHERE " });
        has_where = true;
        qb.push("record_id = ").push_bind(record_id);
    }

    if let Some(fingerprint) = query.fingerprint {
        qb.push(if has_where { " AND " } else { " WHERE " });
        has_where = true;
        qb.push(
            "EXISTS (
                SELECT 1 FROM identity_keys keys
                WHERE keys.key_id = revocations.key_id
                AND keys.fingerprint = ",
        )
        .push_bind(fingerprint)
        .push(")");
    }

    if let Some(issuer_id) = query.issuer_id {
        qb.push(if has_where { " AND " } else { " WHERE " });
        has_where = true;
        qb.push("issuer_identity = ").push_bind(issuer_id);
    }

    if let Some(effective_before) = effective_before {
        qb.push(if has_where { " AND " } else { " WHERE " });
        has_where = true;
        qb.push("effective_at <= ")
            .push_bind(effective_before.to_rfc3339())
            .push("::timestamptz");
    }

    if let Some(effective_after) = effective_after {
        qb.push(if has_where { " AND " } else { " WHERE " });
        qb.push("effective_at >= ")
            .push_bind(effective_after.to_rfc3339())
            .push("::timestamptz");
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 500) as i64;
    qb.push(" ORDER BY effective_at DESC LIMIT ").push_bind(limit);

    let built = qb.build_query_as::<RevocationResponse>();
    match built.fetch_all(&state.db).await {
        Ok(records) => (StatusCode::OK, Json(records)).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("revocation lookup failed: {err}"),
            }),
        )
            .into_response(),
    }
}

pub async fn get_current_trust_bundle(State(state): State<AppState>) -> impl IntoResponse {
    let result = sqlx::query_as::<_, TrustBundleResponse>(
        "SELECT
            schema_version,
            bundle_id,
            generated_at::text AS generated_at,
            issuer_instance_id,
            signing_algorithms,
            records,
            bundle_signature
         FROM trust_bundles
         WHERE is_current = TRUE
         ORDER BY generated_at DESC
         LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some(bundle)) => (StatusCode::OK, Json(bundle)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiMessage {
                status: "not_found",
                detail: "current trust bundle not found".to_string(),
            }),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("trust bundle query failed: {err}"),
            }),
        )
            .into_response(),
    }
}

pub async fn get_trust_bundle(
    State(state): State<AppState>,
    Path(bundle_id): Path<String>,
) -> impl IntoResponse {
    let result = sqlx::query_as::<_, TrustBundleResponse>(
        "SELECT
            schema_version,
            bundle_id,
            generated_at::text AS generated_at,
            issuer_instance_id,
            signing_algorithms,
            records,
            bundle_signature
         FROM trust_bundles
         WHERE bundle_id = $1",
    )
    .bind(bundle_id)
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some(bundle)) => (StatusCode::OK, Json(bundle)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiMessage {
                status: "not_found",
                detail: "trust bundle not found".to_string(),
            }),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("trust bundle query failed: {err}"),
            }),
        )
            .into_response(),
    }
}

pub async fn create_submission(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SubmissionRequest>,
) -> impl IntoResponse {
    let request_id = extract_request_id(&headers);

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
    let payload_type = req.payload_type.clone();
    let has_detached_signature = req.detached_signature.is_some();
    let payload_kind = json_kind(&req.payload);

    let validation_summary = serde_json::json!({
        "status": "pending_validation",
        "payload_type": payload_type,
        "payload_kind": payload_kind,
        "has_detached_signature": has_detached_signature
    });

    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiMessage {
                    status: "db_error",
                    detail: format!("failed to begin transaction: {err}"),
                }),
            )
                .into_response()
        }
    };

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
    .execute(&mut *tx)
    .await;

    match insert {
        Ok(_) => {
            let audit_event_id = Uuid::new_v4().to_string();
            let payload = serde_json::json!({
                "submission_id": &submission_id,
                "payload_type": req.payload_type,
                "has_detached_signature": req.detached_signature.is_some(),
            });
            let payload_hash = payload_sha256(&payload);

            let insert_audit = sqlx::query(
                "INSERT INTO audit_events (
                    event_id,
                    event_type,
                    entity_type,
                    entity_id,
                    actor_identity,
                    request_id,
                    event_payload_hash,
                    event_payload_json
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(&audit_event_id)
            .bind("submission.created")
            .bind("submission")
            .bind(&submission_id)
            .bind("api:anonymous")
            .bind(request_id)
            .bind(payload_hash)
            .bind(payload)
            .execute(&mut *tx)
            .await;

            if let Err(err) = insert_audit {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiMessage {
                        status: "db_error",
                        detail: format!("failed to insert audit event: {err}"),
                    }),
                )
                    .into_response();
            }

            if let Err(err) = tx.commit().await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiMessage {
                        status: "db_error",
                        detail: format!("failed to commit transaction: {err}"),
                    }),
                )
                    .into_response();
            }

            (
                StatusCode::CREATED,
                Json(SubmissionResponse {
                    submission_id,
                    submission_state: "pending",
                }),
            )
                .into_response()
        }
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

pub async fn get_moderation_queue(
    State(state): State<AppState>,
    Query(query): Query<ModerationQueueQuery>,
) -> impl IntoResponse {
    let allowed_state = query.submission_state.as_deref();
    if let Some(state_name) = allowed_state {
        if state_name != "pending" && state_name != "quarantined" {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiMessage {
                    status: "validation_error",
                    detail: "submission_state must be pending or quarantined".to_string(),
                }),
            )
                .into_response();
        }
    }

    let mut qb = QueryBuilder::<Postgres>::new(
        "SELECT submission_id, submitter_identity, submission_state, moderation_reason_code FROM submissions",
    );

    if let Some(state_name) = allowed_state {
        qb.push(" WHERE submission_state = ").push_bind(state_name.to_string());
    } else {
        qb.push(" WHERE submission_state IN ('pending', 'quarantined')");
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 500) as i64;
    qb.push(" ORDER BY received_at ASC LIMIT ").push_bind(limit);

    let built = qb.build_query_as::<ModerationQueueItem>();
    match built.fetch_all(&state.db).await {
        Ok(queue) => (StatusCode::OK, Json(queue)).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("moderation queue query failed: {err}"),
            }),
        )
            .into_response(),
    }
}

pub async fn post_moderation_decision(
    State(state): State<AppState>,
    Path(submission_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<ModerationDecisionRequest>,
) -> impl IntoResponse {
    let request_id = extract_request_id(&headers);

    let new_state = match req.decision.as_str() {
        "accept" => "accepted",
        "reject" => "rejected",
        "quarantine" => "quarantined",
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiMessage {
                    status: "validation_error",
                    detail: "decision must be accept, reject, or quarantine".to_string(),
                }),
            )
                .into_response();
        }
    };

    if req.reason_code.trim().is_empty() || req.reason_text.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiMessage {
                status: "validation_error",
                detail: "reason_code and reason_text must not be empty".to_string(),
            }),
        )
            .into_response();
    }

    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiMessage {
                    status: "db_error",
                    detail: format!("failed to begin transaction: {err}"),
                }),
            )
                .into_response()
        }
    };

    let update = sqlx::query(
        "UPDATE submissions
         SET submission_state = $2,
             moderation_reason_code = $3,
             updated_at = NOW()
         WHERE submission_id = $1",
    )
    .bind(&submission_id)
    .bind(new_state)
    .bind(&req.reason_code)
    .execute(&mut *tx)
    .await;

    let updated_rows = match update {
        Ok(res) => res.rows_affected(),
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiMessage {
                    status: "db_error",
                    detail: format!("failed to update submission: {err}"),
                }),
            )
                .into_response()
        }
    };

    if updated_rows == 0 {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiMessage {
                status: "not_found",
                detail: "submission not found".to_string(),
            }),
        )
            .into_response();
    }

    let event_id = Uuid::new_v4().to_string();
    let insert_event = sqlx::query(
        "INSERT INTO moderation_events (
            event_id,
            submission_id,
            actor_identity,
            action,
            reason_code,
            reason_text
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&event_id)
    .bind(&submission_id)
    .bind("api:moderator")
    .bind(&req.decision)
    .bind(&req.reason_code)
    .bind(&req.reason_text)
    .execute(&mut *tx)
    .await;

    if let Err(err) = insert_event {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("failed to insert moderation event: {err}"),
            }),
        )
            .into_response();
    }

    let audit_event_id = Uuid::new_v4().to_string();
    let audit_payload = serde_json::json!({
        "submission_id": submission_id,
        "decision": req.decision,
        "reason_code": req.reason_code,
    });
    let audit_hash = payload_sha256(&audit_payload);

    let insert_audit = sqlx::query(
        "INSERT INTO audit_events (
            event_id,
            event_type,
            entity_type,
            entity_id,
            actor_identity,
            request_id,
            event_payload_hash,
            event_payload_json
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(audit_event_id)
    .bind("submission.moderated")
    .bind("submission")
    .bind(&submission_id)
    .bind("api:moderator")
    .bind(request_id)
    .bind(audit_hash)
    .bind(audit_payload)
    .execute(&mut *tx)
    .await;

    if let Err(err) = insert_audit {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("failed to insert audit event: {err}"),
            }),
        )
            .into_response();
    }

    if let Err(err) = tx.commit().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("failed to commit transaction: {err}"),
            }),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(ModerationDecisionResponse {
            submission_id,
            submission_state: new_state.to_string(),
            moderation_event_id: event_id,
        }),
    )
        .into_response()
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

fn payload_sha256(payload: &serde_json::Value) -> String {
    let bytes = payload.to_string().into_bytes();
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn extract_request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn parse_rfc3339_query(
    raw: Option<&str>,
    field_name: &'static str,
) -> Result<Option<chrono::DateTime<chrono::FixedOffset>>, axum::response::Response> {
    let Some(value) = raw else {
        return Ok(None);
    };

    match chrono::DateTime::parse_from_rfc3339(value) {
        Ok(parsed) => Ok(Some(parsed)),
        Err(_) => Err((
            StatusCode::BAD_REQUEST,
            Json(ApiMessage {
                status: "validation_error",
                detail: format!("{field_name} must be RFC3339 timestamp"),
            }),
        )
            .into_response()),
    }
}
