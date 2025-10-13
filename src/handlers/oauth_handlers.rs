use crate::services::{oauth_service::parse_scopes, ClientRegistrationRequest};
use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::http::{header, HeaderMap, StatusCode};
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse, Json, Redirect, Response},
    Form,
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

/// POST /.oauth/register - Register a new OAuth client
///
/// Implements RFC 7591 (OAuth 2.0 Dynamic Client Registration Protocol)
///
/// ## Authentication
/// No authentication required - supports public client registration
///
/// ## Request Body (JSON)
/// ```json
/// {
///   "client_name": "My Application",
///   "redirect_uris": ["http://localhost:3000/callback"]
/// }
/// ```
///
/// ## Response (201 CREATED)
/// ```json
/// {
///   "client_id": "mcp_<uuid>",
///   "client_secret": "<base64-encoded-secret>",
///   "client_name": "My Application",
///   "redirect_uris": ["http://localhost:3000/callback"],
///   "client_id_issued_at": 1704067200,
///   "client_secret_expires_at": 0
/// }
/// ```
///
/// ## Errors
/// - 400 Bad Request: Invalid request data
/// - 500 Internal Server Error: Server error
pub async fn register_client(
    State(state): State<AppState>,
    Json(request): Json<ClientRegistrationRequest>,
) -> Result<Response, OAuthError> {
    // Call service layer (no user_id - public registration)
    let response = state
        .oauth_service
        .register_client(None, request)
        .await
        .map_err(|e| OAuthError::BadRequest(e.to_string()))?;

    // Build response with CORS headers
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        "*".parse()
            .map_err(|_| OAuthError::InternalError("Invalid CORS header".to_string()))?,
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        "GET, POST, OPTIONS"
            .parse()
            .map_err(|_| OAuthError::InternalError("Invalid methods header".to_string()))?,
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        "Authorization, Content-Type, mcp-protocol-version"
            .parse()
            .map_err(|_| OAuthError::InternalError("Invalid headers value".to_string()))?,
    );

    Ok((StatusCode::CREATED, headers, Json(response)).into_response())
}

/// GET /.oauth/authorize - OAuth authorization endpoint
///
/// Shows consent screen to user after validating client and redirect_uri
pub async fn authorize(
    State(state): State<AppState>,
    session: Session,
    Query(request): Query<AuthorizationRequest>,
) -> Result<Response, OAuthError> {
    // Validate response_type
    if request.response_type != "code" {
        return Err(OAuthError::BadRequest(
            "Only 'code' response type is supported".to_string(),
        ));
    }

    // Validate client exists (auto-register if not found)
    let client = match state
        .oauth_service
        .get_client(&request.client_id)
        .await
        .map_err(|e| OAuthError::InternalError(e.to_string()))?
    {
        Some(client) => client,
        None => {
            // Auto-register unknown client (for MCP Inspector compatibility)
            use crate::services::ClientRegistrationRequest;
            let registration = ClientRegistrationRequest {
                client_name: format!("Auto-registered: {}", &request.client_id[..8]),
                redirect_uris: vec![request.redirect_uri.clone()],
            };

            // Register with the client's provided client_id
            state
                .oauth_service
                .register_client_with_id(&request.client_id, None, registration)
                .await
                .map_err(|e| OAuthError::InternalError(e.to_string()))?;

            // Fetch the newly created client
            state
                .oauth_service
                .get_client(&request.client_id)
                .await
                .map_err(|e| OAuthError::InternalError(e.to_string()))?
                .ok_or_else(|| OAuthError::InternalError("Failed to create client".to_string()))?
        }
    };

    // Validate redirect_uri
    let redirect_uris: Vec<String> = serde_json::from_str(&client.redirect_uris)
        .map_err(|e| OAuthError::InternalError(e.to_string()))?;
    if !redirect_uris.contains(&request.redirect_uri) {
        return Err(OAuthError::BadRequest("Invalid redirect_uri".to_string()));
    }

    // Check user authentication
    let _user_id: i64 = match session.get("user_id").await {
        Ok(Some(id)) => id,
        _ => {
            // Store OAuth return URL in session for after login
            let oauth_params = serde_urlencoded::to_string(&request)
                .map_err(|e| OAuthError::InternalError(e.to_string()))?;
            let return_url = format!("/.oauth/authorize?{}", oauth_params);

            // Redirect to dedicated login page with return_to parameter
            let login_url = format!("/login?return_to={}", urlencoding::encode(&return_url));
            return Ok(Redirect::to(&login_url).into_response());
        }
    };

    // Get user email from session
    let user_email: String = session
        .get("email")
        .await
        .map_err(|e| OAuthError::InternalError(e.to_string()))?
        .ok_or_else(|| OAuthError::InternalError("Email not in session".to_string()))?;

    // Generate CSRF token
    let csrf_token = uuid::Uuid::new_v4().to_string();
    session
        .insert("oauth_csrf", &csrf_token)
        .await
        .map_err(|e| OAuthError::InternalError(e.to_string()))?;

    // Prepare scope (default to "mcp:read" if not provided)
    let scope = request
        .scope
        .clone()
        .unwrap_or_else(|| "mcp:read".to_string());

    // Prepare template context
    let template = ConsentTemplate {
        csrf_token,
        client_name: client.name,
        client_id: request.client_id,
        redirect_uri: request.redirect_uri,
        response_type: request.response_type,
        scope: scope.clone(),
        scopes: parse_scopes(&scope),
        state: request.state,
        code_challenge: request.code_challenge,
        code_challenge_method: request.code_challenge_method,
        user_email,
    };

    // Render consent screen
    Ok(Html(
        template
            .render()
            .map_err(|e| OAuthError::InternalError(e.to_string()))?,
    )
    .into_response())
}

