use pki_tooling::startup_env::{required_env, EnvVarError};
use sqlx::postgres::PgPoolOptions;
use thiserror::Error;

#[derive(Debug, Error)]
enum MigrateError {
    #[error("DATABASE_URL env var must be set")]
    MissingDatabaseUrl,
    #[error("DATABASE_URL must not be empty")]
    EmptyDatabaseUrl,
    #[error("DATABASE_URL must contain valid Unicode")]
    InvalidDatabaseUrl,
    #[error("failed to connect to database")]
    DatabaseConnect(#[source] sqlx::Error),
    #[error("failed to run migrations")]
    Migrate(#[source] sqlx::migrate::MigrateError),
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("fatal: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), MigrateError> {
    let database_url = required_env("DATABASE_URL").map_err(map_env_error)?;
    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .map_err(MigrateError::DatabaseConnect)?;

    pki_tooling::run_migrations(&db)
        .await
        .map_err(MigrateError::Migrate)?;

    println!("migrations applied successfully");
    Ok(())
}

fn map_env_error(err: EnvVarError) -> MigrateError {
    match err {
        EnvVarError::Missing("DATABASE_URL") => MigrateError::MissingDatabaseUrl,
        EnvVarError::Empty("DATABASE_URL") => MigrateError::EmptyDatabaseUrl,
        EnvVarError::InvalidUnicode("DATABASE_URL") => MigrateError::InvalidDatabaseUrl,
        EnvVarError::Missing(_) => MigrateError::MissingDatabaseUrl,
        EnvVarError::Empty(_) => MigrateError::EmptyDatabaseUrl,
        EnvVarError::InvalidUnicode(_) => MigrateError::InvalidDatabaseUrl,
    }
}
