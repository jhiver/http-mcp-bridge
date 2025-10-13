//! Tests for dynamic MCP server and tool management
//!
//! These tests verify that:
//! 1. Newly created servers are immediately accessible via HTTP transport
//! 2. Tool definitions can be updated without server restart (hot-reload)
//! 3. Deleted servers are immediately inaccessible
//! 4. Concurrent access works properly during updates

use saramcp::mcp::registry::McpServerRegistry;
use saramcp::services::{InstanceService, SecretsManager, ServerService};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Test helper to setup a database with a user and toolkit
async fn setup_test_data(pool: &SqlitePool) -> (i64, i64, i64) {
    // Create a test user
    let user_id = sqlx::query!(
        "INSERT INTO users (email, password_hash) VALUES ('test@example.com', 'hash') RETURNING id"
    )
    .fetch_one(pool)
    .await
    .unwrap()
    .id;

    // Create a test toolkit
    let toolkit_record = sqlx::query!(
        "INSERT INTO toolkits (user_id, title, description) VALUES (?, 'Test Toolkit', 'Test') RETURNING id",
        user_id
    )
    .fetch_one(pool)
    .await
    .unwrap();

    let toolkit_id = toolkit_record.id.unwrap();

    // Create a test tool
    let tool_record = sqlx::query!(
        "INSERT INTO tools (toolkit_id, name, description, method, url)
         VALUES (?, 'test_tool', 'Test tool', 'GET', 'https://example.com/{{string:param}}') RETURNING id",
        toolkit_id
    )
    .fetch_one(pool)
    .await
    .unwrap();

    let tool_id = tool_record.id.unwrap();

    (user_id, toolkit_id, tool_id)
}

#[sqlx::test]
async fn test_dynamic_server_creation_via_http(pool: SqlitePool) {
    let (user_id, _toolkit_id, _tool_id) = setup_test_data(&pool).await;

    // Create MCP registry
    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));

    // Initially no servers
    {
        let reg = registry.read().await;
        assert!(reg.get_instance("test-uuid-1").is_none());
    }

    // Create a new server via service (simulating web handler)
    let secrets_manager = SecretsManager::new().unwrap();
    let _server_service = ServerService::new(pool.clone(), secrets_manager.clone());

    let server_uuid = Uuid::new_v4().to_string();
    let _server_id = sqlx::query!(
        "INSERT INTO servers (user_id, name, uuid, description)
         VALUES (?, 'Test Server', ?, 'Test') RETURNING id",
        user_id,
        server_uuid
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id
    .unwrap();

    // Register server in MCP registry (simulating what handler does)
    {
        let mut reg = registry.write().await;
        reg.register_server(&server_uuid).await.unwrap();
    }

    // Verify server is immediately accessible
    {
        let reg = registry.read().await;
        let instance = reg.get_instance(&server_uuid);
        assert!(
            instance.is_some(),
            "Server should be immediately accessible after registration"
        );

        // Verify we can get the service (for HTTP requests)
        let service = instance.unwrap().get_service();

        // Test initialize request
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });

        let response = service.handle_request(request).await.unwrap();
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 1);
        assert!(response.get("result").is_some());
    }
}