/// POST /.oauth/authorize - Process consent decision
pub async fn authorize_consent(
    State(state): State<AppState>,
    session: Session,
    Form(request): Form<AuthorizeConsentRequest>,
) -> Result<Response, OAuthError> {
    // Validate CSRF token
    let stored_csrf: Option<String> = session
        .get("oauth_csrf")
        .await
        .map_err(|e| OAuthError::InternalError(e.to_string()))?;
    if stored_csrf != Some(request.csrf_token.clone()) {
        return Err(OAuthError::BadRequest("Invalid CSRF token".to_string()));
    }

    // Clear CSRF token after use
    session
        .remove::<String>("oauth_csrf")
        .await
        .map_err(|e| OAuthError::InternalError(e.to_string()))?;

    // Handle denial
    if request.action == "deny" {
        let mut redirect_url = format!("{}?error=access_denied", request.redirect_uri);
        if let Some(state) = &request.state {
            redirect_url.push_str(&format!("&state={}", urlencoding::encode(state)));
        }
        return Ok(Redirect::to(&redirect_url).into_response());
    }

    // Verify user still authenticated
    let user_id: i64 = session
        .get("user_id")
        .await
        .map_err(|e| OAuthError::InternalError(e.to_string()))?
        .ok_or(OAuthError::Unauthorized)?;

    // Generate authorization code
    let code = state
        .oauth_service
        .create_authorization_code(
            &request.client_id,
            user_id,
            &request.redirect_uri,
            &request.scope,
            request.code_challenge.as_deref(),
            request.code_challenge_method.as_deref(),
        )
        .await
        .map_err(|e| OAuthError::InternalError(e.to_string()))?;

    // Redirect to client with code
    let mut redirect_url = format!("{}?code={}", request.redirect_uri, code);
    if let Some(state) = &request.state {
        redirect_url.push_str(&format!("&state={}", urlencoding::encode(state)));
    }
    Ok(Redirect::to(&redirect_url).into_response())
}

