pub mod api;
pub mod auth;
pub mod startup_env;
pub mod verification;

use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{get, patch, post};
use axum::Router;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub signing_key: ed25519_dalek::SigningKey,
    /// API key required for all mutating endpoints (`Authorization: Bearer <key>`).
    pub api_key: String,
}

pub async fn run_migrations(pool: &sqlx::PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

pub fn build_router(state: AppState) -> Router {
    // Read-only and public routes — no auth required.
    let open = Router::new()
        .route("/healthz", get(api::handlers::healthz))
        .route(
            "/api/v1/identities/{record_id}",
            get(api::handlers::get_identity),
        )
        .route(
            "/api/v1/identities:lookup",
            get(api::handlers::lookup_identity),
        )
        .route("/api/v1/revocations", get(api::handlers::list_revocations))
        .route(
            "/api/v1/trust-bundles/current",
            get(api::handlers::get_current_trust_bundle),
        )
        .route(
            "/api/v1/trust-bundles/{bundle_id}",
            get(api::handlers::get_trust_bundle),
        )
        // Submission intake is intentionally public: untrusted/signed payloads are
        // accepted into moderation and verified by server-side policy before use.
        .route(
            "/api/v1/submissions",
            post(api::handlers::create_submission),
        )
        .route(
            "/api/v1/submissions/{submission_id}",
            get(api::handlers::get_submission),
        )
        .route("/api/v1/signing-key", get(api::handlers::get_signing_key));

    // Mutating routes — require valid Bearer token.
    let protected = Router::new()
        .route(
            "/api/v1/revocations",
            post(api::handlers::create_revocation),
        )
        .route(
            "/api/v1/trust-bundles",
            post(api::handlers::publish_trust_bundle),
        )
        .route(
            "/api/v1/trust-bundles/{bundle_id}/promote",
            patch(api::handlers::promote_trust_bundle),
        )
        .route(
            "/api/v1/moderation/{submission_id}/decision",
            post(api::handlers::post_moderation_decision),
        )
        .route(
            "/api/v1/moderation/queue",
            get(api::handlers::get_moderation_queue),
        )
        .route(
            "/api/v1/session-audit-events",
            post(api::handlers::create_session_audit_event),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_api_key,
        ));

    Router::new()
        .merge(open)
        .merge(protected)
        .layer(DefaultBodyLimit::max(256 * 1024))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState {
            db: {
                // Construct a pool that will never connect — tests don't hit the DB.
                sqlx::postgres::PgPoolOptions::new()
                    .max_connections(1)
                    .connect_lazy("postgres://localhost/test")
                    .unwrap()
            },
            signing_key: {
                let seed = [0u8; 32];
                ed25519_dalek::SigningKey::from_bytes(&seed)
            },
            api_key: "test-secret".to_string(),
        }
    }

    async fn status_for(app: Router, req: Request<Body>) -> StatusCode {
        app.oneshot(req).await.unwrap().status()
    }

    #[tokio::test]
    async fn protected_without_token_returns_401() {
        let app = build_router(test_state());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/revocations")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(status_for(app, req).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn protected_with_wrong_token_returns_401() {
        let app = build_router(test_state());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/revocations")
            .header("authorization", "Bearer wrong-token")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(status_for(app, req).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn protected_with_correct_token_passes_auth() {
        let app = build_router(test_state());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/revocations")
            .header("authorization", "Bearer test-secret")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        // Auth passes; handler returns 400/500 (bad JSON / no DB) — anything but 401.
        let status = status_for(app, req).await;
        assert_ne!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn open_route_without_token_passes() {
        let app = build_router(test_state());
        let req = Request::builder()
            .uri("/healthz")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status_for(app, req).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn moderation_queue_without_token_returns_401() {
        let app = build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/moderation/queue")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status_for(app, req).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn moderation_queue_with_token_passes_auth() {
        let app = build_router(test_state());
        let req = Request::builder()
            .uri("/api/v1/moderation/queue")
            .header("authorization", "Bearer test-secret")
            .body(Body::empty())
            .unwrap();
        // In test_state() the DB is deliberately unreachable, so auth should pass
        // and the handler should fail at data access with a deterministic 500.
        assert_eq!(
            status_for(app, req).await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[tokio::test]
    async fn submissions_intake_without_token_is_not_auth_blocked() {
        let app = build_router(test_state());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/submissions")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        // Endpoint is intentionally open; malformed payload should fail validation,
        // but never with auth failure.
        assert_ne!(status_for(app, req).await, StatusCode::UNAUTHORIZED);
    }
}
