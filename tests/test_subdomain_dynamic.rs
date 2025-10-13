//! Tests for dynamic subdomain-based MCP routing
//!
//! This test suite verifies that subdomain-based SSE routing is fully dynamic,
//! allowing servers created after application startup to be immediately accessible
//! without requiring restarts.

use axum::http::{HeaderMap, HeaderName, HeaderValue};
use saramcp::mcp::registry::McpServerRegistry;
use saramcp::middleware::extract_server_uuid_from_headers;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Helper to create headers simulating a subdomain request
fn create_subdomain_headers(uuid: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();

    // Option 1: X-Server-UUID header (for testing)
    headers.insert(
        HeaderName::from_static("x-server-uuid"),
        HeaderValue::from_str(uuid).unwrap(),
    );

    // Option 2: Host header with subdomain
    let host = format!("{}.saramcp.com", uuid);
    headers.insert(
        HeaderName::from_static("host"),
        HeaderValue::from_str(&host).unwrap(),
    );

    headers
}

#[sqlx::test]
async fn test_subdomain_routing_is_dynamic(pool: SqlitePool) {
    // Create a test user
    let user_id = sqlx::query!(
        "INSERT INTO users (email, password_hash) VALUES ('test@example.com', 'hash') RETURNING id"
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id;

    // Create MCP registry (simulating app startup)
    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));

    // Initially no servers exist
    {
        let reg = registry.read().await;
        let test_uuid = Uuid::new_v4().to_string();
        assert!(reg.get_instance(&test_uuid).is_none());
    }

    // Create a new server AFTER "app startup" (runtime creation)
    let server_uuid = Uuid::new_v4().to_string();
    let _server_id = sqlx::query!(
        "INSERT INTO servers (user_id, name, uuid, description, access_level)
         VALUES (?, 'Runtime Server', ?, 'Created after startup', 'public') RETURNING id",
        user_id,
        server_uuid
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id
    .unwrap();

    // Register the server dynamically
    {
        let mut reg = registry.write().await;
        reg.register_server(&server_uuid).await.unwrap();
    }

    // Verify server is accessible via subdomain headers
    let headers = create_subdomain_headers(&server_uuid);
    let extracted_uuid = extract_server_uuid_from_headers(&headers);

    assert_eq!(
        extracted_uuid,
        Some(server_uuid.clone()),
        "UUID should be extractable from subdomain headers"
    );

    // Verify the instance is available for subdomain SSE
    {
        let reg = registry.read().await;
        let instance = reg.get_instance(&server_uuid);

        assert!(
            instance.is_some(),
            "Server should be immediately accessible via registry"
        );

        // The instance has both routers configured
        let instance = instance.unwrap();

        // Can get the service for handling requests
        let service = instance.get_service();

        // Test that we can handle requests
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });

        let response = service.handle_request(request).await.unwrap();
        assert_eq!(response["jsonrpc"], "2.0");
        assert!(response.get("result").is_some());
    }

    // Add more servers dynamically
    for i in 0..5 {
        let uuid = Uuid::new_v4().to_string();
        let server_name = format!("Dynamic Server {}", i);
        sqlx::query!(
            "INSERT INTO servers (user_id, name, uuid, access_level) VALUES (?, ?, ?, 'public')",
            user_id,
            server_name,
            uuid
        )
        .execute(&pool)
        .await
        .unwrap();

        // Register immediately
        {
            let mut reg = registry.write().await;
            reg.register_server(&uuid).await.unwrap();
        }

        // Verify immediately accessible via subdomain
        let headers = create_subdomain_headers(&uuid);
        let extracted = extract_server_uuid_from_headers(&headers);
        assert_eq!(extracted, Some(uuid.clone()));

        let reg = registry.read().await;
        assert!(reg.get_instance(&uuid).is_some());
    }
}