/// POST /.oauth/token - OAuth token endpoint
///
/// Supports two grant types:
/// 1. authorization_code - Exchange authorization code for tokens
/// 2. refresh_token - Exchange refresh token for new tokens
pub async fn token(
    State(state): State<AppState>,
    Form(request): Form<TokenRequest>,
) -> Result<Response, OAuthError> {
    match request.grant_type.as_str() {
        "authorization_code" => handle_authorization_code_grant(state, request).await,
        "refresh_token" => handle_refresh_token_grant(state, request).await,
        _ => Err(OAuthError::UnsupportedGrantType),
    }
}

async fn handle_authorization_code_grant(
    state: AppState,
    request: TokenRequest,
) -> Result<Response, OAuthError> {
    // 1. Extract required fields
    let code = request
        .code
        .ok_or_else(|| OAuthError::BadRequest("Missing code".to_string()))?;
    let client_id = request
        .client_id
        .ok_or_else(|| OAuthError::BadRequest("Missing client_id".to_string()))?;
    let redirect_uri = request
        .redirect_uri
        .ok_or_else(|| OAuthError::BadRequest("Missing redirect_uri".to_string()))?;

    // 2. Consume authorization code (validates and marks as used)
    let consumed = state
        .oauth_service
        .consume_authorization_code(&code, &client_id, &redirect_uri)
        .await
        .map_err(|e| OAuthError::InvalidGrant(e.to_string()))?;

    // 3. Validate PKCE if present
    if let Some(challenge) = consumed.code_challenge {
        let verifier = request
            .code_verifier
            .ok_or_else(|| OAuthError::BadRequest("Missing code_verifier".to_string()))?;

        let method = consumed.code_challenge_method.as_deref().unwrap_or("S256");

        state
            .oauth_service
            .validate_pkce(&verifier, &challenge, method)
            .map_err(|e| OAuthError::InvalidGrant(e.to_string()))?;
    }

    // 4. Generate access token (1 hour)
    let (access_token, _expires_at) = state
        .oauth_service
        .create_access_token(&client_id, consumed.user_id, &consumed.scope)
        .await
        .map_err(|e| OAuthError::InternalError(e.to_string()))?;

    // 5. Generate refresh token (30 days)
    let refresh_token = state
        .oauth_service
        .create_refresh_token(&client_id, consumed.user_id, &consumed.scope)
        .await
        .map_err(|e| OAuthError::InternalError(e.to_string()))?;

    // 6. Build response
    let response = TokenResponse {
        access_token,
        token_type: "Bearer".to_string(),
        expires_in: 3600, // 1 hour
        scope: consumed.scope,
        refresh_token: Some(refresh_token),
    };

    // 7. Add CORS headers
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        "*".parse()
            .map_err(|_| OAuthError::InternalError("Invalid CORS header".to_string()))?,
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        "Authorization, Content-Type, mcp-protocol-version"
            .parse()
            .map_err(|_| OAuthError::InternalError("Invalid headers value".to_string()))?,
    );
    headers.insert(
        header::CONTENT_TYPE,
        "application/json"
            .parse()
            .map_err(|_| OAuthError::InternalError("Invalid content type".to_string()))?,
    );

    Ok((StatusCode::OK, headers, Json(response)).into_response())
}