#[sqlx::test]
async fn test_tool_hot_reload(pool: SqlitePool) {
    let (user_id, toolkit_id, tool_id) = setup_test_data(&pool).await;

    // Create and register a server
    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));
    let server_uuid = Uuid::new_v4().to_string();

    let server_id = sqlx::query!(
        "INSERT INTO servers (user_id, name, uuid) VALUES (?, 'Test Server', ?) RETURNING id",
        user_id,
        server_uuid
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id
    .unwrap();

    // Add server to toolkit binding
    sqlx::query!(
        "INSERT INTO server_toolkits (server_id, toolkit_id) VALUES (?, ?)",
        server_id,
        toolkit_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Register server
    {
        let mut reg = registry.write().await;
        reg.register_server(&server_uuid).await.unwrap();
    }

    // Check initial state - no tools
    {
        let reg = registry.read().await;
        let service = reg.get_instance(&server_uuid).unwrap().get_service();

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {}
        });

        let response = service.handle_request(request).await.unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 0, "Should have no tools initially");
    }

    // Add a tool instance
    let _instance_service = InstanceService::new(pool.clone(), SecretsManager::new().unwrap());
    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'my_tool', 'My tool instance') RETURNING id",
        server_id,
        tool_id
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id;

    // Configure a parameter
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'param', 'instance', 'test_value')",
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Reload tools (simulating what handler does)
    {
        let mut reg = registry.write().await;
        reg.reload_tools(&server_uuid).await.unwrap();
    }

    // Verify tool is now available
    {
        let reg = registry.read().await;
        let service = reg.get_instance(&server_uuid).unwrap().get_service();

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });

        let response = service.handle_request(request).await.unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1, "Should have one tool after hot-reload");
        assert_eq!(tools[0]["name"], "my_tool");
    }

    // Update the tool instance name
    sqlx::query!(
        "UPDATE tool_instances SET instance_name = 'updated_tool' WHERE id = ?",
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Reload tools again
    {
        let mut reg = registry.write().await;
        reg.reload_tools(&server_uuid).await.unwrap();
    }

    // Verify tool name is updated
    {
        let reg = registry.read().await;
        let service = reg.get_instance(&server_uuid).unwrap().get_service();

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/list",
            "params": {}
        });

        let response = service.handle_request(request).await.unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0]["name"], "updated_tool",
            "Tool name should be updated after hot-reload"
        );
    }

    // Delete the tool instance
    sqlx::query!("DELETE FROM tool_instances WHERE id = ?", instance_id)
        .execute(&pool)
        .await
        .unwrap();

    // Reload tools to remove it
    {
        let mut reg = registry.write().await;
        reg.reload_tools(&server_uuid).await.unwrap();
    }

    // Verify tool is removed
    {
        let reg = registry.read().await;
        let service = reg.get_instance(&server_uuid).unwrap().get_service();

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/list",
            "params": {}
        });

        let response = service.handle_request(request).await.unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        assert_eq!(
            tools.len(),
            0,
            "Should have no tools after deletion and hot-reload"
        );
    }
}

#[sqlx::test]
async fn test_concurrent_access_during_reload(pool: SqlitePool) {
    let (user_id, toolkit_id, tool_id) = setup_test_data(&pool).await;

    // Setup server with initial tool
    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));
    let server_uuid = Uuid::new_v4().to_string();

    let server_id = sqlx::query!(
        "INSERT INTO servers (user_id, name, uuid) VALUES (?, 'Test Server', ?) RETURNING id",
        user_id,
        server_uuid
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id
    .unwrap();

    sqlx::query!(
        "INSERT INTO server_toolkits (server_id, toolkit_id) VALUES (?, ?)",
        server_id,
        toolkit_id
    )
    .execute(&pool)
    .await
    .unwrap();

    let _instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'concurrent_tool', 'Test') RETURNING id",
        server_id,
        tool_id
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id;

    {
        let mut reg = registry.write().await;
        reg.register_server(&server_uuid).await.unwrap();
        reg.reload_tools(&server_uuid).await.unwrap();
    }

    // Spawn multiple concurrent readers
    let registry_clone = Arc::clone(&registry);
    let uuid_clone = server_uuid.clone();

    let reader_task = tokio::spawn(async move {
        for i in 0..10 {
            let reg = registry_clone.read().await;
            let service = reg.get_instance(&uuid_clone).unwrap().get_service();

            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "id": i,
                "method": "tools/list",
                "params": {}
            });

            let response = service.handle_request(request).await.unwrap();
            assert!(response["result"]["tools"].is_array());

            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    });

    // Spawn a writer that reloads tools
    let registry_clone2 = Arc::clone(&registry);
    let uuid_clone2 = server_uuid.clone();

    let writer_task = tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let mut reg = registry_clone2.write().await;
        reg.reload_tools(&uuid_clone2).await.unwrap();
    });

    // Wait for both tasks to complete
    let (reader_result, writer_result) = tokio::join!(reader_task, writer_task);

    assert!(
        reader_result.is_ok(),
        "Reader task should complete successfully"
    );
    assert!(
        writer_result.is_ok(),
        "Writer task should complete successfully"
    );
}

