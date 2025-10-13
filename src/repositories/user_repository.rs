use crate::models::user::User;
use async_trait::async_trait;
use sqlx::SqlitePool;

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("User not found")]
    NotFound,
    #[error("User already exists")]
    AlreadyExists,
}

pub type RepositoryResult<T> = Result<T, RepositoryError>;

#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait UserRepository: Send + Sync {
    async fn create_user(
        &self,
        email: &str,
        password_hash: &str,
        email_verified: bool,
    ) -> RepositoryResult<User>;
    async fn find_by_email(&self, email: &str) -> RepositoryResult<Option<User>>;
    async fn find_by_id(&self, id: i64) -> RepositoryResult<Option<User>>;
    async fn update_password(&self, id: i64, password_hash: &str) -> RepositoryResult<()>;
    async fn update_email(&self, id: i64, email: &str) -> RepositoryResult<()>;
    async fn verify_email(&self, id: i64) -> RepositoryResult<()>;
    async fn delete_user(&self, id: i64) -> RepositoryResult<()>;
    async fn list_users(
        &self,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> RepositoryResult<Vec<User>>;
}

pub struct SqliteUserRepository {
    pool: SqlitePool,
}

impl SqliteUserRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for SqliteUserRepository {
    async fn create_user(
        &self,
        email: &str,
        password_hash: &str,
        email_verified: bool,
    ) -> RepositoryResult<User> {
        let result = sqlx::query!(
            "INSERT INTO users (email, password_hash, email_verified) VALUES (?, ?, ?)",
            email,
            password_hash,
            email_verified
        )
        .execute(&self.pool)
        .await;

        match result {
            Ok(res) => {
                let id = res.last_insert_rowid();
                self.find_by_id(id).await?.ok_or(RepositoryError::NotFound)
            }
            Err(e) => {
                if e.to_string().contains("UNIQUE") {
                    Err(RepositoryError::AlreadyExists)
                } else {
                    Err(RepositoryError::Database(e))
                }
            }
        }
    }

    async fn find_by_email(&self, email: &str) -> RepositoryResult<Option<User>> {
        let row = sqlx::query!(
            r#"
            SELECT
                id,
                email,
                password_hash,
                created_at,
                email_verified as "email_verified: bool"
            FROM users
            WHERE email = ?
            "#,
            email
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| User {
            id: r.id.unwrap_or(0),
            email: r.email,
            password_hash: r.password_hash,
            created_at: r.created_at.map(|dt| dt.to_string()),
            email_verified: r.email_verified.unwrap_or(false),
        }))
    }

    async fn find_by_id(&self, id: i64) -> RepositoryResult<Option<User>> {
        let row = sqlx::query!(
            r#"
            SELECT
                id,
                email,
                password_hash,
                created_at,
                email_verified as "email_verified: bool"
            FROM users
            WHERE id = ?
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| User {
            id: r.id,
            email: r.email,
            password_hash: r.password_hash,
            created_at: r.created_at.map(|dt| dt.to_string()),
            email_verified: r.email_verified.unwrap_or(false),
        }))
    }

    async fn update_password(&self, id: i64, password_hash: &str) -> RepositoryResult<()> {
        let result = sqlx::query!(
            "UPDATE users SET password_hash = ? WHERE id = ?",
            password_hash,
            id
        )
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::NotFound);
        }

        Ok(())
    }

    async fn update_email(&self, id: i64, email: &str) -> RepositoryResult<()> {
        let result = sqlx::query!("UPDATE users SET email = ? WHERE id = ?", email, id)
            .execute(&self.pool)
            .await;

        match result {
            Ok(res) => {
                if res.rows_affected() == 0 {
                    return Err(RepositoryError::NotFound);
                }
                Ok(())
            }
            Err(e) => {
                if e.to_string().contains("UNIQUE") {
                    Err(RepositoryError::AlreadyExists)
                } else {
                    Err(RepositoryError::Database(e))
                }
            }
        }
    }

    async fn verify_email(&self, id: i64) -> RepositoryResult<()> {
        let result = sqlx::query!("UPDATE users SET email_verified = true WHERE id = ?", id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::NotFound);
        }

        Ok(())
    }

    async fn delete_user(&self, id: i64) -> RepositoryResult<()> {
        let result = sqlx::query!("DELETE FROM users WHERE id = ?", id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::NotFound);
        }

        Ok(())
    }

    async fn list_users(
        &self,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> RepositoryResult<Vec<User>> {
        let limit = limit.unwrap_or(100);
        let offset = offset.unwrap_or(0);

        let rows = sqlx::query!(
            r#"
            SELECT
                id,
                email,
                password_hash,
                created_at,
                email_verified as "email_verified: bool"
            FROM users
            ORDER BY created_at DESC
            LIMIT ? OFFSET ?
            "#,
            limit,
            offset
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| User {
                id: r.id,
                email: r.email,
                password_hash: r.password_hash,
                created_at: r.created_at.map(|dt| dt.to_string()),
                email_verified: r.email_verified.unwrap_or(false),
            })
            .collect())
    }
}
