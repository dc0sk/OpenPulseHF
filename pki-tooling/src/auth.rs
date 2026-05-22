use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use serde_json::json;

use crate::AppState;

/// Middleware that requires a valid `Authorization: Bearer <PKI_API_KEY>` header.
///
/// Returns 401 for missing, malformed, or incorrect tokens.
pub async fn require_api_key(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let valid = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|token| token == state.api_key)
        .unwrap_or(false);

    if valid {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"status": "unauthorized", "detail": "valid Bearer token required"})),
        )
            .into_response()
    }
}
