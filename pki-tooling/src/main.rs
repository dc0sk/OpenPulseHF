mod api;

use axum::routing::{get, post};
use axum::Router;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let app = Router::new()
        .route("/healthz", get(api::handlers::healthz))
        .route(
            "/api/v1/identities/:record_id",
            get(api::handlers::get_identity),
        )
        .route("/api/v1/identities:lookup", get(api::handlers::lookup_identity))
        .route("/api/v1/revocations", get(api::handlers::list_revocations))
        .route(
            "/api/v1/trust-bundles/current",
            get(api::handlers::get_current_trust_bundle),
        )
        .route(
            "/api/v1/trust-bundles/:bundle_id",
            get(api::handlers::get_trust_bundle),
        )
        .route("/api/v1/submissions", post(api::handlers::create_submission))
        .route(
            "/api/v1/submissions/:submission_id",
            get(api::handlers::get_submission),
        )
        .route("/api/v1/moderation/queue", get(api::handlers::get_moderation_queue))
        .route(
            "/api/v1/moderation/:submission_id/decision",
            post(api::handlers::post_moderation_decision),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("failed to bind listener");

    tracing::info!("pki-tooling API listening on http://127.0.0.1:8080");
    axum::serve(listener, app)
        .await
        .expect("server runtime failure");
}