#[sqlx::test]
async fn test_subdomain_with_different_access_levels(pool: SqlitePool) {
    let user_id = sqlx::query!(
        "INSERT INTO users (email, password_hash) VALUES ('test2@example.com', 'hash') RETURNING id"
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id;

    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));

    // Create servers with different access levels
    let public_uuid = Uuid::new_v4().to_string();
    let org_uuid = Uuid::new_v4().to_string();
    let private_uuid = Uuid::new_v4().to_string();

    // Public server
    sqlx::query!(
        "INSERT INTO servers (user_id, name, uuid, access_level) VALUES (?, 'Public', ?, 'public')",
        user_id,
        public_uuid
    )
    .execute(&pool)
    .await
    .unwrap();

    // Organization server
    sqlx::query!(
        "INSERT INTO servers (user_id, name, uuid, access_level) VALUES (?, 'Org', ?, 'organization')",
        user_id,
        org_uuid
    )
    .execute(&pool)
    .await
    .unwrap();

    // Private server
    sqlx::query!(
        "INSERT INTO servers (user_id, name, uuid, access_level) VALUES (?, 'Private', ?, 'private')",
        user_id,
        private_uuid
    )
    .execute(&pool)
    .await
    .unwrap();

    // Register all servers
    {
        let mut reg = registry.write().await;
        reg.register_server(&public_uuid).await.unwrap();
        reg.register_server(&org_uuid).await.unwrap();
        reg.register_server(&private_uuid).await.unwrap();
    }

    // All servers are accessible in registry (access control happens at handler level)
    {
        let reg = registry.read().await;
        assert!(reg.get_instance(&public_uuid).is_some());
        assert!(reg.get_instance(&org_uuid).is_some());
        assert!(reg.get_instance(&private_uuid).is_some());
    }

    // Verify subdomain headers work for all
    for uuid in [&public_uuid, &org_uuid, &private_uuid] {
        let headers = create_subdomain_headers(uuid);
        let extracted = extract_server_uuid_from_headers(&headers);
        assert_eq!(extracted, Some(uuid.clone()));
    }
}

#[sqlx::test]
async fn test_subdomain_uuid_extraction_formats(pool: SqlitePool) {
    let _pool = pool; // Suppress unused warning

    // Test various subdomain formats
    let uuid = "550e8400-e29b-41d4-a716-446655440000";

    // Test with X-Server-UUID header
    {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("x-server-uuid"),
            HeaderValue::from_str(uuid).unwrap(),
        );
        assert_eq!(
            extract_server_uuid_from_headers(&headers),
            Some(uuid.to_string())
        );
    }

    // Test with Host header (subdomain.domain.com)
    {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("host"),
            HeaderValue::from_str(&format!("{}.saramcp.com", uuid)).unwrap(),
        );
        assert_eq!(
            extract_server_uuid_from_headers(&headers),
            Some(uuid.to_string())
        );
    }

    // Test with Host header including port
    {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("host"),
            HeaderValue::from_str(&format!("{}.saramcp.com:8080", uuid)).unwrap(),
        );
        assert_eq!(
            extract_server_uuid_from_headers(&headers),
            Some(uuid.to_string())
        );
    }

    // Test that localhost subdomain doesn't work (current limitation)
    {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("host"),
            HeaderValue::from_str(&format!("{}.localhost", uuid)).unwrap(),
        );
        // The function currently only supports .saramcp.com domains
        assert_eq!(extract_server_uuid_from_headers(&headers), None);
    }

    // Test with no subdomain (main domain)
    {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("host"),
            HeaderValue::from_str("saramcp.com").unwrap(),
        );
        assert_eq!(extract_server_uuid_from_headers(&headers), None);
    }
}

#[sqlx::test]
async fn test_concurrent_subdomain_access(pool: SqlitePool) {
    let user_id = sqlx::query!(
        "INSERT INTO users (email, password_hash) VALUES ('test3@example.com', 'hash') RETURNING id"
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id;

    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));

    // Create multiple servers
    let mut server_uuids = Vec::new();
    for i in 0..10 {
        let uuid = Uuid::new_v4().to_string();
        let server_name = format!("Server {}", i);
        sqlx::query!(
            "INSERT INTO servers (user_id, name, uuid, access_level) VALUES (?, ?, ?, 'public')",
            user_id,
            server_name,
            uuid
        )
        .execute(&pool)
        .await
        .unwrap();
        server_uuids.push(uuid);
    }

    // Register all servers
    {
        let mut reg = registry.write().await;
        for uuid in &server_uuids {
            reg.register_server(uuid).await.unwrap();
        }
    }

    // Spawn concurrent readers simulating subdomain requests
    let mut tasks = Vec::new();

    for uuid in server_uuids.clone() {
        let registry_clone = Arc::clone(&registry);

        let task = tokio::spawn(async move {
            for _ in 0..10 {
                // Simulate subdomain request processing
                let headers = create_subdomain_headers(&uuid);
                let extracted = extract_server_uuid_from_headers(&headers);
                assert_eq!(extracted, Some(uuid.clone()));

                // Access the instance
                let reg = registry_clone.read().await;
                let instance = reg.get_instance(&uuid).unwrap();
                let service = instance.get_service();

                // Make a request
                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "tools/list",
                    "params": {}
                });

                let response = service.handle_request(request).await.unwrap();
                assert!(response.get("result").is_some());

                tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            }
        });

        tasks.push(task);
    }

    // Wait for all tasks
    for task in tasks {
        task.await.unwrap();
    }

    // Verify all servers still accessible
    {
        let reg = registry.read().await;
        for uuid in &server_uuids {
            assert!(reg.get_instance(uuid).is_some());
        }
    }
}
