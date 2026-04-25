use crate::AppState;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Postgres, QueryBuilder, Row};
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

#[derive(Deserialize)]
pub struct CreateRevocationRequest {
    pub record_id: String,
    pub revision_id: String,
    pub key_id: String,
    pub issuer_identity: String,
    pub reason_code: String,
    pub effective_at: String,
}

#[derive(Serialize)]
struct CreateRevocationResponse {
    revocation_id: String,
    effective_at: String,
    created_at: String,
}

#[derive(Deserialize)]
pub struct PublishTrustBundleRequest {
    pub schema_version: String,
    pub generated_at: String,
    pub issuer_instance_id: String,
    pub signing_algorithms: serde_json::Value,
    pub records: serde_json::Value,
    pub bundle_signature: String,
}

#[derive(Serialize)]
struct PublishTrustBundleResponse {
    bundle_id: String,
    is_current: bool,
    created_at: String,
}

#[derive(Deserialize)]
pub struct PromoteTrustBundleRequest {
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct SessionAuditEventRequest {
    pub session_id: String,
    pub peer_id: Option<String>,
    pub policy_profile: String,
    pub selected_mode: String,
    pub trust_level: String,
    pub certificate_source: String,
    pub trust_reason_code: Option<String>,
    pub transitions: serde_json::Value,
    pub actor_identity: Option<String>,
}

#[derive(Serialize)]
struct PromoteTrustBundleResponse {
    bundle_id: String,
    is_current: bool,
    promoted_at: String,
}

#[derive(Serialize)]
struct SessionAuditEventResponse {
    event_id: String,
    session_id: String,
    event_type: &'static str,
}

pub async fn healthz() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(ApiMessage {
            status: "ok",
            detail: "service healthy".to_string(),
        }),
    )
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
    let effective_before =
        match parse_rfc3339_query(query.effective_before.as_deref(), "effective_before") {
            Ok(value) => value,
            Err(response) => return response,
        };
    let effective_after =
        match parse_rfc3339_query(query.effective_after.as_deref(), "effective_after") {
            Ok(value) => value,
            Err(response) => return response,
        };

    if let (Some(after), Some(before)) = (&effective_after, &effective_before) {
        if after > before {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiMessage {
                    status: "validation_error",
                    detail: "effective_after must be less than or equal to effective_before"
                        .to_string(),
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
    qb.push(" ORDER BY effective_at DESC, created_at DESC, revocation_id ASC LIMIT ")
        .push_bind(limit);

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

    if let Err(detail) = validate_signed_payload_conformance(&req) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiMessage {
                status: "validation_error",
                detail,
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
        qb.push(" WHERE submission_state = ")
            .push_bind(state_name.to_string());
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

pub async fn create_revocation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateRevocationRequest>,
) -> impl IntoResponse {
    let request_id = extract_request_id(&headers);

    if let Some(request_id_str) = request_id.as_deref() {
        match check_request_tracking(&state.db, request_id_str, "/api/v1/revocations", "POST").await
        {
            Ok(Some((status, body))) => return (status, Json(body)).into_response(),
            Ok(None) => {}
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiMessage {
                        status: "db_error",
                        detail: format!("failed to read request tracking: {err}"),
                    }),
                )
                    .into_response();
            }
        }
    }

    // Validation: check required fields
    if req.record_id.trim().is_empty()
        || req.revision_id.trim().is_empty()
        || req.key_id.trim().is_empty()
        || req.issuer_identity.trim().is_empty()
        || req.reason_code.trim().is_empty()
        || req.effective_at.trim().is_empty()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiMessage {
                status: "validation_error",
                detail: "all fields (record_id, revision_id, key_id, issuer_identity, reason_code, effective_at) are required and non-empty".to_string(),
            }),
        )
            .into_response();
    }

    // Validation: check effective_at is RFC3339
    let effective_at_parsed = match chrono::DateTime::parse_from_rfc3339(req.effective_at.as_str())
    {
        Ok(dt) => dt.to_rfc3339(),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiMessage {
                    status: "validation_error",
                    detail: "effective_at must be RFC3339 timestamp".to_string(),
                }),
            )
                .into_response();
        }
    };

    // Validation: verify record exists
    let record_check = sqlx::query("SELECT record_id FROM identity_records WHERE record_id = $1")
        .bind(&req.record_id)
        .fetch_optional(&state.db)
        .await;

    match record_check {
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiMessage {
                    status: "validation_error",
                    detail: format!("record_id {} not found", req.record_id),
                }),
            )
                .into_response();
        }
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiMessage {
                    status: "db_error",
                    detail: format!("failed to verify record: {err}"),
                }),
            )
                .into_response();
        }
        Ok(Some(_)) => {} // record exists, proceed
    }

    let revocation_id = Uuid::new_v4().to_string();

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

    // Insert revocation
    let insert_revocation = sqlx::query(
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
    .bind(&revocation_id)
    .bind(&req.record_id)
    .bind(&req.revision_id)
    .bind(&req.key_id)
    .bind(&req.issuer_identity)
    .bind(&req.reason_code)
    .bind(&effective_at_parsed)
    .execute(&mut *tx)
    .await;

    if let Err(err) = insert_revocation {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("failed to insert revocation: {err}"),
            }),
        )
            .into_response();
    }

    // Insert audit event
    let audit_event_id = Uuid::new_v4().to_string();
    let audit_payload = serde_json::json!({
        "revocation_id": &revocation_id,
        "record_id": &req.record_id,
        "issuer_identity": &req.issuer_identity,
        "reason_code": &req.reason_code
    });
    let payload_hash = payload_sha256(&audit_payload);

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
    .bind("revocation_created")
    .bind("revocation")
    .bind(&revocation_id)
    .bind(&req.issuer_identity)
    .bind(&request_id)
    .bind(&payload_hash)
    .bind(&audit_payload)
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

    // Query back the created_at timestamp
    let created_record = sqlx::query("SELECT created_at FROM revocations WHERE revocation_id = $1")
        .bind(&revocation_id)
        .fetch_one(&state.db)
        .await;

    let created_at = match created_record {
        Ok(row) => {
            let ts: String = row.get("created_at");
            ts
        }
        Err(_) => "unknown".to_string(),
    };

    let response_body = serde_json::json!(CreateRevocationResponse {
        revocation_id,
        effective_at: effective_at_parsed,
        created_at,
    });

    if let Some(request_id_str) = request_id.as_deref() {
        let _ = track_request(
            &state.db,
            request_id_str,
            "/api/v1/revocations",
            "POST",
            StatusCode::CREATED,
            &response_body,
        )
        .await;
    }

    (StatusCode::CREATED, Json(response_body)).into_response()
}

