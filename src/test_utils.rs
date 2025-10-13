pub mod test_helpers {
    use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
    use tempfile::NamedTempFile;

    /// Create a new in-memory SQLite database for testing
    pub async fn create_test_db() -> Result<SqlitePool, sqlx::Error> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await?;

        // Run migrations
        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(pool)
    }

    /// Create a temporary file-based SQLite database for testing
    /// Useful when you need to test features that don't work with in-memory databases
    pub async fn create_test_db_file() -> Result<(SqlitePool, NamedTempFile), sqlx::Error> {
        let temp_file = NamedTempFile::new().map_err(sqlx::Error::Io)?;
        let db_path = temp_file
            .path()
            .to_str()
            .ok_or_else(|| sqlx::Error::Configuration("Invalid database path".into()))?;
        let database_url = format!("sqlite://{}", db_path);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await?;

        // Run migrations
        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok((pool, temp_file))
    }

    /// Insert a test user with hashed password
    pub async fn insert_test_user(
        pool: &SqlitePool,
        email: &str,
        password: &str,
        verified: bool,
    ) -> Result<i64, sqlx::Error> {
        use argon2::{
            password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
            Argon2,
        };

        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| {
                sqlx::Error::Configuration(format!("Password hashing failed: {}", e).into())
            })?
            .to_string();

        let result = sqlx::query!(
            "INSERT INTO users (email, password_hash, email_verified) VALUES (?, ?, ?)",
            email,
            password_hash,
            verified
        )
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Create a test toolkit for testing
    pub async fn create_test_toolkit(
        pool: &SqlitePool,
        user_id: i64,
        title: &str,
    ) -> Result<i64, sqlx::Error> {
        let result = sqlx::query!(
            "INSERT INTO toolkits (user_id, title, description, visibility) VALUES (?, ?, ?, ?)",
            user_id,
            title,
            "Test toolkit description",
            "private"
        )
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Create a test tool for testing
    #[allow(clippy::too_many_arguments)]
    pub async fn create_test_tool(
        pool: &SqlitePool,
        toolkit_id: i64,
        name: &str,
        method: &str,
        url: Option<&str>,
        headers: Option<&str>,
        body: Option<&str>,
        timeout_ms: i64,
    ) -> Result<i64, sqlx::Error> {
        let result = sqlx::query!(
            r#"
            INSERT INTO tools (toolkit_id, name, description, method, url, headers, body, timeout_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            toolkit_id,
            name,
            "Test tool description",
            method,
            url,
            headers,
            body,
            timeout_ms
        )
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Create a test server for testing
    pub async fn create_test_server(
        pool: &SqlitePool,
        user_id: i64,
        name: &str,
        description: Option<&str>,
    ) -> Result<(i64, String), sqlx::Error> {
        use uuid::Uuid;

        let uuid = Uuid::new_v4().to_string();
        let result = sqlx::query!(
            r#"
            INSERT INTO servers (uuid, user_id, name, description)
            VALUES (?, ?, ?, ?)
            "#,
            uuid,
            user_id,
            name,
            description
        )
        .execute(pool)
        .await?;

        Ok((result.last_insert_rowid(), uuid))
    }
}

// Re-export commonly used test functions at module level for convenience
// Note: This is test-only code. Panic on error is acceptable in tests.
#[cfg(test)]
pub async fn create_test_pool() -> sqlx::SqlitePool {
    match test_helpers::create_test_db().await {
        Ok(pool) => pool,
        Err(e) => panic!("Failed to create test pool: {}", e),
    }
}

#[cfg(test)]
pub async fn create_test_user(
    pool: &sqlx::SqlitePool,
    email: &str,
    password: &str,
) -> Result<i64, sqlx::Error> {
    test_helpers::insert_test_user(pool, email, password, true).await
}

#[cfg(test)]
pub use test_helpers::create_test_toolkit;
