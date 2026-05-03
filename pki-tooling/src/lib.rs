pub mod api;
pub mod verification;

use axum::routing::{get, patch, post};
use axum::Router;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
}

pub async fn run_migrations(pool: &sqlx::PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(api::handlers::healthz))
        .route(
            "/api/v1/identities/:record_id",
            get(api::handlers::get_identity),
        )
        .route(
            "/api/v1/identities:lookup",
            get(api::handlers::lookup_identity),
        )
        .route("/api/v1/revocations", get(api::handlers::list_revocations))
        .route(
            "/api/v1/revocations",
            post(api::handlers::create_revocation),
        )
        .route(
            "/api/v1/trust-bundles/current",
            get(api::handlers::get_current_trust_bundle),
        )
        .route(
            "/api/v1/trust-bundles/:bundle_id",
            get(api::handlers::get_trust_bundle),
        )
        .route(
            "/api/v1/trust-bundles",
            post(api::handlers::publish_trust_bundle),
        )
        .route(
            "/api/v1/trust-bundles/:bundle_id/promote",
            patch(api::handlers::promote_trust_bundle),
        )
        .route(
            "/api/v1/submissions",
            post(api::handlers::create_submission),
        )
        .route(
            "/api/v1/submissions/:submission_id",
            get(api::handlers::get_submission),
        )
        .route(
            "/api/v1/moderation/queue",
            get(api::handlers::get_moderation_queue),
        )
        .route(
            "/api/v1/moderation/:submission_id/decision",
            post(api::handlers::post_moderation_decision),
        )
        .route(
            "/api/v1/session-audit-events",
            post(api::handlers::create_session_audit_event),
        )
        .with_state(state)
}