async fn handle_refresh_token_grant(
    state: AppState,
    request: TokenRequest,
) -> Result<Response, OAuthError> {
    // 1. Extract refresh token
    let refresh_token = request
        .refresh_token
        .ok_or_else(|| OAuthError::BadRequest("Missing refresh_token".to_string()))?;

    // 2. Consume refresh token (validates, marks as used)
    let consumed = state
        .oauth_service
        .consume_refresh_token(&refresh_token)
        .await
        .map_err(|e| OAuthError::InvalidGrant(e.to_string()))?;

    // 3. Generate NEW access token (1 hour)
    let (access_token, _expires_at) = state
        .oauth_service
        .create_access_token(&consumed.client_id, consumed.user_id, &consumed.scope)
        .await
        .map_err(|e| OAuthError::InternalError(e.to_string()))?;

    // 4. Generate NEW refresh token (rotation)
    let new_refresh_token = state
        .oauth_service
        .create_refresh_token(&consumed.client_id, consumed.user_id, &consumed.scope)
        .await
        .map_err(|e| OAuthError::InternalError(e.to_string()))?;

    // 5. Build response
    let response = TokenResponse {
        access_token,
        token_type: "Bearer".to_string(),
        expires_in: 3600, // 1 hour
        scope: consumed.scope,
        refresh_token: Some(new_refresh_token),
    };

    // 6. Add CORS headers
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        "*".parse()
            .map_err(|_| OAuthError::InternalError("Invalid CORS header".to_string()))?,
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        "Authorization, Content-Type, mcp-protocol-version"
            .parse()
            .map_err(|_| OAuthError::InternalError("Invalid headers value".to_string()))?,
    );
    headers.insert(
        header::CONTENT_TYPE,
        "application/json"
            .parse()
            .map_err(|_| OAuthError::InternalError("Invalid content type".to_string()))?,
    );

    Ok((StatusCode::OK, headers, Json(response)).into_response())
}

/// GET /.well-known/oauth-authorization-server
///
/// Returns OAuth 2.1 server metadata for client auto-configuration
///
/// This endpoint detects if it's being accessed via subdomain (e.g., {uuid}.saramcp.com)
/// or main domain (e.g., saramcp.com) and returns appropriate OAuth endpoints.
/// This ensures the OAuth flow stays on the same domain to avoid cross-domain issues.
pub async fn authorization_server_metadata(
    headers: HeaderMap,
    State(_state): State<AppState>,
) -> Response {
    // Detect if this is a subdomain request
    let base_url = if let Some(uuid) = crate::middleware::extract_server_uuid_from_headers(&headers)
    {
        // Subdomain request - OAuth flow happens on subdomain
        format!("https://{}.saramcp.com", uuid)
    } else {
        // Main domain request - use configured base URL
        std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string())
    };

    let metadata = AuthorizationServerMetadata {
        issuer: base_url.clone(),
        authorization_endpoint: format!("{}/.oauth/authorize", base_url),
        token_endpoint: format!("{}/.oauth/token", base_url),
        registration_endpoint: format!("{}/.oauth/register", base_url),
        scopes_supported: vec!["mcp:read".to_string()],
        response_types_supported: vec!["code".to_string()],
        response_modes_supported: vec!["query".to_string()],
        grant_types_supported: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        token_endpoint_auth_methods_supported: vec!["none".to_string()],
        code_challenge_methods_supported: vec!["S256".to_string(), "plain".to_string()],
    };

    let mut headers = HeaderMap::new();
    // Use static header values - these are known to be valid
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        header::HeaderValue::from_static("GET, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        header::HeaderValue::from_static("Authorization, Content-Type, mcp-protocol-version"),
    );
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );

    (StatusCode::OK, headers, Json(metadata)).into_response()
}

/// GET /.well-known/mcp-servers
///
/// Returns list of public MCP servers for discovery
pub async fn mcp_servers_discovery(State(state): State<AppState>) -> Result<Response, StatusCode> {
    use crate::models::server::Server;

    // Get base URL
    let base_url =
        std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());

    // Query public and organization servers only
    let servers = sqlx::query_as::<_, Server>(
        r#"
        SELECT id, uuid, user_id, name, description, access_level, created_at, updated_at
        FROM servers
        WHERE access_level IN ('public', 'organization')
        ORDER BY name
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let server_info: Vec<McpServerInfo> = servers
        .into_iter()
        .map(|s| {
            let uuid = s.uuid.clone();
            McpServerInfo {
                server_uuid: uuid.clone(),
                name: s.name.clone(),
                access_level: s
                    .access_level
                    .clone()
                    .unwrap_or_else(|| "public".to_string()),
                http_endpoint: format!("{}/s/{}", base_url, uuid),
                sse_endpoint: format!("{}/s/{}/sse", base_url, uuid),
                authentication_required: s.access_level.as_deref() != Some("public"),
            }
        })
        .collect();

    let mut headers = HeaderMap::new();
    // Use static header values - these are known to be valid
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        header::HeaderValue::from_static("GET, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        header::HeaderValue::from_static("Authorization, Content-Type, mcp-protocol-version"),
    );
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );

    Ok((StatusCode::OK, headers, Json(server_info)).into_response())
}

