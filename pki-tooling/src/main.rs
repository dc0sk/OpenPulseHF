use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::SigningKey;
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

    let signing_key = match env::var("PKI_SIGNING_KEY") {
        Ok(b64) => {
            let seed = STANDARD
                .decode(&b64)
                .expect("PKI_SIGNING_KEY must be valid base64");
            let arr: [u8; 32] = seed
                .as_slice()
                .try_into()
                .expect("PKI_SIGNING_KEY seed must be exactly 32 bytes");
            SigningKey::from_bytes(&arr)
        }
        Err(_) => {
            tracing::warn!(
                "PKI_SIGNING_KEY not set; using ephemeral signing key — bundles will not survive restart"
            );
            use rand::RngCore;
            let mut seed = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut seed);
            SigningKey::from_bytes(&seed)
        }
    };

    let state = AppState { db, signing_key };

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("failed to bind listener");

    tracing::info!("pki-tooling API listening on http://127.0.0.1:8080");
    axum::serve(listener, app)
        .await
        .expect("server runtime failure");
}