#[sqlx::test]
async fn test_server_unregistration(pool: SqlitePool) {
    let (user_id, _, _) = setup_test_data(&pool).await;

    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));
    let server_uuid = Uuid::new_v4().to_string();

    // Create and register server
    sqlx::query!(
        "INSERT INTO servers (user_id, name, uuid) VALUES (?, 'Test Server', ?)",
        user_id,
        server_uuid
    )
    .execute(&pool)
    .await
    .unwrap();

    {
        let mut reg = registry.write().await;
        reg.register_server(&server_uuid).await.unwrap();
    }

    // Verify server exists
    {
        let reg = registry.read().await;
        assert!(reg.get_instance(&server_uuid).is_some());
    }

    // Unregister server
    {
        let mut reg = registry.write().await;
        reg.unregister_server(&server_uuid).await.unwrap();
    }

    // Verify server no longer exists
    {
        let reg = registry.read().await;
        assert!(
            reg.get_instance(&server_uuid).is_none(),
            "Server should be inaccessible after unregistration"
        );
    }

    // Verify we can't unregister again (should error)
    {
        let mut reg = registry.write().await;
        let result = reg.unregister_server(&server_uuid).await;
        assert!(
            result.is_err(),
            "Should error when unregistering non-existent server"
        );
    }
}

#[sqlx::test]
async fn test_multiple_servers_isolation(pool: SqlitePool) {
    let (user_id, toolkit_id, tool_id) = setup_test_data(&pool).await;

    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));

    // Create two servers
    let server1_uuid = Uuid::new_v4().to_string();
    let server2_uuid = Uuid::new_v4().to_string();

    let server1_id = sqlx::query!(
        "INSERT INTO servers (user_id, name, uuid) VALUES (?, 'Server 1', ?) RETURNING id",
        user_id,
        server1_uuid
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id
    .unwrap();

    let server2_id = sqlx::query!(
        "INSERT INTO servers (user_id, name, uuid) VALUES (?, 'Server 2', ?) RETURNING id",
        user_id,
        server2_uuid
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .id
    .unwrap();

    // Add toolkit to both servers
    for server_id in [server1_id, server2_id] {
        sqlx::query!(
            "INSERT INTO server_toolkits (server_id, toolkit_id) VALUES (?, ?)",
            server_id,
            toolkit_id
        )
        .execute(&pool)
        .await
        .unwrap();
    }

    // Add different tools to each server
    sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name)
         VALUES (?, ?, 'server1_tool')",
        server1_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name)
         VALUES (?, ?, 'server2_tool')",
        server2_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Register both servers
    {
        let mut reg = registry.write().await;
        reg.register_server(&server1_uuid).await.unwrap();
        reg.register_server(&server2_uuid).await.unwrap();
        reg.reload_tools(&server1_uuid).await.unwrap();
        reg.reload_tools(&server2_uuid).await.unwrap();
    }

    // Verify each server has its own tools
    {
        let reg = registry.read().await;

        // Server 1 tools
        let service1 = reg.get_instance(&server1_uuid).unwrap().get_service();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {}
        });
        let response1 = service1.handle_request(request.clone()).await.unwrap();
        let tools1 = response1["result"]["tools"].as_array().unwrap();
        assert_eq!(tools1.len(), 1);
        assert_eq!(tools1[0]["name"], "server1_tool");

        // Server 2 tools
        let service2 = reg.get_instance(&server2_uuid).unwrap().get_service();
        let response2 = service2.handle_request(request).await.unwrap();
        let tools2 = response2["result"]["tools"].as_array().unwrap();
        assert_eq!(tools2.len(), 1);
        assert_eq!(tools2[0]["name"], "server2_tool");
    }

    // Update server1's tools shouldn't affect server2
    sqlx::query!(
        "UPDATE tool_instances SET instance_name = 'updated_server1_tool'
         WHERE server_id = ?",
        server1_id
    )
    .execute(&pool)
    .await
    .unwrap();

    {
        let mut reg = registry.write().await;
        reg.reload_tools(&server1_uuid).await.unwrap();
    }

    // Verify isolation
    {
        let reg = registry.read().await;

        let service1 = reg.get_instance(&server1_uuid).unwrap().get_service();
        let service2 = reg.get_instance(&server2_uuid).unwrap().get_service();

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {}
        });

        let response1 = service1.handle_request(request.clone()).await.unwrap();
        let tools1 = response1["result"]["tools"].as_array().unwrap();
        assert_eq!(
            tools1[0]["name"], "updated_server1_tool",
            "Server 1 should be updated"
        );

        let response2 = service2.handle_request(request).await.unwrap();
        let tools2 = response2["result"]["tools"].as_array().unwrap();
        assert_eq!(
            tools2[0]["name"], "server2_tool",
            "Server 2 should remain unchanged"
        );
    }
}
