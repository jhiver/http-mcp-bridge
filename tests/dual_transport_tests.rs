//! Integration tests for dual MCP transport (HTTP + SSE)
//!
//! Tests that both Streamable HTTP and SSE transports work correctly
//! at their respective endpoints.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::json;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt; // for `oneshot`

use saramcp::{mcp, AppState};

/// Create test server with a configured MCP instance
async fn setup_test_server(pool: &SqlitePool) -> (String, Router) {
    // Create test user
    let user_record = sqlx::query!(
        "INSERT INTO users (email, password_hash) VALUES (?, ?) RETURNING id",
        "test@example.com",
        "hashed"
    )
    .fetch_one(pool)
    .await
    .expect("Failed to create test user");
    let user_id = user_record.id;

    // Create test server with UUID
    let server_uuid = "test-uuid-http-sse";
    let server_record = sqlx::query!(
        "INSERT INTO servers (user_id, name, description, uuid, access_level)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
        user_id,
        "Test Server",
        "Test MCP Server",
        server_uuid,
        "public"
    )
    .fetch_one(pool)
    .await
    .expect("Failed to create test server");
    let server_id = server_record.id.expect("Server ID should not be null");

    // Create test toolkit
    let toolkit_record = sqlx::query!(
        "INSERT INTO toolkits (user_id, title, description) VALUES (?, ?, ?) RETURNING id",
        user_id,
        "Test Toolkit",
        "Test toolkit"
    )
    .fetch_one(pool)
    .await
    .expect("Failed to create toolkit");
    let toolkit_id = toolkit_record.id;

    // Create test tool
    let tool_record = sqlx::query!(
        "INSERT INTO tools (toolkit_id, name, description, method, url, headers, body)
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
        toolkit_id,
        "test_tool",
        "Test tool",
        "GET",
        "https://api.example.com/test",
        "{}",
        ""
    )
    .fetch_one(pool)
    .await
    .expect("Failed to create tool");
    let tool_id = tool_record.id;

    // Create tool instance
    sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, ?, ?)",
        server_id,
        tool_id,
        "test_instance",
        "Test instance"
    )
    .execute(pool)
    .await
    .expect("Failed to create tool instance");

    // Initialize MCP registry
    let mcp_registry = Arc::new(RwLock::new(mcp::McpServerRegistry::new(pool.clone())));

    // Load servers into registry
    {
        let mut registry = mcp_registry.write().await;
        registry
            .load_all_servers()
            .await
            .expect("Failed to load servers");
    }

    // Build router with HTTP transport
    let http_routes = Router::new()
        .route(
            "/s/{uuid}",
            axum::routing::post(mcp::http_transport::handle_streamable_http)
                .options(mcp::http_transport::handle_streamable_http_options),
        )
        .with_state(mcp_registry.clone());

    // For SSE routes, we need to extract the router from the instance
    let sse_router = {
        let registry = mcp_registry.read().await;
        registry
            .get_instance(server_uuid)
            .expect("Server instance not found")
            .subdomain_sse_router
            .clone()
    };

    // Merge routers
    let app = http_routes.merge(sse_router);

    (server_uuid.to_string(), app)
}

#[sqlx::test]
async fn test_http_transport_initialize(pool: SqlitePool) {
    let (uuid, app) = setup_test_server(&pool).await;

    // Create initialize request
    let request_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("/s/{}", uuid))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&request_body).unwrap()))
        .unwrap();

    let response = app
        .oneshot(request)
        .await
        .expect("Failed to execute request");

    // Verify response
    assert_eq!(response.status(), StatusCode::OK);

    // Parse response body
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    let response_json: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    // Verify JSON-RPC response structure
    assert_eq!(response_json["jsonrpc"], "2.0");
    assert_eq!(response_json["id"], 1);
    assert!(
        response_json["result"].is_object(),
        "Result should be an object"
    );
    assert!(
        response_json["result"]["protocolVersion"].is_string(),
        "Should have protocolVersion"
    );
}

