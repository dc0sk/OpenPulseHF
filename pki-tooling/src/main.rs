use pki_tooling::{build_router, AppState};
use sqlx::postgres::PgPoolOptions;
use std::env;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("failed to connect to database");

    let state = AppState { db };

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("failed to bind listener");

    tracing::info!("pki-tooling API listening on http://127.0.0.1:8080");
    axum::serve(listener, app)
        .await
        .expect("server runtime failure");
}
