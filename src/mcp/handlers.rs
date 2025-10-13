//! HTTP request handlers for MCP endpoints
//!
//! These handlers dispatch incoming SSE and message requests to the correct
//! MCP server instance based on the server UUID in the URL path.

use crate::mcp::registry::SharedRegistry;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Handles SSE connection requests for MCP servers
///
/// Routes requests to the appropriate server instance based on UUID.
///
/// # Route
///
/// `GET /s/:server_uuid`
///
/// # Arguments
///
/// * `server_uuid` - Server UUID from URL path
/// * `registry` - Shared registry of server instances
///
/// # Returns
///
/// * `Ok(Response)` - SSE connection established
/// * `Err(StatusCode::NOT_FOUND)` - Server not found
pub async fn mcp_sse_handler(
    Path(server_uuid): Path<String>,
    State(registry): State<SharedRegistry>,
) -> Result<Response, StatusCode> {
    let instance = registry
        .read()
        .await
        .get_instance(&server_uuid)
        .ok_or(StatusCode::NOT_FOUND)?;

    // TODO: Task 003 will implement actual SSE streaming
    Ok(Json(json!({
        "server_uuid": server_uuid,
        "server_id": instance.server_id,
        "status": "available"
    }))
    .into_response())
}

/// Handles MCP message POST requests
///
/// Routes JSON-RPC messages to the appropriate server instance based on UUID.
///
/// # Route
///
/// `POST /s/:server_uuid/message`
///
/// # Arguments
///
/// * `server_uuid` - Server UUID from URL path
/// * `registry` - Shared registry of server instances
///
/// # Returns
///
/// * `Ok(Response)` - Message processed
/// * `Err(StatusCode::NOT_FOUND)` - Server not found
pub async fn mcp_message_handler(
    Path(server_uuid): Path<String>,
    State(registry): State<SharedRegistry>,
) -> Result<Response, StatusCode> {
    let instance = registry
        .read()
        .await
        .get_instance(&server_uuid)
        .ok_or(StatusCode::NOT_FOUND)?;

    // TODO: Task 003 will implement actual message handling
    Ok(Json(json!({
        "server_uuid": server_uuid,
        "server_id": instance.server_id,
        "status": "received"
    }))
    .into_response())
}
