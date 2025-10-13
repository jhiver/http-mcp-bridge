use crate::models::{MagicLoginToken, PendingRegistration, User};
use crate::repositories::user_repository::UserRepository;
use crate::services::email_service::{EmailError, EmailService};
use crate::services::user_service::UserService;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use chrono::{Duration, Utc};
use rand::Rng;
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum AuthTokenError {
    #[error("Token not found or expired")]
    TokenNotFound,
    #[error("Token already used")]
    TokenAlreadyUsed,
    #[error("User not found")]
    UserNotFound,
    #[error("Email error: {0}")]
    EmailError(#[from] EmailError),
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
    #[error("User service error: {0}")]
    UserServiceError(String),
}

pub struct AuthTokenService {
    pool: SqlitePool,
    email_service: Box<dyn EmailService>,
    user_repository: Arc<dyn UserRepository>,
    #[allow(dead_code)]
    user_service: Arc<UserService>,
}

impl AuthTokenService {
    pub fn new(
        pool: SqlitePool,
        email_service: Box<dyn EmailService>,
        user_repository: Arc<dyn UserRepository>,
        user_service: Arc<UserService>,
    ) -> Self {
        Self {
            pool,
            email_service,
            user_repository,
            user_service,
        }
    }

    fn generate_token() -> String {
        let mut rng = rand::thread_rng();
        let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
        hex::encode(bytes)
    }

    fn generate_random_password() -> String {
        let mut rng = rand::thread_rng();
        let charset: &[u8] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*";
        let password: String = (0..16)
            .map(|_| {
                let idx = rng.gen_range(0..charset.len());
                // Use direct indexing since gen_range guarantees idx is in bounds
                charset[idx] as char
            })
            .collect();
        password
    }

    pub async fn create_pending_registration(
        &self,
        email: &str,
        password_opt: Option<&str>,
    ) -> Result<String, AuthTokenError> {
        let token = Self::generate_token();
        let expires_at = Utc::now() + Duration::hours(24);
        let expires_at_str = expires_at.to_rfc3339();

        let password_hash = if let Some(password) = password_opt {
            let salt = SaltString::generate(&mut OsRng);
            let argon2 = Argon2::default();
            Some(
                argon2
                    .hash_password(password.as_bytes(), &salt)
                    .map_err(|e| AuthTokenError::UserServiceError(e.to_string()))?
                    .to_string(),
            )
        } else {
            None
        };

        // Check if pending registration already exists for this email
        let existing = sqlx::query!(
            r#"
            SELECT id FROM pending_registrations WHERE email = ?
            "#,
            email
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(existing_row) = existing {
            // Update existing pending registration with new token and expiry
            let id = existing_row
                .id
                .ok_or(AuthTokenError::DatabaseError(sqlx::Error::RowNotFound))?;
            sqlx::query!(
                r#"
                UPDATE pending_registrations
                SET password_hash = ?, token = ?, expires_at = ?, created_at = CURRENT_TIMESTAMP
                WHERE id = ?
                "#,
                password_hash,
                token,
                expires_at_str,
                id
            )
            .execute(&self.pool)
            .await?;
        } else {
            // Insert new pending registration
            sqlx::query!(
                r#"
                INSERT INTO pending_registrations (email, password_hash, token, expires_at)
                VALUES (?, ?, ?, ?)
                "#,
                email,
                password_hash,
                token,
                expires_at_str
            )
            .execute(&self.pool)
            .await?;
        }

        tracing::info!("Attempting to send verification email to: {}", email);
        match self
            .email_service
            .send_verification_email(email, &token)
            .await
        {
            Ok(_) => {
                tracing::info!("✅ Verification email sent successfully to: {}", email);
                Ok(token)
            }
            Err(e) => {
                tracing::error!("❌ Failed to send verification email to {}: {:?}", email, e);
                Err(e.into())
            }
        }
    }

    pub async fn verify_registration_token(&self, token: &str) -> Result<User, AuthTokenError> {
        let row = sqlx::query!(
            r#"
            SELECT id, email, password_hash, token, expires_at, created_at
            FROM pending_registrations
            WHERE token = ?
            "#,
            token
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(AuthTokenError::TokenNotFound)?;

        let pending = PendingRegistration {
            id: row
                .id
                .ok_or(AuthTokenError::DatabaseError(sqlx::Error::RowNotFound))?,
            email: row.email,
            password_hash: row.password_hash,
            token: row.token,
            expires_at: row
                .expires_at
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|e| AuthTokenError::DatabaseError(sqlx::Error::Decode(Box::new(e))))?,
            created_at: row.created_at.map(|dt| {
                dt.format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default()
            }),
        };

        let expires_at = chrono::DateTime::parse_from_rfc3339(&pending.expires_at)
            .map_err(|e| AuthTokenError::DatabaseError(sqlx::Error::Decode(Box::new(e))))?;

        if expires_at < Utc::now() {
            sqlx::query!("DELETE FROM pending_registrations WHERE id = ?", pending.id)
                .execute(&self.pool)
                .await?;
            return Err(AuthTokenError::TokenNotFound);
        }

        // Check if user already exists
        let existing_user = self
            .user_repository
            .find_by_email(&pending.email)
            .await
            .map_err(|e| AuthTokenError::UserServiceError(e.to_string()))?;

        let user = if let Some(mut user) = existing_user {
            // User exists - verify their email
            self.user_repository
                .verify_email(user.id)
                .await
                .map_err(|e| AuthTokenError::UserServiceError(e.to_string()))?;
            user.email_verified = true;
            user
        } else {
            // User doesn't exist - create new user
            let password_hash = if let Some(hash) = pending.password_hash {
                hash
            } else {
                let random_password = Self::generate_random_password();
                let salt = SaltString::generate(&mut OsRng);
                let argon2 = Argon2::default();
                argon2
                    .hash_password(random_password.as_bytes(), &salt)
                    .map_err(|e| AuthTokenError::UserServiceError(e.to_string()))?
                    .to_string()
            };

            self.user_repository
                .create_user(&pending.email, &password_hash, true)
                .await
                .map_err(|e| AuthTokenError::UserServiceError(e.to_string()))?
        };

        sqlx::query!("DELETE FROM pending_registrations WHERE id = ?", pending.id)
            .execute(&self.pool)
            .await?;

        Ok(user)
    }

    pub async fn create_magic_login_token(&self, user_id: i64) -> Result<String, AuthTokenError> {
        let token = Self::generate_token();
        let expires_at = Utc::now() + Duration::minutes(15);
        let expires_at_str = expires_at.to_rfc3339();

        let user = self
            .user_repository
            .find_by_id(user_id)
            .await
            .map_err(|e| AuthTokenError::UserServiceError(e.to_string()))?
            .ok_or(AuthTokenError::UserNotFound)?;

        sqlx::query!(
            r#"
            INSERT INTO magic_login_tokens (user_id, token, expires_at)
            VALUES (?, ?, ?)
            "#,
            user_id,
            token,
            expires_at_str
        )
        .execute(&self.pool)
        .await?;

        self.email_service
            .send_magic_login_email(&user.email, &token)
            .await?;

        Ok(token)
    }

    pub async fn verify_magic_login_token(&self, token: &str) -> Result<User, AuthTokenError> {
        let row = sqlx::query!(
            r#"
            SELECT id, user_id, token, expires_at, created_at, used_at
            FROM magic_login_tokens
            WHERE token = ?
            "#,
            token
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(AuthTokenError::TokenNotFound)?;

        let magic_token = MagicLoginToken {
            id: row
                .id
                .ok_or(AuthTokenError::DatabaseError(sqlx::Error::RowNotFound))?,
            user_id: row.user_id,
            token: row.token,
            expires_at: row
                .expires_at
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|e| AuthTokenError::DatabaseError(sqlx::Error::Decode(Box::new(e))))?,
            created_at: row.created_at.map(|dt| {
                dt.format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default()
            }),
            used_at: row.used_at.map(|dt| {
                dt.format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default()
            }),
        };

        if magic_token.used_at.is_some() {
            return Err(AuthTokenError::TokenAlreadyUsed);
        }

        let expires_at = chrono::DateTime::parse_from_rfc3339(&magic_token.expires_at)
            .map_err(|e| AuthTokenError::DatabaseError(sqlx::Error::Decode(Box::new(e))))?;

        if expires_at < Utc::now() {
            sqlx::query!(
                "DELETE FROM magic_login_tokens WHERE id = ?",
                magic_token.id
            )
            .execute(&self.pool)
            .await?;
            return Err(AuthTokenError::TokenNotFound);
        }

        let now = Utc::now().to_rfc3339();
        sqlx::query!(
            "UPDATE magic_login_tokens SET used_at = ? WHERE id = ?",
            now,
            magic_token.id
        )
        .execute(&self.pool)
        .await?;

        let user = self
            .user_repository
            .find_by_id(magic_token.user_id)
            .await
            .map_err(|e| AuthTokenError::UserServiceError(e.to_string()))?
            .ok_or(AuthTokenError::UserNotFound)?;

        Ok(user)
    }

    pub async fn cleanup_expired_tokens(&self) -> Result<(), AuthTokenError> {
        let now = Utc::now().to_rfc3339();

        sqlx::query!(
            "DELETE FROM pending_registrations WHERE expires_at < ?",
            now
        )
        .execute(&self.pool)
        .await?;

        sqlx::query!("DELETE FROM magic_login_tokens WHERE expires_at < ?", now)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub fn email_service(&self) -> &dyn EmailService {
        self.email_service.as_ref()
    }
}
