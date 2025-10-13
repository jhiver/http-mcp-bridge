//! Test to demonstrate SSE endpoint limitations
//!
//! This test documents the current limitation where SSE endpoints
//! are only registered at application startup, not dynamically.

use saramcp::mcp::registry::McpServerRegistry;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[sqlx::test]
async fn test_sse_endpoints_not_dynamic(pool: SqlitePool) {
    // Create a test user
    let user_id = sqlx::query!(
        "INSERT INTO users (email, password_hash) VALUES ('test@example.com', 'hash') RETURNING id"
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id;

    // Create MCP registry
    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));

    // Create a server AFTER the app has started (simulating runtime creation)
    let server_uuid = Uuid::new_v4().to_string();
    let _server_id = sqlx::query!(
        "INSERT INTO servers (user_id, name, uuid, description)
         VALUES (?, 'Runtime Server', ?, 'Created after startup') RETURNING id",
        user_id,
        server_uuid
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id
    .unwrap();

    // Register the server in the registry
    {
        let mut reg = registry.write().await;
        reg.register_server(&server_uuid).await.unwrap();
    }

    // The server IS available via the registry (for HTTP transport)
    {
        let reg = registry.read().await;
        let instance = reg.get_instance(&server_uuid);
        assert!(
            instance.is_some(),
            "Server is available in registry for HTTP transport"
        );

        // The instance has SSE routers created
        let _instance = instance.unwrap();
        // Note: The SSE router exists on the instance
        // (We can't easily check router internals, but it exists)
    }

    // HOWEVER: The SSE router is NOT merged into the main app router
    // because that happens only at startup in main.rs lines 394-402
    //
    // This means:
    // ✅ POST /s/{uuid} works immediately (HTTP transport uses registry lookup)
    // ❌ GET /s/{uuid}/sse does NOT work (SSE routes are static from startup)
    //
    // To make SSE endpoints fully dynamic, the application would need to:
    // 1. Use a dynamic router that looks up SSE handlers at request time, OR
    // 2. Implement a proxy/dispatch mechanism for SSE endpoints, OR
    // 3. Require a server restart after creating new servers
    //
    // Current workaround: Use HTTP transport for dynamically created servers
    // or restart the application after creating new servers that need SSE.

    // Demonstrate that multiple servers can coexist in the registry
    let server2_uuid = Uuid::new_v4().to_string();
    sqlx::query!(
        "INSERT INTO servers (user_id, name, uuid) VALUES (?, 'Another Runtime Server', ?)",
        user_id,
        server2_uuid
    )
    .execute(&pool)
    .await
    .unwrap();

    {
        let mut reg = registry.write().await;
        reg.register_server(&server2_uuid).await.unwrap();
    }

    // Both servers are available via registry
    {
        let reg = registry.read().await;
        assert!(reg.get_instance(&server_uuid).is_some());
        assert!(reg.get_instance(&server2_uuid).is_some());
    }
}

#[sqlx::test]
async fn test_workaround_http_transport_is_dynamic(pool: SqlitePool) {
    // This test confirms the recommended workaround:
    // HTTP transport (POST /s/{uuid}) IS fully dynamic

    let user_id = sqlx::query!(
        "INSERT INTO users (email, password_hash) VALUES ('test2@example.com', 'hash') RETURNING id"
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id;

    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));

    // Simulate creating 10 servers at runtime
    let mut server_uuids = Vec::new();
    for i in 0..10 {
        let server_uuid = Uuid::new_v4().to_string();
        let server_name = format!("Dynamic Server {}", i);

        sqlx::query!(
            "INSERT INTO servers (user_id, name, uuid) VALUES (?, ?, ?)",
            user_id,
            server_name,
            server_uuid
        )
        .execute(&pool)
        .await
        .unwrap();

        // Register immediately
        {
            let mut reg = registry.write().await;
            reg.register_server(&server_uuid).await.unwrap();
        }

        server_uuids.push(server_uuid);
    }

    // All servers are immediately available for HTTP transport
    for (i, uuid) in server_uuids.iter().enumerate() {
        let reg = registry.read().await;
        let instance = reg.get_instance(uuid);
        assert!(instance.is_some(), "Server {} should be available", i);

        // Can handle requests immediately
        let service = instance.unwrap().get_service();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": i,
            "method": "initialize",
            "params": {}
        });

        let response = service.handle_request(request).await.unwrap();
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], i);
    }

    // Servers can also be unregistered dynamically
    {
        let mut reg = registry.write().await;
        for uuid in &server_uuids[0..5] {
            reg.unregister_server(uuid).await.unwrap();
        }
    }

    // Verify first 5 are gone, last 5 remain
    {
        let reg = registry.read().await;
        for uuid in &server_uuids[0..5] {
            assert!(reg.get_instance(uuid).is_none());
        }
        for uuid in &server_uuids[5..10] {
            assert!(reg.get_instance(uuid).is_some());
        }
    }
}
