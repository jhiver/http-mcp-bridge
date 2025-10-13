use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

// ============================================================================
// OAuthClient - Registered OAuth applications
// ============================================================================

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct OAuthClient {
    pub id: Option<i64>,
    pub client_id: String,
    #[serde(skip_serializing)]
    pub client_secret_hash: String,
    pub user_id: Option<i64>, // Nullable - supports public client registration
    pub name: String,
    pub redirect_uris: String, // JSON array
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

impl OAuthClient {
    /// Generate client_id, hash secret, and insert into database
    /// user_id is optional - None for public/anonymous registration
    pub async fn create(
        pool: &SqlitePool,
        user_id: Option<i64>,
        name: &str,
        redirect_uris: &str,
    ) -> Result<(String, String)> {
        let client_id = format!("mcp_{}", Uuid::new_v4());
        let client_secret = Self::generate_secret();
        let secret_hash = Self::hash_secret(&client_secret);

        sqlx::query!(
            r#"
            INSERT INTO oauth_clients (client_id, client_secret_hash, user_id, name, redirect_uris)
            VALUES (?, ?, ?, ?, ?)
            "#,
            client_id,
            secret_hash,
            user_id,
            name,
            redirect_uris
        )
        .execute(pool)
        .await?;

        Ok((client_id, client_secret))
    }

    /// Fetch client by client_id
    pub async fn get_by_client_id(pool: &SqlitePool, client_id: &str) -> Result<Option<Self>> {
        let client = sqlx::query_as!(
            OAuthClient,
            r#"
            SELECT id, client_id, client_secret_hash, user_id, name, redirect_uris, created_at, updated_at
            FROM oauth_clients
            WHERE client_id = ?
            "#,
            client_id
        )
        .fetch_optional(pool)
        .await?;

        Ok(client)
    }

    /// List all clients for a user
    pub async fn list_by_user(pool: &SqlitePool, user_id: i64) -> Result<Vec<Self>> {
        let clients = sqlx::query_as!(
            OAuthClient,
            r#"
            SELECT id, client_id, client_secret_hash, user_id, name, redirect_uris, created_at, updated_at
            FROM oauth_clients
            WHERE user_id = ?
            ORDER BY created_at DESC
            "#,
            user_id
        )
        .fetch_all(pool)
        .await?;

        Ok(clients)
    }

    /// Verify client secret by hashing and comparing
    pub fn verify_secret(&self, secret: &str) -> bool {
        let hash = Self::hash_secret(secret);
        hash == self.client_secret_hash
    }

    /// Generate a secure random client secret (base64 encoded)
    fn generate_secret() -> String {
        let mut bytes = [0u8; 32];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut bytes);
        BASE64.encode(bytes)
    }

    /// Hash a secret using SHA-256
    fn hash_secret(secret: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(secret.as_bytes());
        hex::encode(hasher.finalize())
    }
}

// ============================================================================
// OAuthAuthorizationCode - Authorization codes with PKCE
// ============================================================================

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct OAuthAuthorizationCode {
    pub id: Option<i64>,
    pub code: String,
    pub client_id: String,
    pub user_id: i64,
    pub redirect_uri: String,
    pub scope: String,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub expires_at: i64,
    pub used: i64, // SQLite doesn't have boolean
    pub created_at: Option<i64>,
}

impl OAuthAuthorizationCode {
    /// Create authorization code with 10 minute expiry
    pub async fn create(
        pool: &SqlitePool,
        client_id: &str,
        user_id: i64,
        redirect_uri: &str,
        scope: &str,
        code_challenge: Option<&str>,
        code_challenge_method: Option<&str>,
    ) -> Result<String> {
        let code = format!("code_{}", Uuid::new_v4());
        let now = chrono::Utc::now().timestamp();
        let expires_at = now + 600; // 10 minutes

        sqlx::query!(
            r#"
            INSERT INTO oauth_authorization_codes
            (code, client_id, user_id, redirect_uri, scope, code_challenge, code_challenge_method, expires_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            code,
            client_id,
            user_id,
            redirect_uri,
            scope,
            code_challenge,
            code_challenge_method,
            expires_at
        )
        .execute(pool)
        .await?;

        Ok(code)
    }

    /// Fetch authorization code by code value
    pub async fn get_by_code(pool: &SqlitePool, code: &str) -> Result<Option<Self>> {
        let auth_code = sqlx::query_as!(
            OAuthAuthorizationCode,
            r#"
            SELECT id, code, client_id, user_id, redirect_uri, scope,
                   code_challenge, code_challenge_method, expires_at,
                   used as "used!: i64", created_at
            FROM oauth_authorization_codes
            WHERE code = ?
            "#,
            code
        )
        .fetch_optional(pool)
        .await?;

        Ok(auth_code)
    }

    /// Mark authorization code as used (prevent replay attacks)
    pub async fn mark_used(&self, pool: &SqlitePool) -> Result<()> {
        sqlx::query!(
            "UPDATE oauth_authorization_codes SET used = 1 WHERE code = ?",
            self.code
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Check if code is expired
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        now > self.expires_at
    }

    /// Check if code has been used
    pub fn is_used(&self) -> bool {
        self.used == 1
    }
}

// ============================================================================
// OAuthAccessToken - Access tokens (hashed)
// ============================================================================

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OAuthAccessToken {
    pub id: i64,
    pub token_hash: String,
    pub client_id: String,
    pub user_id: i64,
    pub scope: String,
    pub expires_at: i64,
    pub created_at: i64,
}

// ============================================================================
// OAuthRefreshToken - Refresh tokens with rotation
// ============================================================================

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OAuthRefreshToken {
    pub id: i64,
    pub token_hash: String,
    pub client_id: String,
    pub user_id: i64,
    pub scope: String,
    pub expires_at: i64,
    pub used_at: Option<i64>,
    pub created_at: i64,
}
