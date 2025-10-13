use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::env;

pub async fn create_pool() -> Result<SqlitePool, sqlx::Error> {
    let database_url = env::var("DATABASE_URL")
        .map_err(|_| sqlx::Error::Configuration("DATABASE_URL must be set".into()))?;

    // Ensure the data directory exists
    if let Some(parent) = std::path::Path::new(&database_url.replace("sqlite://", "")).parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    Ok(pool)
}