#[sqlx::test]
async fn test_http_transport_not_found(pool: SqlitePool) {
    let (_uuid, app) = setup_test_server(&pool).await;

    let request_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });

    let request = Request::builder()
        .method("POST")
        .uri("/s/nonexistent-uuid")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&request_body).unwrap()))
        .unwrap();

    let response = app
        .oneshot(request)
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn test_http_transport_cors_headers(pool: SqlitePool) {
    let (uuid, app) = setup_test_server(&pool).await;

    let request_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });

    let request = Request::builder()
        .method("POST")
        .uri(format!("/s/{}", uuid))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&request_body).unwrap()))
        .unwrap();

    let response = app
        .oneshot(request)
        .await
        .expect("Failed to execute request");

    // Verify CORS headers
    let headers = response.headers();
    assert_eq!(
        headers
            .get("access-control-allow-origin")
            .map(|h| h.to_str().unwrap()),
        Some("*")
    );
    assert_eq!(
        headers.get("content-type").map(|h| h.to_str().unwrap()),
        Some("application/json")
    );
}

#[sqlx::test]
async fn test_http_transport_options_request(pool: SqlitePool) {
    let (uuid, app) = setup_test_server(&pool).await;

    let request = Request::builder()
        .method("OPTIONS")
        .uri(format!("/s/{}", uuid))
        .body(Body::empty())
        .unwrap();

    let response = app
        .oneshot(request)
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // Verify CORS headers
    let headers = response.headers();
    assert_eq!(
        headers
            .get("access-control-allow-origin")
            .map(|h| h.to_str().unwrap()),
        Some("*")
    );
    assert_eq!(
        headers
            .get("access-control-allow-methods")
            .map(|h| h.to_str().unwrap()),
        Some("POST, OPTIONS")
    );
}

#[sqlx::test]
async fn test_service_handle_request_initialize(pool: SqlitePool) {
    // Create test server
    let user_record = sqlx::query!(
        "INSERT INTO users (email, password_hash) VALUES (?, ?) RETURNING id",
        "test@example.com",
        "hashed"
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to create user");
    let user_id = user_record.id;

    let server_record = sqlx::query!(
        "INSERT INTO servers (user_id, name, description, uuid) VALUES (?, ?, ?, ?) RETURNING id",
        user_id,
        "Test Server",
        "Test",
        "test-uuid"
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to create server");
    let server_id = server_record.id.expect("Server ID should not be null");

    // Create service
    let service = mcp::SaraMcpService::new(server_id, pool.clone())
        .await
        .expect("Failed to create service");

    // Test initialize request
    let request = json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "initialize",
        "params": {}
    });

    let response = service
        .handle_request(request)
        .await
        .expect("Failed to handle request");

    // Verify response structure
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 42);
    assert!(response["result"].is_object());
    assert!(response["result"]["protocolVersion"].is_string());
}

#[sqlx::test]
async fn test_service_handle_request_unknown_method(pool: SqlitePool) {
    let user_record = sqlx::query!(
        "INSERT INTO users (email, password_hash) VALUES (?, ?) RETURNING id",
        "test@example.com",
        "hashed"
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to create user");
    let user_id = user_record.id;

    let server_record = sqlx::query!(
        "INSERT INTO servers (user_id, name, description, uuid) VALUES (?, ?, ?, ?) RETURNING id",
        user_id,
        "Test Server",
        "Test",
        "test-uuid"
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to create server");
    let server_id = server_record.id.expect("Server ID should not be null");

    let service = mcp::SaraMcpService::new(server_id, pool.clone())
        .await
        .expect("Failed to create service");

    // Test unknown method
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "unknown/method",
        "params": {}
    });

    let response = service
        .handle_request(request)
        .await
        .expect("Failed to handle request");

    // Should return error response
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert!(response["error"].is_object());
    assert_eq!(response["error"]["code"], -32603);
}

#[sqlx::test]
async fn test_service_handle_request_missing_method(pool: SqlitePool) {
    let user_record = sqlx::query!(
        "INSERT INTO users (email, password_hash) VALUES (?, ?) RETURNING id",
        "test@example.com",
        "hashed"
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to create user");
    let user_id = user_record.id;

    let server_record = sqlx::query!(
        "INSERT INTO servers (user_id, name, description, uuid) VALUES (?, ?, ?, ?) RETURNING id",
        user_id,
        "Test Server",
        "Test",
        "test-uuid"
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to create server");
    let server_id = server_record.id.expect("Server ID should not be null");

    let service = mcp::SaraMcpService::new(server_id, pool.clone())
        .await
        .expect("Failed to create service");

    // Test request without method field
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "params": {}
    });

    let response = service
        .handle_request(request)
        .await
        .expect("Failed to handle request");

    // Should return error response
    assert_eq!(response["jsonrpc"], "2.0");
    assert!(response["error"].is_object());
}