pub async fn publish_trust_bundle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PublishTrustBundleRequest>,
) -> impl IntoResponse {
    let request_id = extract_request_id(&headers);

    if let Some(request_id_str) = request_id.as_deref() {
        match check_request_tracking(&state.db, request_id_str, "/api/v1/trust-bundles", "POST")
            .await
        {
            Ok(Some((status, body))) => return (status, Json(body)).into_response(),
            Ok(None) => {}
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiMessage {
                        status: "db_error",
                        detail: format!("failed to read request tracking: {err}"),
                    }),
                )
                    .into_response();
            }
        }
    }

    // Validation: check required fields
    if req.schema_version.trim().is_empty()
        || req.generated_at.trim().is_empty()
        || req.issuer_instance_id.trim().is_empty()
        || req.bundle_signature.trim().is_empty()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiMessage {
                status: "validation_error",
                detail: "all fields (schema_version, generated_at, issuer_instance_id, signing_algorithms, records, bundle_signature) are required and non-empty".to_string(),
            }),
        )
            .into_response();
    }

    // Validation: check generated_at is RFC3339
    let generated_at_parsed = match chrono::DateTime::parse_from_rfc3339(req.generated_at.as_str())
    {
        Ok(dt) => dt.to_rfc3339(),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiMessage {
                    status: "validation_error",
                    detail: "generated_at must be RFC3339 timestamp".to_string(),
                }),
            )
                .into_response();
        }
    };

    let bundle_id = Uuid::new_v4().to_string();

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

    // Insert trust bundle (is_current = false)
    let insert_bundle = sqlx::query(
        "INSERT INTO trust_bundles (
            bundle_id,
            schema_version,
            generated_at,
            issuer_instance_id,
            signing_algorithms,
            records,
            bundle_signature,
            is_current
         ) VALUES ($1, $2, $3::timestamptz, $4, $5, $6, $7, $8)",
    )
    .bind(&bundle_id)
    .bind(&req.schema_version)
    .bind(&generated_at_parsed)
    .bind(&req.issuer_instance_id)
    .bind(&req.signing_algorithms)
    .bind(&req.records)
    .bind(&req.bundle_signature)
    .bind(false) // initially not current
    .execute(&mut *tx)
    .await;

    if let Err(err) = insert_bundle {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("failed to insert trust bundle: {err}"),
            }),
        )
            .into_response();
    }

    // Insert audit event
    let audit_event_id = Uuid::new_v4().to_string();
    let audit_payload = serde_json::json!({
        "bundle_id": &bundle_id,
        "schema_version": &req.schema_version,
        "issuer_instance_id": &req.issuer_instance_id
    });
    let payload_hash = payload_sha256(&audit_payload);

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
    .bind("bundle_published")
    .bind("trust_bundle")
    .bind(&bundle_id)
    .bind(&req.issuer_instance_id)
    .bind(&request_id)
    .bind(&payload_hash)
    .bind(&audit_payload)
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

    // Query back the created_at timestamp
    let created_record = sqlx::query("SELECT created_at FROM trust_bundles WHERE bundle_id = $1")
        .bind(&bundle_id)
        .fetch_one(&state.db)
        .await;

    let created_at = match created_record {
        Ok(row) => {
            let ts: String = row.get("created_at");
            ts
        }
        Err(_) => "unknown".to_string(),
    };

    let response_body = serde_json::json!(PublishTrustBundleResponse {
        bundle_id,
        is_current: false,
        created_at,
    });

    if let Some(request_id_str) = request_id.as_deref() {
        let _ = track_request(
            &state.db,
            request_id_str,
            "/api/v1/trust-bundles",
            "POST",
            StatusCode::CREATED,
            &response_body,
        )
        .await;
    }

    (StatusCode::CREATED, Json(response_body)).into_response()
}

