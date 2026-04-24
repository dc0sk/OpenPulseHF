use sqlx::postgres::PgPoolOptions;
use std::env;

#[tokio::main]
async fn main() {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("failed to connect to database");

    pki_tooling::run_migrations(&db)
        .await
        .expect("failed to run migrations");

    println!("migrations applied successfully");
}