#[sqlx::test]
async fn test_discovery_endpoint_dual_transports(pool: SqlitePool) {
    // Create test server with UUID
    let user_record = sqlx::query!(
        "INSERT INTO users (email, password_hash) VALUES (?, ?) RETURNING id",
        "test@example.com",
        "hashed"
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to create user");
    let user_id = user_record.id;

    let test_uuid = "test-discovery-uuid";
    sqlx::query!(
        "INSERT INTO servers (user_id, name, description, uuid, access_level)
         VALUES (?, ?, ?, ?, ?)",
        user_id,
        "Public Server",
        "Test public server",
        test_uuid,
        "public"
    )
    .execute(&pool)
    .await
    .expect("Failed to create server");

    // Build app state
    let user_repository = Arc::new(saramcp::repositories::SqliteUserRepository::new(
        pool.clone(),
    ));
    let user_service = Arc::new(saramcp::services::UserService::new(user_repository.clone()));
    let auth_service = Arc::new(saramcp::services::AuthService::new(user_repository.clone()));
    let email_service = saramcp::services::create_email_service();
    let auth_token_service = Arc::new(saramcp::services::AuthTokenService::new(
        pool.clone(),
        email_service,
        user_repository.clone(),
        user_service.clone(),
    ));

    let app_state = AppState {
        user_service,
        auth_service,
        auth_token_service,
        toolkit_service: None,
        tool_service: None,
        server_service: None,
        instance_service: None,
        oauth_service: Arc::new(saramcp::services::OAuthService::new(pool.clone())),
        toolkit_repository: None,
        tool_repository: None,
        mcp_registry: None,
        pool: pool.clone(),
    };

    // Build router with discovery endpoint
    let app = Router::new()
        .route(
            "/.well-known/mcp-servers",
            axum::routing::get(saramcp::handlers::mcp_servers_discovery),
        )
        .with_state(app_state);

    // Make request
    let request = Request::builder()
        .method("GET")
        .uri("/.well-known/mcp-servers")
        .body(Body::empty())
        .unwrap();

    let response = app
        .oneshot(request)
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    // Parse response
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read body");
    let servers: Vec<serde_json::Value> =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    // Verify server has both endpoints
    assert!(!servers.is_empty(), "Should have at least one server");
    let server = &servers[0];

    assert_eq!(server["server_uuid"], test_uuid);
    assert!(
        server["http_endpoint"].is_string(),
        "Should have http_endpoint"
    );
    assert!(
        server["sse_endpoint"].is_string(),
        "Should have sse_endpoint"
    );

    // Verify endpoint formats
    let http_endpoint = server["http_endpoint"].as_str().unwrap();
    let sse_endpoint = server["sse_endpoint"].as_str().unwrap();

    assert!(http_endpoint.ends_with(&format!("/s/{}", test_uuid)));
    assert!(sse_endpoint.ends_with(&format!("/s/{}/sse", test_uuid)));
}

#[sqlx::test]
async fn test_sse_path_configuration(pool: SqlitePool) {
    // Create test server
    let user_record = sqlx::query!(
        "INSERT INTO users (email, password_hash) VALUES (?, ?) RETURNING id",
        "test@example.com",
        "hashed"
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to create user");
    let user_id = user_record.id;

    let server_uuid = "test-sse-path";
    let server_record = sqlx::query!(
        "INSERT INTO servers (user_id, name, description, uuid) VALUES (?, ?, ?, ?) RETURNING id",
        user_id,
        "Test Server",
        "Test",
        server_uuid
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to create server");
    let server_id = server_record.id.expect("Server ID should not be null");

    // Create MCP instance
    let instance = mcp::McpServerInstance::new(pool.clone(), server_id, server_uuid.to_string())
        .await
        .expect("Failed to create instance");

    // The SSE router should be configured for /s/{uuid}/sse
    // We can't directly inspect the router path, but we can verify it exists
    assert_eq!(instance.server_uuid, server_uuid);

    // Verify instance has an SSE router (it's a non-null Router)
    // This is a basic smoke test - the router exists
    let _router = instance.subdomain_sse_router;
}