pub async fn promote_trust_bundle(
    State(state): State<AppState>,
    Path(bundle_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<PromoteTrustBundleRequest>,
) -> impl IntoResponse {
    let request_id = extract_request_id(&headers);

    if let Some(request_id_str) = request_id.as_deref() {
        match check_request_tracking(
            &state.db,
            request_id_str,
            "/api/v1/trust-bundles/:bundle_id/promote",
            "PATCH",
        )
        .await
        {
            Ok(Some((status, body))) => return (status, Json(body)).into_response(),
            Ok(None) => {}
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiMessage {
                        status: "db_error",
                        detail: format!("failed to read request tracking: {err}"),
                    }),
                )
                    .into_response();
            }
        }
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

    let exists = match sqlx::query("SELECT bundle_id FROM trust_bundles WHERE bundle_id = $1")
        .bind(&bundle_id)
        .fetch_optional(&mut *tx)
        .await
    {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiMessage {
                    status: "db_error",
                    detail: format!("failed to query trust bundle: {err}"),
                }),
            )
                .into_response();
        }
    };

    if !exists {
        let body = serde_json::json!({
            "status": "not_found",
            "detail": "trust bundle not found"
        });
        if let Some(request_id_str) = request_id.as_deref() {
            let _ = track_request(
                &state.db,
                request_id_str,
                "/api/v1/trust-bundles/:bundle_id/promote",
                "PATCH",
                StatusCode::NOT_FOUND,
                &body,
            )
            .await;
        }
        return (StatusCode::NOT_FOUND, Json(body)).into_response();
    }

    if let Err(err) =
        sqlx::query("UPDATE trust_bundles SET is_current = false WHERE is_current = true")
            .execute(&mut *tx)
            .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("failed to clear current trust bundle: {err}"),
            }),
        )
            .into_response();
    }

    if let Err(err) = sqlx::query("UPDATE trust_bundles SET is_current = true WHERE bundle_id = $1")
        .bind(&bundle_id)
        .execute(&mut *tx)
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("failed to promote trust bundle: {err}"),
            }),
        )
            .into_response();
    }

    let audit_event_id = Uuid::new_v4().to_string();
    let audit_payload = serde_json::json!({
        "bundle_id": &bundle_id,
        "reason": req.reason
    });
    let payload_hash = payload_sha256(&audit_payload);

    if let Err(err) = sqlx::query(
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
    .bind("bundle_promoted")
    .bind("trust_bundle")
    .bind(&bundle_id)
    .bind("system")
    .bind(&request_id)
    .bind(&payload_hash)
    .bind(&audit_payload)
    .execute(&mut *tx)
    .await
    {
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

    let promoted_at = match sqlx::query("SELECT created_at FROM trust_bundles WHERE bundle_id = $1")
        .bind(&bundle_id)
        .fetch_one(&state.db)
        .await
    {
        Ok(row) => row.get::<String, _>("created_at"),
        Err(_) => "unknown".to_string(),
    };

    let response_body = serde_json::json!(PromoteTrustBundleResponse {
        bundle_id,
        is_current: true,
        promoted_at,
    });

    if let Some(request_id_str) = request_id.as_deref() {
        let _ = track_request(
            &state.db,
            request_id_str,
            "/api/v1/trust-bundles/:bundle_id/promote",
            "PATCH",
            StatusCode::OK,
            &response_body,
        )
        .await;
    }

    (StatusCode::OK, Json(response_body)).into_response()
}

pub async fn create_session_audit_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SessionAuditEventRequest>,
) -> impl IntoResponse {
    let request_id = extract_request_id(&headers);

    if req.session_id.trim().is_empty()
        || req.policy_profile.trim().is_empty()
        || req.selected_mode.trim().is_empty()
        || req.trust_level.trim().is_empty()
        || req.certificate_source.trim().is_empty()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiMessage {
                status: "validation_error",
                detail: "session_id, policy_profile, selected_mode, trust_level, and certificate_source must be non-empty"
                    .to_string(),
            }),
        )
            .into_response();
    }

    let Some(transitions) = req.transitions.as_array() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiMessage {
                status: "validation_error",
                detail: "transitions must be a JSON array".to_string(),
            }),
        )
            .into_response();
    };

    if transitions.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiMessage {
                status: "validation_error",
                detail: "transitions must contain at least one state transition".to_string(),
            }),
        )
            .into_response();
    }

    let event_id = Uuid::new_v4().to_string();
    let actor_identity = req
        .actor_identity
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("openpulse-modem")
        .to_string();
    let session_id = req.session_id.clone();

    let payload = serde_json::json!({
        "session_id": &session_id,
        "peer_id": req.peer_id,
        "policy_profile": req.policy_profile,
        "selected_mode": req.selected_mode,
        "trust_level": req.trust_level,
        "certificate_source": req.certificate_source,
        "trust_reason_code": req.trust_reason_code,
        "transition_count": transitions.len(),
        "transitions": req.transitions,
    });
    let payload_hash = payload_sha256(&payload);
    let entity_id = payload["session_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    let insert = sqlx::query(
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
    .bind(&event_id)
    .bind("session.audit_recorded")
    .bind("session")
    .bind(entity_id)
    .bind(actor_identity)
    .bind(request_id)
    .bind(payload_hash)
    .bind(payload)
    .execute(&state.db)
    .await;

    match insert {
        Ok(_) => (
            StatusCode::CREATED,
            Json(SessionAuditEventResponse {
                event_id,
                session_id,
                event_type: "session.audit_recorded",
            }),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiMessage {
                status: "db_error",
                detail: format!("failed to insert session audit event: {err}"),
            }),
        )
            .into_response(),
    }
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

