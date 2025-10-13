use axum::{
    extract::Request,
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use tracing::{debug, warn};
use uuid::Uuid;

pub const CSRF_TOKEN_KEY: &str = "csrf_token";
pub const CSRF_HEADER: &str = "X-CSRF-Token";

/// CSRF Token structure for session storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsrfToken {
    pub value: String,
    pub created_at: i64,
}

impl CsrfToken {
    /// Create a new CSRF token
    pub fn new() -> Self {
        Self {
            value: Uuid::new_v4().to_string(),
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    /// Check if token is expired (24 hours)
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        let age = now - self.created_at;
        age > 86400 // 24 hours in seconds
    }
}

impl Default for CsrfToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a new CSRF token and store in session
pub async fn generate_csrf_token(
    session: &Session,
) -> Result<String, tower_sessions::session::Error> {
    let token = CsrfToken::new();
    let value = token.value.clone();

    // Store the token in the session
    session.insert(CSRF_TOKEN_KEY, token).await?;

    debug!("Generated new CSRF token: {}", &value[..8]);
    Ok(value)
}

/// Get or create a CSRF token for the session
pub async fn get_or_create_csrf_token(
    session: &Session,
) -> Result<String, tower_sessions::session::Error> {
    // Try to get existing token
    let token: Option<CsrfToken> = session.get(CSRF_TOKEN_KEY).await?;

    match token {
        Some(existing_token) if !existing_token.is_expired() => {
            debug!("Using existing CSRF token: {}", &existing_token.value[..8]);
            Ok(existing_token.value)
        }
        _ => {
            // Generate new token if none exists or expired
            generate_csrf_token(session).await
        }
    }
}

/// Middleware to validate CSRF tokens on state-changing requests
pub async fn csrf_validation_middleware(
    session: Session,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    // Skip validation for GET, HEAD, OPTIONS requests (safe methods)
    if matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return Ok(next.run(request).await);
    }

    // Skip validation for API endpoints that use different auth (like OAuth)
    if path.starts_with("/api/") || path.starts_with("/oauth/") || path.starts_with("/s/") {
        return Ok(next.run(request).await);
    }

    // Skip validation for public endpoints
    if path == "/contact" {
        return Ok(next.run(request).await);
    }

    // For state-changing methods, validate CSRF token
    debug!("Validating CSRF for {} {}", method, path);

    // Get stored token from session
    let stored_token: Option<CsrfToken> = session.get(CSRF_TOKEN_KEY).await.map_err(|e| {
        warn!("Failed to get CSRF token from session: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let stored_token = match stored_token {
        Some(token) => {
            // Check if token is expired
            if token.is_expired() {
                warn!("CSRF token expired for {} {}", method, path);
                return Err(StatusCode::FORBIDDEN);
            }
            token
        }
        None => {
            warn!("No CSRF token in session for {} {}", method, path);
            return Err(StatusCode::FORBIDDEN);
        }
    };

    // Extract provided token from request
    // First check header (for AJAX requests)
    let provided_token = request
        .headers()
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    // For form submissions, the token will be in the form data
    // Since we can't easily extract form data from the request without consuming it,
    // we'll rely on handlers to validate form tokens themselves
    // The middleware will primarily handle AJAX requests via headers

    // If we have a header token, validate it
    if let Some(token) = provided_token {
        if token != stored_token.value {
            warn!(
                "CSRF token mismatch for {} {}: expected {}, got {}",
                method,
                path,
                &stored_token.value[..8],
                &token[..8]
            );
            return Err(StatusCode::FORBIDDEN);
        }

        // Token is valid - regenerate for next request (replay protection)
        debug!("CSRF token validated, regenerating for replay protection");
        let _ = generate_csrf_token(&session).await;

        return Ok(next.run(request).await);
    }

    // No header token found, pass through to handler for form validation
    // Handlers will need to call validate_csrf_form_field
    Ok(next.run(request).await)
}

/// Helper function for handlers to validate CSRF tokens from form fields
pub async fn validate_csrf_form_field(
    session: &Session,
    form_token: &str,
) -> Result<(), StatusCode> {
    // Get stored token from session
    let stored_token: Option<CsrfToken> = session.get(CSRF_TOKEN_KEY).await.map_err(|e| {
        warn!("Failed to get CSRF token from session: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let stored_token = match stored_token {
        Some(token) => {
            if token.is_expired() {
                warn!("CSRF token expired during form validation");
                return Err(StatusCode::FORBIDDEN);
            }
            token
        }
        None => {
            warn!("No CSRF token in session for form validation");
            return Err(StatusCode::FORBIDDEN);
        }
    };

    // Validate token
    if form_token != stored_token.value {
        warn!(
            "CSRF form token mismatch: expected {}, got {}",
            &stored_token.value[..8],
            &form_token[..8]
        );
        return Err(StatusCode::FORBIDDEN);
    }

    // Token is valid - regenerate for next request (replay protection)
    debug!("CSRF form token validated, regenerating for replay protection");
    let _ = generate_csrf_token(session).await;

    Ok(())
}

/// Response wrapper to include CSRF error message
pub struct CsrfError {
    pub message: String,
}

impl IntoResponse for CsrfError {
    fn into_response(self) -> Response {
        (
            StatusCode::FORBIDDEN,
            format!("CSRF validation failed: {}", self.message),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_sessions::MemoryStore;

    #[tokio::test]
    async fn test_csrf_token_generation() {
        let store = std::sync::Arc::new(MemoryStore::default());
        let session = Session::new(None, store.clone(), None);

        let token1 = generate_csrf_token(&session).await.unwrap();
        assert!(!token1.is_empty());

        let token2 = generate_csrf_token(&session).await.unwrap();
        assert!(!token2.is_empty());
        assert_ne!(token1, token2, "Tokens should be unique");
    }

    #[tokio::test]
    async fn test_csrf_token_expiry() {
        let token = CsrfToken {
            value: "test".to_string(),
            created_at: chrono::Utc::now().timestamp() - 100000, // Old token
        };

        assert!(token.is_expired());

        let fresh_token = CsrfToken::new();
        assert!(!fresh_token.is_expired());
    }

    #[tokio::test]
    async fn test_get_or_create_csrf_token() {
        let store = std::sync::Arc::new(MemoryStore::default());
        let session = Session::new(None, store.clone(), None);

        // First call should create a new token
        let token1 = get_or_create_csrf_token(&session).await.unwrap();

        // Second call should return the same token
        let token2 = get_or_create_csrf_token(&session).await.unwrap();
        assert_eq!(token1, token2);

        // After regeneration, should get a different token
        let _ = generate_csrf_token(&session).await.unwrap();
        let token3 = get_or_create_csrf_token(&session).await.unwrap();
        assert_ne!(token1, token3);
    }
}