/// GET /.well-known/oauth-protected-resource/s/{uuid}
/// GET /.well-known/oauth-protected-resource/s/{uuid}/sse
///
/// Returns OAuth Protected Resource Metadata for MCP server
/// This endpoint is PUBLIC (no authentication required) to allow OAuth discovery
pub async fn oauth_protected_resource_metadata(
    State(state): State<AppState>,
    _uri: axum::http::Uri,
    axum::extract::Path(uuid): axum::extract::Path<String>,
) -> Result<Response, StatusCode> {
    use crate::models::server::Server;

    // Get base URL
    let base_url =
        std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());

    // Get server by UUID
    let _server = Server::get_by_uuid(&state.pool, &uuid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Build resource metadata (Claude Desktop compatible format)
    // Advertise subdomain URLs for Claude Desktop integration
    // Format: https://{uuid}.saramcp.com/ (root path on dedicated subdomain)
    let subdomain_url = format!("https://{}.saramcp.com", uuid);
    let metadata = OAuthProtectedResourceMetadata {
        oauth_authorization_server: format!("{}/.well-known/oauth-authorization-server", base_url),
        protected_resources: vec![
            // Primary: Root path on subdomain (new format for Claude Desktop)
            format!("{}/", subdomain_url),
            // Message endpoint on subdomain
            format!("{}/message", subdomain_url),
            // Legacy: Keep old paths for backward compatibility
            format!("{}/s/{}/sse", base_url, uuid),
            format!("{}/s/{}/sse/message", base_url, uuid),
        ],
    };

    let mut headers = HeaderMap::new();
    // Use static header values - these are known to be valid
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        header::HeaderValue::from_static("GET, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        header::HeaderValue::from_static("Authorization, Content-Type, mcp-protocol-version"),
    );
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );

    Ok((StatusCode::OK, headers, Json(metadata)).into_response())
}

/// GET /.well-known/oauth-protected-resource (subdomain version)
///
/// Returns OAuth Protected Resource Metadata for MCP server accessed via subdomain
/// UUID is extracted from X-Server-UUID header or Host header
/// This endpoint is PUBLIC (no authentication required) to allow OAuth discovery
pub async fn oauth_protected_resource_metadata_subdomain(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, StatusCode> {
    use crate::models::server::Server;

    // Extract UUID from headers
    let uuid = crate::middleware::extract_server_uuid_from_headers(&headers)
        .ok_or(StatusCode::BAD_REQUEST)?;

    // Get server by UUID to validate it exists
    let _server = Server::get_by_uuid(&state.pool, &uuid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Build subdomain URL for OAuth flow
    // The entire OAuth flow happens on the subdomain to avoid cross-domain issues
    let subdomain_url = format!("https://{}.saramcp.com", uuid);

    // Build resource metadata (Claude Desktop compatible format)
    let metadata = OAuthProtectedResourceMetadata {
        // OAuth flow happens on the same subdomain
        oauth_authorization_server: format!(
            "{}/.well-known/oauth-authorization-server",
            subdomain_url
        ),
        protected_resources: vec![
            // Primary: Root path on subdomain
            format!("{}/", subdomain_url),
            // Message endpoint on subdomain
            format!("{}/message", subdomain_url),
        ],
    };

    let mut response_headers = HeaderMap::new();
    // Use static header values - these are known to be valid
    response_headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::HeaderValue::from_static("*"),
    );
    response_headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        header::HeaderValue::from_static("GET, OPTIONS"),
    );
    response_headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        header::HeaderValue::from_static("Authorization, Content-Type, mcp-protocol-version"),
    );
    response_headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );

    Ok((StatusCode::OK, response_headers, Json(metadata)).into_response())
}

