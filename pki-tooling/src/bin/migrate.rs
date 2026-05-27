use sqlx::postgres::PgPoolOptions;
use std::env;
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
    let database_url = required_database_url()?;
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

fn required_database_url() -> Result<String, MigrateError> {
    match env::var("DATABASE_URL") {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Err(MigrateError::EmptyDatabaseUrl)
            } else {
                Ok(trimmed.to_string())
            }
        }
        Err(env::VarError::NotPresent) => Err(MigrateError::MissingDatabaseUrl),
        Err(env::VarError::NotUnicode(_)) => Err(MigrateError::InvalidDatabaseUrl),
    }
}
