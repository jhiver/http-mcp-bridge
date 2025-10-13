//! Streamable HTTP transport handler for MCP
//!
//! Provides a simple HTTP POST endpoint for MCP protocol messages.
//! This is the primary transport method, with SSE as a fallback for
//! clients that require long-lived connections.
//!
//! # URL Structure
//!
//! - `POST /s/{uuid}` - Send JSON-RPC request, receive JSON response
//! - `OPTIONS /s/{uuid}` - CORS preflight
//!
//! # Usage
//!
//! ```http
//! POST /s/550e8400-e29b-41d4-a716-446655440000
//! Content-Type: application/json
//!
//! {"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
//! ```
//!
//! Response:
//! ```http
//! HTTP/1.1 200 OK
//! Content-Type: application/json
//!
//! {"jsonrpc":"2.0","id":1,"result":{...}}
//! ```

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::mcp::registry::McpServerRegistry;

/// POST /s/{uuid} - Streamable HTTP transport
///
/// Handles a single MCP JSON-RPC request and returns the response.
/// This is a simple request/response pattern, unlike SSE which maintains
/// a persistent connection.
///
/// # Arguments
///
/// * `Path(uuid)` - Server UUID from URL path
/// * `State(registry)` - MCP server registry
/// * `Json(request)` - JSON-RPC request body
///
/// # Returns
///
/// * `200 OK` with JSON-RPC response on success
/// * `404 Not Found` if server UUID doesn't exist
/// * `500 Internal Server Error` if request processing fails
///
/// # Example
///
/// ```http
/// POST /s/550e8400-e29b-41d4-a716-446655440000
/// Content-Type: application/json
///
/// {
///   "jsonrpc": "2.0",
///   "id": 1,
///   "method": "initialize",
///   "params": {}
/// }
/// ```
pub async fn handle_streamable_http(
    Path(uuid): Path<String>,
    State(registry): State<Arc<RwLock<McpServerRegistry>>>,
    Json(request): Json<Value>,
) -> Result<Response, StatusCode> {
    tracing::debug!(uuid = %uuid, "Received HTTP transport request");

    // 1. Get MCP service from registry
    let service = {
        let registry = registry.read().await;
        match registry.get_instance(&uuid) {
            Some(instance) => instance.get_service(),
            None => {
                tracing::warn!(uuid = %uuid, "Server instance not found");
                return Err(StatusCode::NOT_FOUND);
            }
        }
    };

    // 2. Process JSON-RPC request
    let response = service.handle_request(request).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to handle request");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // 3. Return streaming response with CORS headers
    let body = Body::from(
        serde_json::to_string(&response).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );

    let mut headers = HeaderMap::new();
    // Use static header values - these are known to be valid
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::HeaderValue::from_static("*"),
    );

    Ok((StatusCode::OK, headers, body).into_response())
}

/// OPTIONS /s/{uuid} - CORS preflight handler
///
/// Handles CORS preflight requests for the Streamable HTTP endpoint.
///
/// # Returns
///
/// `204 No Content` with appropriate CORS headers
pub async fn handle_streamable_http_options() -> Response {
    let mut headers = HeaderMap::new();
    // Use static header values - these are known to be valid
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        header::HeaderValue::from_static("POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        header::HeaderValue::from_static("Content-Type, Authorization"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        header::HeaderValue::from_static("3600"),
    );

    (StatusCode::NO_CONTENT, headers).into_response()
}
