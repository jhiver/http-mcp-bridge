use crate::{error::McpAuthError, models::server::Server, AppState};
use axum::{
    extract::{Path, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use regex::Regex;
use std::borrow::Cow;

/// Extract UUID from path like /s/UUID or /s/UUID/sse
fn extract_uuid_from_path(path: &str) -> Option<String> {
    let re = Regex::new(r"/s/([a-f0-9\-]+)").ok()?;
    re.captures(path)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
}

/// Extract server UUID from request headers for subdomain-based routing
///
/// This function enables subdomain-based MCP routing by extracting the server UUID
/// from request headers. It supports two methods:
///
/// 1. **X-Server-UUID header** (Primary method)
///    - Set by nginx/reverse proxy after extracting from subdomain
///    - Most reliable method
///
/// 2. **Host header parsing** (Fallback method)
///    - Parses subdomain from Host header directly
///    - Pattern: `{uuid}.saramcp.com` or `{uuid}.saramcp.com:port`
///
/// # Arguments
///
/// * `headers` - HTTP request headers
///
/// # Returns
///
/// * `Some(uuid)` - If UUID found via either method
/// * `None` - If no UUID found or invalid format
///
/// # Examples
///
/// ```rust,ignore
/// let mut headers = HeaderMap::new();
/// headers.insert("x-server-uuid", "550e8400-e29b-41d4-a716-446655440000".parse().unwrap());
/// assert_eq!(
///     extract_server_uuid_from_headers(&headers),
///     Some("550e8400-e29b-41d4-a716-446655440000".to_string())
/// );
/// ```
pub fn extract_server_uuid_from_headers(headers: &HeaderMap) -> Option<String> {
    // Method 1: Try X-Server-UUID header (set by nginx)
    if let Some(uuid_header) = headers.get("x-server-uuid") {
        if let Ok(uuid_str) = uuid_header.to_str() {
            // Basic validation: non-empty and reasonable length (UUIDs are 36 chars with hyphens)
            if !uuid_str.is_empty() && uuid_str.len() >= 32 {
                return Some(uuid_str.to_string());
            }
        }
    }

    // Method 2: Parse from Host header as fallback
    if let Some(host_header) = headers.get("host") {
        if let Ok(host_str) = host_header.to_str() {
            // Remove port if present (e.g., "uuid.saramcp.com:8080" -> "uuid.saramcp.com")
            let host_without_port = host_str.split(':').next().unwrap_or(host_str);

            // Match pattern: {uuid}.saramcp.com
            // The UUID should be at least 32 chars (without hyphens) and not contain dots
            if let Some(uuid) = host_without_port.strip_suffix(".saramcp.com") {
                // Validate it's a UUID-like string (contains only hex chars and hyphens, no dots)
                if !uuid.contains('.') && uuid.len() >= 32 {
                    return Some(uuid.to_string());
                }
            }
        }
    }

    None
}

/// Extract Bearer token from Authorization header
fn extract_bearer_token(headers: &HeaderMap) -> Result<String, McpAuthError> {
    let auth_header = headers
        .get("authorization")
        .ok_or(McpAuthError::MissingAuthorizationHeader)?
        .to_str()
        .map_err(|_| McpAuthError::InvalidAuthorizationFormat)?;

    if !auth_header.starts_with("Bearer ") {
        return Err(McpAuthError::InvalidAuthorizationFormat);
    }

    Ok(auth_header["Bearer ".len()..].to_string())
}

/// MCP authentication middleware with three-tier access control
///
/// Access levels:
/// - public: No authentication required
/// - organization: Requires valid OAuth token (any user)
/// - private: Requires valid OAuth token + ownership check
pub async fn mcp_auth_middleware(
    State(state): State<AppState>,
    Path(server_uuid): Path<String>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, McpAuthError> {
    // 1. Get server by UUID
    let server = Server::get_by_uuid(&state.pool, &server_uuid)
        .await?
        .ok_or(McpAuthError::ServerNotFound)?;

    // 2. Apply three-tier access control
    match server.access_level.as_deref() {
        Some("public") | None => {
            // Public servers: no authentication required
            // Just proceed to the handler
            Ok(next.run(request).await)
        }

        Some("organization") => {
            // Organization servers: require valid OAuth token
            let token = match extract_bearer_token(&headers) {
                Ok(token) => token,
                Err(err) => return handle_auth_error(err, &server_uuid, AuthResourceKind::Http),
            };
            let validated = match state.oauth_service.validate_access_token(&token).await {
                Ok(validated) => validated,
                Err(err) => {
                    let mapped: McpAuthError = err.into();
                    return handle_auth_error(mapped, &server_uuid, AuthResourceKind::Http);
                }
            };

            // Attach validated token to request for handlers to use
            request.extensions_mut().insert(validated);

            Ok(next.run(request).await)
        }

        Some("private") => {
            // Private servers: require valid OAuth token + ownership
            let token = match extract_bearer_token(&headers) {
                Ok(token) => token,
                Err(err) => return handle_auth_error(err, &server_uuid, AuthResourceKind::Http),
            };
            let validated = match state.oauth_service.validate_access_token(&token).await {
                Ok(validated) => validated,
                Err(err) => {
                    let mapped: McpAuthError = err.into();
                    return handle_auth_error(mapped, &server_uuid, AuthResourceKind::Http);
                }
            };

            // Check if user owns the server
            let can_access = state
                .oauth_service
                .can_access_server(&server_uuid, validated.user_id)
                .await?;

            if !can_access {
                return Err(McpAuthError::Forbidden);
            }

            // Attach validated token to request for handlers to use
            request.extensions_mut().insert(validated);

            Ok(next.run(request).await)
        }

        _ => {
            // Invalid access level
            Err(McpAuthError::ServiceError(anyhow::anyhow!(
                "Invalid access level"
            )))
        }
    }
}

/// MCP authentication middleware for SSE routes (non-parametric paths)
///
/// This variant extracts the UUID from the path using regex instead of
/// relying on Axum's Path extractor, making it suitable for routes where
/// the UUID is hard-coded in the path (e.g., from rmcp SSE routers).
pub async fn mcp_auth_middleware_sse(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, McpAuthError> {
    // 1. Extract server UUID from path
    let path = request.uri().path();
    let server_uuid = extract_uuid_from_path(path)
        .ok_or_else(|| McpAuthError::ServiceError(anyhow::anyhow!("Invalid path format")))?;

    // 2. Get server by UUID
    let server = Server::get_by_uuid(&state.pool, &server_uuid)
        .await?
        .ok_or(McpAuthError::ServerNotFound)?;

    // 3. Apply three-tier access control
    match server.access_level.as_deref() {
        Some("public") | None => {
            // Public servers: no authentication required
            Ok(next.run(request).await)
        }

        Some("organization") => {
            // Organization servers: require valid OAuth token
            let token = match extract_bearer_token(&headers) {
                Ok(token) => token,
                Err(err) => return handle_auth_error(err, &server_uuid, AuthResourceKind::Sse),
            };
            let validated = match state.oauth_service.validate_access_token(&token).await {
                Ok(validated) => validated,
                Err(err) => {
                    let mapped: McpAuthError = err.into();
                    return handle_auth_error(mapped, &server_uuid, AuthResourceKind::Sse);
                }
            };

            // Attach validated token to request for handlers to use
            request.extensions_mut().insert(validated);

            Ok(next.run(request).await)
        }

        Some("private") => {
            // Private servers: require valid OAuth token + ownership
            let token = match extract_bearer_token(&headers) {
                Ok(token) => token,
                Err(err) => return handle_auth_error(err, &server_uuid, AuthResourceKind::Sse),
            };
            let validated = match state.oauth_service.validate_access_token(&token).await {
                Ok(validated) => validated,
                Err(err) => {
                    let mapped: McpAuthError = err.into();
                    return handle_auth_error(mapped, &server_uuid, AuthResourceKind::Sse);
                }
            };

            // Check if user owns the server
            let can_access = state
                .oauth_service
                .can_access_server(&server_uuid, validated.user_id)
                .await?;

            if !can_access {
                return Err(McpAuthError::Forbidden);
            }

            // Attach validated token to request for handlers to use
            request.extensions_mut().insert(validated);

            Ok(next.run(request).await)
        }

        _ => {
            // Invalid access level
            Err(McpAuthError::ServiceError(anyhow::anyhow!(
                "Invalid access level"
            )))
        }
    }
}

/// MCP authentication middleware for subdomain-based routing
///
/// This variant extracts the UUID from request headers instead of the path,
/// enabling subdomain-based routing like `https://{uuid}.saramcp.com/`.
///
/// **Main Domain Passthrough**: If no UUID is found in headers AND the request
/// is to the main domain (saramcp.com, www.saramcp.com, localhost), the middleware
/// passes through to allow homepage/UI handlers to process the request.
///
/// # UUID Extraction
///
/// Uses `extract_server_uuid_from_headers()` which checks:
/// 1. X-Server-UUID header (set by nginx)
/// 2. Host header parsing (fallback)
///
/// # Access Control
///
/// Applies the same three-tier access control as other middleware variants:
/// - public: No authentication required
/// - organization: Requires valid OAuth token
/// - private: Requires valid OAuth token + ownership check
pub async fn mcp_auth_middleware_subdomain(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, McpAuthError> {
    // 1. Extract server UUID from headers
    let server_uuid = match extract_server_uuid_from_headers(&headers) {
        Some(uuid) => uuid,
        None => {
            // No UUID found - check if this is a main domain request
            // If so, pass through to homepage/UI handlers
            if let Some(host_header) = headers.get("host") {
                if let Ok(host_str) = host_header.to_str() {
                    let host_without_port = host_str.split(':').next().unwrap_or(host_str);

                    // Allow main domain requests to pass through
                    if host_without_port == "saramcp.com"
                        || host_without_port == "www.saramcp.com"
                        || host_without_port == "localhost"
                        || host_without_port == "127.0.0.1"
                    {
                        return Ok(next.run(request).await);
                    }
                }
            }

            // Not a main domain request but no UUID - this is an error
            return Err(McpAuthError::ServiceError(anyhow::anyhow!(
                "No server UUID found in headers for subdomain request"
            )));
        }
    };

    // 2. Get server by UUID
    let server = Server::get_by_uuid(&state.pool, &server_uuid)
        .await?
        .ok_or(McpAuthError::ServerNotFound)?;

    // 3. Apply three-tier access control
    match server.access_level.as_deref() {
        Some("public") | None => {
            // Public servers: no authentication required
            Ok(next.run(request).await)
        }

        Some("organization") => {
            // Organization servers: require valid OAuth token
            let token = match extract_bearer_token(&headers) {
                Ok(token) => token,
                Err(err) => {
                    return handle_auth_error(err, &server_uuid, AuthResourceKind::Subdomain)
                }
            };
            let validated = match state.oauth_service.validate_access_token(&token).await {
                Ok(validated) => validated,
                Err(err) => {
                    let mapped: McpAuthError = err.into();
                    return handle_auth_error(mapped, &server_uuid, AuthResourceKind::Subdomain);
                }
            };

            // Attach validated token to request for handlers to use
            request.extensions_mut().insert(validated);

            Ok(next.run(request).await)
        }

        Some("private") => {
            // Private servers: require valid OAuth token + ownership
            let token = match extract_bearer_token(&headers) {
                Ok(token) => token,
                Err(err) => {
                    return handle_auth_error(err, &server_uuid, AuthResourceKind::Subdomain)
                }
            };
            let validated = match state.oauth_service.validate_access_token(&token).await {
                Ok(validated) => validated,
                Err(err) => {
                    let mapped: McpAuthError = err.into();
                    return handle_auth_error(mapped, &server_uuid, AuthResourceKind::Subdomain);
                }
            };

            // Check if user owns the server
            let can_access = state
                .oauth_service
                .can_access_server(&server_uuid, validated.user_id)
                .await?;

            if !can_access {
                return Err(McpAuthError::Forbidden);
            }

            // Attach validated token to request for handlers to use
            request.extensions_mut().insert(validated);

            Ok(next.run(request).await)
        }

        _ => {
            // Invalid access level
            Err(McpAuthError::ServiceError(anyhow::anyhow!(
                "Invalid access level"
            )))
        }
    }
}

#[derive(Clone, Copy)]
enum AuthResourceKind {
    Http,
    Sse,
    Subdomain,
}

fn handle_auth_error(
    err: McpAuthError,
    server_uuid: &str,
    resource_kind: AuthResourceKind,
) -> Result<Response, McpAuthError> {
    match err {
        McpAuthError::MissingAuthorizationHeader
        | McpAuthError::InvalidAuthorizationFormat
        | McpAuthError::InvalidToken
        | McpAuthError::ExpiredToken => Ok(respond_with_bearer_challenge(
            err,
            server_uuid,
            resource_kind,
        )),
        other => Err(other),
    }
}

fn respond_with_bearer_challenge(
    error: McpAuthError,
    server_uuid: &str,
    resource_kind: AuthResourceKind,
) -> Response {
    let (status, error_code, description) = error.describe();
    let mut response = error.into_response();

    if status == StatusCode::UNAUTHORIZED {
        let (resource, resource_metadata) = compute_resource_urls(server_uuid, resource_kind);
        let description = sanitize_header_value(description);
        let header_value = format!(
            r#"Bearer realm="{}", error="{}", error_description="{}", resource="{}", resource_metadata="{}""#,
            AUTH_REALM, error_code, description, resource, resource_metadata
        );

        if let Ok(value) = HeaderValue::from_str(&header_value) {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, value);
        }
    }

    response
}

fn compute_resource_urls(server_uuid: &str, kind: AuthResourceKind) -> (String, String) {
    match kind {
        AuthResourceKind::Http => {
            let base = canonical_base_url();
            let resource = format!("{}/s/{}", base, server_uuid);
            let metadata = format!(
                "{}/.well-known/oauth-protected-resource/s/{}",
                base, server_uuid
            );
            (resource, metadata)
        }
        AuthResourceKind::Sse => {
            let base = canonical_base_url();
            let resource = format!("{}/s/{}/sse", base, server_uuid);
            let metadata = format!(
                "{}/.well-known/oauth-protected-resource/s/{}",
                base, server_uuid
            );
            (resource, metadata)
        }
        AuthResourceKind::Subdomain => {
            let subdomain = format!("https://{}.saramcp.com", server_uuid);
            let resource = format!("{}/", subdomain.trim_end_matches('/'));
            let metadata = format!("{}/.well-known/oauth-protected-resource", subdomain);
            (resource, metadata)
        }
    }
}

fn canonical_base_url() -> String {
    let raw = std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let trimmed = raw.trim_end_matches('/').to_string();

    if trimmed.starts_with("http://") {
        let host = trimmed.trim_start_matches("http://");
        if host.starts_with("localhost") || host.starts_with("127.0.0.1") {
            trimmed
        } else {
            format!("https://{}", host)
        }
    } else {
        trimmed
    }
}

fn sanitize_header_value(input: &str) -> Cow<'_, str> {
    if input.contains('"') {
        Cow::Owned(input.replace('"', "'"))
    } else {
        Cow::Borrowed(input)
    }
}

const AUTH_REALM: &str = "saramcp";

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn test_extract_uuid_from_x_server_uuid_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-server-uuid",
            HeaderValue::from_static("550e8400-e29b-41d4-a716-446655440000"),
        );

        let result = extract_server_uuid_from_headers(&headers);
        assert_eq!(
            result,
            Some("550e8400-e29b-41d4-a716-446655440000".to_string())
        );
    }

    #[test]
    fn test_extract_uuid_from_host_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "host",
            HeaderValue::from_static("550e8400-e29b-41d4-a716-446655440000.saramcp.com"),
        );

        let result = extract_server_uuid_from_headers(&headers);
        assert_eq!(
            result,
            Some("550e8400-e29b-41d4-a716-446655440000".to_string())
        );
    }

    #[test]
    fn test_extract_uuid_from_host_header_with_port() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "host",
            HeaderValue::from_static("550e8400-e29b-41d4-a716-446655440000.saramcp.com:8080"),
        );

        let result = extract_server_uuid_from_headers(&headers);
        assert_eq!(
            result,
            Some("550e8400-e29b-41d4-a716-446655440000".to_string())
        );
    }

    #[test]
    fn test_extract_uuid_prefers_x_server_uuid_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-server-uuid",
            HeaderValue::from_static("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
        );
        headers.insert(
            "host",
            HeaderValue::from_static("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb.saramcp.com"),
        );

        let result = extract_server_uuid_from_headers(&headers);
        // Should prefer X-Server-UUID header
        assert_eq!(
            result,
            Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_string())
        );
    }

    #[test]
    fn test_extract_uuid_no_headers() {
        let headers = HeaderMap::new();
        let result = extract_server_uuid_from_headers(&headers);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_uuid_main_domain_returns_none() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("saramcp.com"));

        let result = extract_server_uuid_from_headers(&headers);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_uuid_www_subdomain_returns_none() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("www.saramcp.com"));

        let result = extract_server_uuid_from_headers(&headers);
        // "www" is too short (< 32 chars), should return None
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_uuid_wrong_domain_returns_none() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "host",
            HeaderValue::from_static("550e8400-e29b-41d4-a716-446655440000.example.com"),
        );

        let result = extract_server_uuid_from_headers(&headers);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_uuid_empty_header_returns_none() {
        let mut headers = HeaderMap::new();
        headers.insert("x-server-uuid", HeaderValue::from_static(""));

        let result = extract_server_uuid_from_headers(&headers);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_uuid_too_short_returns_none() {
        let mut headers = HeaderMap::new();
        headers.insert("x-server-uuid", HeaderValue::from_static("short-uuid"));

        let result = extract_server_uuid_from_headers(&headers);
        // "short-uuid" is only 10 chars, should return None
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_uuid_localhost_returns_none() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("localhost"));

        let result = extract_server_uuid_from_headers(&headers);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_uuid_localhost_with_port_returns_none() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("localhost:8080"));

        let result = extract_server_uuid_from_headers(&headers);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_uuid_ip_address_returns_none() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("127.0.0.1"));

        let result = extract_server_uuid_from_headers(&headers);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_uuid_ip_address_with_port_returns_none() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("127.0.0.1:8080"));

        let result = extract_server_uuid_from_headers(&headers);
        assert_eq!(result, None);
    }
}
