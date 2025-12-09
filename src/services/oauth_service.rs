use crate::models::oauth::{
    OAuthAccessToken, OAuthAuthorizationCode, OAuthClient, OAuthRefreshToken,
};
use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Clone)]
pub struct OAuthService {
    pool: SqlitePool,
}

impl OAuthService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Register a new OAuth client
    /// user_id is optional - None for public/anonymous registration
    pub async fn register_client(
        &self,
        user_id: Option<i64>,
        request: ClientRegistrationRequest,
    ) -> Result<ClientRegistrationResponse> {
        // Validate request
        self.validate_registration_request(&request)?;

        // Create OAuth client (generates client_id and secret)
        let redirect_uris_json = serde_json::to_string(&request.redirect_uris)?;
        let (client_id, client_secret) = OAuthClient::create(
            &self.pool,
            user_id,
            &request.client_name,
            &redirect_uris_json,
        )
        .await?;

        // Build RFC 7591 compliant response
        let now = chrono::Utc::now().timestamp();
        Ok(ClientRegistrationResponse {
            client_id,
            client_secret,
            client_name: request.client_name,
            redirect_uris: request.redirect_uris,
            client_id_issued_at: now,
            client_secret_expires_at: 0, // 0 means never expires
        })
    }

    /// Register OAuth client with client-provided ID (for auto-registration)
    /// This is used when MCP Inspector or similar tools provide their own client_id
    pub async fn register_client_with_id(
        &self,
        client_id: &str,
        user_id: Option<i64>,
        request: ClientRegistrationRequest,
    ) -> Result<ClientRegistrationResponse> {
        // Validate request
        self.validate_registration_request(&request)?;

        // Generate client secret
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
        let mut secret_bytes = [0u8; 32];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut secret_bytes);
        let client_secret = BASE64.encode(secret_bytes);

        // Hash the secret
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(client_secret.as_bytes());
        let secret_hash = format!("{:x}", hasher.finalize());

        // Insert into database
        let redirect_uris_json = serde_json::to_string(&request.redirect_uris)?;
        sqlx::query!(
            r#"
            INSERT INTO oauth_clients (client_id, client_secret_hash, user_id, name, redirect_uris)
            VALUES (?, ?, ?, ?, ?)
            "#,
            client_id,
            secret_hash,
            user_id,
            request.client_name,
            redirect_uris_json
        )
        .execute(&self.pool)
        .await?;

        // Build RFC 7591 compliant response
        let now = chrono::Utc::now().timestamp();
        Ok(ClientRegistrationResponse {
            client_id: client_id.to_string(),
            client_secret,
            client_name: request.client_name,
            redirect_uris: request.redirect_uris,
            client_id_issued_at: now,
            client_secret_expires_at: 0, // 0 means never expires
        })
    }

    /// Validate client registration request per RFC 7591
    fn validate_registration_request(&self, request: &ClientRegistrationRequest) -> Result<()> {
        // Client name is required
        if request.client_name.trim().is_empty() {
            bail!("client_name is required");
        }

        // At least one redirect URI is required
        if request.redirect_uris.is_empty() {
            bail!("At least one redirect_uri is required");
        }

        // Validate each redirect URI
        for uri in &request.redirect_uris {
            self.validate_redirect_uri(uri)?;
        }

        Ok(())
    }

    /// Validate redirect URI for security
    fn validate_redirect_uri(&self, uri: &str) -> Result<()> {
        // URI must not be empty
        if uri.trim().is_empty() {
            bail!("redirect_uri cannot be empty");
        }

        // Parse and validate URI format
        let parsed = reqwest::Url::parse(uri).map_err(|e| anyhow::anyhow!("Invalid URI: {}", e))?;

        // Reject javascript: and data: URIs (XSS risk)
        let scheme = parsed.scheme();
        if scheme == "javascript" || scheme == "data" {
            bail!("Unsupported URI scheme: {}", scheme);
        }

        // Allow http/https (common OAuth schemes)
        // Also allow custom schemes for native apps (e.g., myapp://)
        if !["http", "https"].contains(&scheme) && !scheme.contains('-') {
            // Custom schemes often use dashes, e.g., com.example.app://
            if !scheme.chars().all(|c| c.is_alphanumeric() || c == '.') {
                bail!("Invalid URI scheme: {}", scheme);
            }
        }

        Ok(())
    }

    /// Get OAuth client by client_id
    pub async fn get_client(&self, client_id: &str) -> Result<Option<OAuthClient>> {
        OAuthClient::get_by_client_id(&self.pool, client_id).await
    }

    /// Validate client credentials (client_id + client_secret)
    pub async fn verify_client_credentials(
        &self,
        client_id: &str,
        client_secret: &str,
    ) -> Result<OAuthClient> {
        let client = self
            .get_client(client_id)
            .await?
            .ok_or_else(|| anyhow!("Invalid client_id"))?;

        if !client.verify_secret(client_secret) {
            bail!("Invalid client secret");
        }

        Ok(client)
    }

    /// Create authorization code with 10 minute expiry
    pub async fn create_authorization_code(
        &self,
        client_id: &str,
        user_id: i64,
        redirect_uri: &str,
        scope: &str,
        code_challenge: Option<&str>,
        code_challenge_method: Option<&str>,
    ) -> Result<String> {
        OAuthAuthorizationCode::create(
            &self.pool,
            client_id,
            user_id,
            redirect_uri,
            scope,
            code_challenge,
            code_challenge_method,
        )
        .await
    }

    /// Validate and consume authorization code
    pub async fn consume_authorization_code(
        &self,
        code: &str,
        client_id: &str,
        redirect_uri: &str,
    ) -> Result<ConsumedCode> {
        // 1. Get authorization code
        let auth_code = OAuthAuthorizationCode::get_by_code(&self.pool, code)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Invalid authorization code"))?;

        // 2. Validate client_id
        if auth_code.client_id != client_id {
            bail!("Client mismatch");
        }

        // 3. Validate redirect_uri
        if auth_code.redirect_uri != redirect_uri {
            bail!("Redirect URI mismatch");
        }

        // 4. Check expiry
        if auth_code.is_expired() {
            bail!("Authorization code expired");
        }

        // 5. Check single-use
        if auth_code.is_used() {
            bail!("Authorization code already used");
        }

        // 6. Mark as used
        auth_code.mark_used(&self.pool).await?;

        Ok(ConsumedCode {
            user_id: auth_code.user_id,
            scope: auth_code.scope,
            code_challenge: auth_code.code_challenge,
            code_challenge_method: auth_code.code_challenge_method,
        })
    }

    /// Validate PKCE code_verifier against challenge
    pub fn validate_pkce(&self, verifier: &str, challenge: &str, method: &str) -> Result<()> {
        let computed = if method == "S256" {
            // Compute SHA256 hash of verifier
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(verifier.as_bytes());

            // Base64 URL-safe encode (no padding)
            use base64::Engine;
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize())
        } else {
            // Plain method: verifier is the challenge
            verifier.to_string()
        };

        if computed != challenge {
            bail!("Invalid code_verifier");
        }

        Ok(())
    }

    /// Generate access token (1 hour expiry)
    pub async fn create_access_token(
        &self,
        client_id: &str,
        user_id: i64,
        scope: &str,
    ) -> Result<(String, i64)> {
        use sha2::{Digest, Sha256};

        // Generate token
        let token = format!("mcp_token_{}", uuid::Uuid::new_v4());

        // Hash for storage
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let token_hash = format!("{:x}", hasher.finalize());

        // 1 hour expiry
        let now = chrono::Utc::now().timestamp();
        let expires_at = now + 3600; // 1 hour in seconds

        // Insert into database
        sqlx::query!(
            r#"
            INSERT INTO oauth_access_tokens (token_hash, client_id, user_id, scope, expires_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
            token_hash,
            client_id,
            user_id,
            scope,
            expires_at
        )
        .execute(&self.pool)
        .await?;

        Ok((token, expires_at))
    }

    /// Generate refresh token (30 day expiry)
    pub async fn create_refresh_token(
        &self,
        client_id: &str,
        user_id: i64,
        scope: &str,
    ) -> Result<String> {
        use sha2::{Digest, Sha256};

        // Generate token
        let token = format!("mcp_refresh_{}", uuid::Uuid::new_v4());

        // Hash for storage
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let token_hash = format!("{:x}", hasher.finalize());

        // 30 day expiry
        let now = chrono::Utc::now().timestamp();
        let expires_at = now + (30 * 24 * 3600); // 30 days in seconds

        // Insert into database
        sqlx::query!(
            r#"
            INSERT INTO oauth_refresh_tokens (token_hash, client_id, user_id, scope, expires_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
            token_hash,
            client_id,
            user_id,
            scope,
            expires_at
        )
        .execute(&self.pool)
        .await?;

        Ok(token)
    }

    /// Validate and consume refresh token
    pub async fn consume_refresh_token(&self, refresh_token: &str) -> Result<ConsumedRefreshToken> {
        use sha2::{Digest, Sha256};

        // Hash incoming token
        let mut hasher = Sha256::new();
        hasher.update(refresh_token.as_bytes());
        let token_hash = format!("{:x}", hasher.finalize());

        // Lookup refresh token
        let stored = sqlx::query_as!(
            OAuthRefreshToken,
            r#"
            SELECT id as "id!: i64", token_hash, client_id, user_id, scope, expires_at, used_at, created_at as "created_at!: i64"
            FROM oauth_refresh_tokens
            WHERE token_hash = ?
            "#,
            token_hash
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Invalid refresh token"))?;

        // Check expiry
        let now = chrono::Utc::now().timestamp();
        if now > stored.expires_at {
            bail!("Refresh token expired");
        }

        // Check if already used (rotation detection)
        if stored.used_at.is_some() {
            bail!("Refresh token already used");
        }

        // Mark as used
        sqlx::query!(
            "UPDATE oauth_refresh_tokens SET used_at = unixepoch() WHERE token_hash = ?",
            token_hash
        )
        .execute(&self.pool)
        .await?;

        Ok(ConsumedRefreshToken {
            client_id: stored.client_id,
            user_id: stored.user_id,
            scope: stored.scope,
        })
    }

    /// Hash a token using SHA-256 (shared utility for access/refresh tokens)
    pub fn hash_token(&self, token: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Validate an access token and return token information
    ///
    /// Returns Ok(ValidatedToken) if valid
    /// Returns Err if token is invalid, expired, or not found
    pub async fn validate_access_token(&self, token: &str) -> Result<ValidatedToken> {
        // 1. Hash the incoming token (SHA-256 hex)
        let token_hash = self.hash_token(token);

        // 2. Query database for token
        let token_record = sqlx::query_as::<_, OAuthAccessToken>(
            r#"
            SELECT id, token_hash, client_id, user_id, scope, expires_at, created_at
            FROM oauth_access_tokens
            WHERE token_hash = ?
            "#,
        )
        .bind(&token_hash)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| anyhow!("Invalid access token"))?;

        // 3. Check if expired
        let now = Utc::now().timestamp();
        if token_record.expires_at < now {
            return Err(anyhow!("Access token expired"));
        }

        // 4. Update last_used_at (fire and forget, don't block)
        let pool = self.pool.clone();
        let hash = token_hash.clone();
        tokio::spawn(async move {
            let _ =
                sqlx::query("UPDATE oauth_access_tokens SET last_used_at = ? WHERE token_hash = ?")
                    .bind(Utc::now().timestamp())
                    .bind(hash)
                    .execute(&pool)
                    .await;
        });

        // 5. Return validated token info
        Ok(ValidatedToken {
            user_id: token_record.user_id,
            client_id: token_record.client_id,
            scope: token_record.scope,
            expires_at: token_record.expires_at,
        })
    }

    /// Check if a user can access a server based on access level
    ///
    /// Access levels:
    /// - "public" or None: Always allow
    /// - "organization": Allow any authenticated user
    /// - "private": Only allow owner
    pub async fn can_access_server(&self, server_uuid: &str, user_id: i64) -> Result<bool> {
        use crate::models::server::Server;

        // 1. Get server by UUID
        let server = Server::get_by_uuid(&self.pool, server_uuid)
            .await?
            .ok_or_else(|| anyhow!("Server not found"))?;

        // 2. Check access level
        match server.access_level.as_deref() {
            Some("public") | None => {
                // Public servers: anyone can access
                Ok(true)
            }
            Some("organization") => {
                // Organization servers: any authenticated user can access
                Ok(true)
            }
            Some("private") => {
                // Private servers: only owner can access
                Ok(user_id == server.user_id)
            }
            _ => {
                // Invalid access level
                Err(anyhow!("Invalid access level"))
            }
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Parse scopes from space-separated string
pub fn parse_scopes(scope: &str) -> Vec<String> {
    scope.split_whitespace().map(|s| s.to_string()).collect()
}

// ============================================================================
// Validated Token Info
// ============================================================================

/// Validated token information
#[derive(Debug, Clone)]
pub struct ValidatedToken {
    pub user_id: i64,
    pub client_id: String,
    pub scope: String,
    pub expires_at: i64,
}

// ============================================================================
// Request/Response DTOs
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct ClientRegistrationRequest {
    pub client_name: String,
    pub redirect_uris: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ClientRegistrationResponse {
    pub client_id: String,
    pub client_secret: String, // Only returned once at registration!
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    pub client_id_issued_at: i64,
    pub client_secret_expires_at: i64, // 0 = never expires
}

// ============================================================================
// Helper Structs for Token Exchange
// ============================================================================

#[derive(Debug)]
pub struct ConsumedCode {
    pub user_id: i64,
    pub scope: String,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
}

#[derive(Debug)]
pub struct ConsumedRefreshToken {
    pub client_id: String,
    pub user_id: i64,
    pub scope: String,
}