fn require_object_field(payload: &serde_json::Value, field: &str) -> Result<(), String> {
    let obj = payload
        .as_object()
        .ok_or_else(|| "payload must be a JSON object".to_string())?;

    if !obj.contains_key(field) {
        return Err(format!("payload is missing required field '{field}'"));
    }

    Ok(())
}

fn validate_signed_payload_conformance(req: &SubmissionRequest) -> Result<(), String> {
    let payload_type = req.payload_type.trim();
    if payload_type != "signed_handshake" && payload_type != "signed_manifest" {
        return Ok(());
    }

    let signature = req.detached_signature.as_ref().ok_or_else(|| {
        "detached_signature is required for signed_handshake and signed_manifest".to_string()
    })?;

    if signature.trim().is_empty() {
        return Err(
            "detached_signature is required for signed_handshake and signed_manifest".to_string(),
        );
    }

    require_object_field(&req.payload, "session_id")?;
    require_object_field(&req.payload, "signed_at")?;

    if payload_type == "signed_handshake" {
        require_object_field(&req.payload, "peer_id")?;
        require_object_field(&req.payload, "handshake_nonce")?;
    }

    if payload_type == "signed_manifest" {
        require_object_field(&req.payload, "manifest_hash")?;
        require_object_field(&req.payload, "chunk_count")?;
    }

    Ok(())
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

async fn check_request_tracking(
    db: &sqlx::PgPool,
    request_id: &str,
    endpoint: &str,
    method: &str,
) -> Result<Option<(StatusCode, serde_json::Value)>, sqlx::Error> {
    let record = sqlx::query(
        "SELECT endpoint, method, response_status, response_body_json
         FROM request_tracking
         WHERE request_id = $1 AND expires_at > NOW()",
    )
    .bind(request_id)
    .fetch_optional(db)
    .await?;

    let Some(row) = record else {
        return Ok(None);
    };

    let existing_endpoint: String = row.get("endpoint");
    let existing_method: String = row.get("method");
    if existing_endpoint != endpoint || existing_method != method {
        return Ok(Some((
            StatusCode::CONFLICT,
            serde_json::json!({
                "status": "validation_error",
                "detail": "request_id already used for a different endpoint or method"
            }),
        )));
    }

    let status: i32 = row.get("response_status");
    let body: serde_json::Value = row.get("response_body_json");
    Ok(Some((
        StatusCode::from_u16(status as u16).unwrap_or(StatusCode::OK),
        body,
    )))
}

async fn track_request(
    db: &sqlx::PgPool,
    request_id: &str,
    endpoint: &str,
    method: &str,
    response_status: StatusCode,
    response_body: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    let status_code = response_status.as_u16() as i32;
    let body_hash = payload_sha256(response_body);

    sqlx::query(
        "INSERT INTO request_tracking (
            request_id,
            endpoint,
            method,
            response_status,
            response_body_hash,
            response_body_json,
            expires_at
         ) VALUES ($1, $2, $3, $4, $5, $6, NOW() + INTERVAL '24 hours')
         ON CONFLICT (request_id) DO NOTHING",
    )
    .bind(request_id)
    .bind(endpoint)
    .bind(method)
    .bind(status_code)
    .bind(&body_hash)
    .bind(response_body)
    .execute(db)
    .await?;

    Ok(())
}