/// OPTIONS handler for CORS preflight requests
pub async fn options_handler() -> Response {
    let mut headers = HeaderMap::new();
    // Use static header values - these are known to be valid
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        header::HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        header::HeaderValue::from_static("Authorization, Content-Type, mcp-protocol-version"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        header::HeaderValue::from_static("3600"),
    );

    (StatusCode::NO_CONTENT, headers).into_response()
}

// ============================================================================
// Templates
// ============================================================================

#[derive(Template, WebTemplate)]
#[template(path = "oauth/consent.html")]
pub struct ConsentTemplate {
    pub csrf_token: String,
    pub client_name: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub response_type: String,
    pub scope: String,
    pub scopes: Vec<String>,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub user_email: String,
}

// ============================================================================
// Request/Response DTOs
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct AuthorizationRequest {
    pub response_type: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: Option<String>,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuthorizeConsentRequest {
    pub csrf_token: String,
    pub action: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub response_type: String,
    pub scope: String,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    pub grant_type: String,

    // Authorization code grant fields
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub code_verifier: Option<String>,

    // Refresh token grant fields
    pub refresh_token: Option<String>,

    // Client credentials
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub scope: String,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TokenErrorResponse {
    pub error: String,
    pub error_description: Option<String>,
}

/// OAuth 2.1 Authorization Server Metadata
/// Spec: RFC 8414 - OAuth 2.0 Authorization Server Metadata
#[derive(Debug, Serialize)]
pub struct AuthorizationServerMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: String,
    pub scopes_supported: Vec<String>,
    pub response_types_supported: Vec<String>,
    pub response_modes_supported: Vec<String>,
    pub grant_types_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
}

/// MCP Server Information
#[derive(Debug, Serialize)]
pub struct McpServerInfo {
    pub server_uuid: String,
    pub name: String,
    pub access_level: String,
    pub http_endpoint: String, // Streamable HTTP at /s/{uuid}
    pub sse_endpoint: String,  // SSE at /s/{uuid}/sse
    pub authentication_required: bool,
}

/// OAuth 2.0 Protected Resource Metadata
/// Claude Desktop compatible format (matches doxyde.com working implementation)
#[derive(Debug, Serialize)]
pub struct OAuthProtectedResourceMetadata {
    #[serde(rename = "oauth-authorization-server")]
    pub oauth_authorization_server: String,
    #[serde(rename = "protected-resources")]
    pub protected_resources: Vec<String>,
}

// ============================================================================
// Error Handling
// ============================================================================

#[derive(Debug)]
pub enum OAuthError {
    Unauthorized,
    InvalidClient(String),
    InvalidGrant(String),
    UnsupportedGrantType,
    BadRequest(String),
    InternalError(String),
}

impl IntoResponse for OAuthError {
    fn into_response(self) -> Response {
        let (status, error, description) = match self {
            OAuthError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Authentication required".to_string(),
            ),
            OAuthError::InvalidClient(msg) => (StatusCode::BAD_REQUEST, "invalid_client", msg),
            OAuthError::InvalidGrant(msg) => (StatusCode::BAD_REQUEST, "invalid_grant", msg),
            OAuthError::UnsupportedGrantType => (
                StatusCode::BAD_REQUEST,
                "unsupported_grant_type",
                "Only authorization_code and refresh_token grants supported".to_string(),
            ),
            OAuthError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "invalid_request", msg),
            OAuthError::InternalError(msg) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "server_error", msg)
            }
        };

        let error_response = TokenErrorResponse {
            error: error.to_string(),
            error_description: Some(description),
        };

        let mut headers = HeaderMap::new();
        // Use static header values - these are known to be valid
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            header::HeaderValue::from_static("*"),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            header::HeaderValue::from_static("Authorization, Content-Type, mcp-protocol-version"),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );

        (status, headers, Json(error_response)).into_response()
    }
}
