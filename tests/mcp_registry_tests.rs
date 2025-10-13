use saramcp::mcp::registry::{McpServerRegistry, RegistryError};
use saramcp::test_utils::test_helpers;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;

// Test 1: Create empty registry
#[sqlx::test]
async fn test_registry_new(pool: SqlitePool) {
    let registry = McpServerRegistry::new(pool.clone());

    // Test that registry is empty initially
    assert!(
        registry.get_instance("any-uuid").is_none(),
        "Empty registry should return None for any UUID"
    );

    // Test that get_instance returns None for any UUID
    assert!(
        registry.get_instance("test-uuid-123").is_none(),
        "Empty registry should return None for test-uuid-123"
    );
    assert!(
        registry.get_instance("").is_none(),
        "Empty registry should return None for empty string"
    );
}

// Test 2: Register a server
#[sqlx::test]
async fn test_register_server(pool: SqlitePool) {
    // Create test user
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .expect("Failed to create test user");

    // Create test server
    let (server_id, server_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Test Server", Some("Test"))
            .await
            .expect("Failed to create test server");

    // Create registry
    let mut registry = McpServerRegistry::new(pool.clone());

    // Register server
    let result = registry.register_server(&server_uuid).await;

    // Assertions
    assert!(
        result.is_ok(),
        "Server registration should succeed: {:?}",
        result.err()
    );

    let instance = registry.get_instance(&server_uuid);
    assert!(
        instance.is_some(),
        "get_instance should return Some for registered UUID"
    );

    let instance = instance.unwrap();
    assert_eq!(
        instance.server_id, server_id,
        "Instance should have correct server_id"
    );
    assert_eq!(
        instance.server_uuid, server_uuid,
        "Instance should have correct UUID"
    );
}

// Test 3: Register duplicate server (should return error)
#[sqlx::test]
async fn test_register_duplicate_server(pool: SqlitePool) {
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .expect("Failed to create test user");

    let (_, server_uuid) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .expect("Failed to create test server");

    let mut registry = McpServerRegistry::new(pool.clone());

    // First registration should succeed
    registry
        .register_server(&server_uuid)
        .await
        .expect("First registration should succeed");

    // Second registration should fail
    let result = registry.register_server(&server_uuid).await;

    assert!(result.is_err(), "Duplicate registration should fail");
    match result.unwrap_err() {
        RegistryError::AlreadyRegistered(uuid) => {
            assert_eq!(uuid, server_uuid, "Error should contain the duplicate UUID");
        }
        other => panic!("Expected AlreadyRegistered error, got: {:?}", other),
    }
}

// Test 4: Unregister a server
#[sqlx::test]
async fn test_unregister_server(pool: SqlitePool) {
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .expect("Failed to create test user");

    let (_, server_uuid) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .expect("Failed to create test server");

    let mut registry = McpServerRegistry::new(pool.clone());

    // Register server
    registry
        .register_server(&server_uuid)
        .await
        .expect("Registration should succeed");

    // Verify it exists
    assert!(
        registry.get_instance(&server_uuid).is_some(),
        "Server should be registered before unregister"
    );

    // Unregister it
    let result = registry.unregister_server(&server_uuid).await;
    assert!(
        result.is_ok(),
        "Unregister should succeed: {:?}",
        result.err()
    );

    // Verify it's gone
    assert!(
        registry.get_instance(&server_uuid).is_none(),
        "Server should be unregistered"
    );
}

// Test 5: Unregister nonexistent server (should return error)
#[sqlx::test]
async fn test_unregister_nonexistent_server(pool: SqlitePool) {
    let mut registry = McpServerRegistry::new(pool);
    let result = registry.unregister_server("nonexistent-uuid").await;

    assert!(
        result.is_err(),
        "Unregistering nonexistent server should fail"
    );
    match result.unwrap_err() {
        RegistryError::ServerNotFound(uuid) => {
            assert_eq!(
                uuid, "nonexistent-uuid",
                "Error should contain the missing UUID"
            );
        }
        other => panic!("Expected ServerNotFound error, got: {:?}", other),
    }
}

// Test 6: Reload tools
#[sqlx::test]
async fn test_reload_tools(pool: SqlitePool) {
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .expect("Failed to create test user");

    let (_, server_uuid) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .expect("Failed to create test server");

    let mut registry = McpServerRegistry::new(pool.clone());

    // Register server
    registry
        .register_server(&server_uuid)
        .await
        .expect("Registration should succeed");

    // Call reload_tools
    let result = registry.reload_tools(&server_uuid).await;

    // Assert it returns Ok (even though it's a stub)
    assert!(
        result.is_ok(),
        "reload_tools should succeed: {:?}",
        result.err()
    );
}

// Test 7: Reload tools for nonexistent server
#[sqlx::test]
async fn test_reload_tools_nonexistent(pool: SqlitePool) {
    let mut registry = McpServerRegistry::new(pool);
    let result = registry.reload_tools("nonexistent-uuid").await;

    assert!(
        result.is_err(),
        "reload_tools for nonexistent server should fail"
    );
    match result.unwrap_err() {
        RegistryError::ServerNotFound(uuid) => {
            assert_eq!(
                uuid, "nonexistent-uuid",
                "Error should contain the missing UUID"
            );
        }
        other => panic!("Expected ServerNotFound error, got: {:?}", other),
    }
}

// Test 8: Get instance - found
#[sqlx::test]
async fn test_get_instance_found(pool: SqlitePool) {
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .expect("Failed to create test user");

    let (server_id, server_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Test Server", None)
            .await
            .expect("Failed to create test server");

    let mut registry = McpServerRegistry::new(pool.clone());

    // Register server
    registry
        .register_server(&server_uuid)
        .await
        .expect("Registration should succeed");

    // Get instance with correct UUID
    let instance = registry.get_instance(&server_uuid);

    assert!(instance.is_some(), "get_instance should return Some");

    let instance = instance.unwrap();
    assert_eq!(
        instance.server_id, server_id,
        "Instance should have correct server_id"
    );
    assert_eq!(
        instance.server_uuid, server_uuid,
        "Instance should have correct UUID"
    );
}

// Test 9: Get instance - not found
#[sqlx::test]
async fn test_get_instance_not_found(pool: SqlitePool) {
    let registry = McpServerRegistry::new(pool);
    let result = registry.get_instance("nonexistent-uuid");

    assert!(
        result.is_none(),
        "get_instance for nonexistent UUID should return None"
    );
}

// Test 10: Load all servers on startup
#[sqlx::test]
async fn test_load_all_servers(pool: SqlitePool) {
    // Create 3 test servers in database
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .expect("Failed to create test user");

    let (server1_id, server1_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Server 1", None)
            .await
            .expect("Failed to create server 1");

    let (server2_id, server2_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Server 2", None)
            .await
            .expect("Failed to create server 2");

    let (server3_id, server3_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Server 3", None)
            .await
            .expect("Failed to create server 3");

    // Create registry
    let mut registry = McpServerRegistry::new(pool.clone());

    // Load all servers
    let result = registry.load_all_servers().await;

    // Assertions
    assert!(
        result.is_ok(),
        "load_all_servers should succeed: {:?}",
        result.err()
    );

    // Verify all 3 are registered
    let instance1 = registry.get_instance(&server1_uuid);
    assert!(instance1.is_some(), "Server 1 should be registered");
    assert_eq!(
        instance1.unwrap().server_id,
        server1_id,
        "Server 1 should have correct ID"
    );

    let instance2 = registry.get_instance(&server2_uuid);
    assert!(instance2.is_some(), "Server 2 should be registered");
    assert_eq!(
        instance2.unwrap().server_id,
        server2_id,
        "Server 2 should have correct ID"
    );

    let instance3 = registry.get_instance(&server3_uuid);
    assert!(instance3.is_some(), "Server 3 should be registered");
    assert_eq!(
        instance3.unwrap().server_id,
        server3_id,
        "Server 3 should have correct ID"
    );
}

// Test 11: Shutdown all
#[sqlx::test]
async fn test_shutdown_all(pool: SqlitePool) {
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .expect("Failed to create test user");

    // Create and register 2 servers
    let (_, server1_uuid) = test_helpers::create_test_server(&pool, user_id, "Server 1", None)
        .await
        .expect("Failed to create server 1");

    let (_, server2_uuid) = test_helpers::create_test_server(&pool, user_id, "Server 2", None)
        .await
        .expect("Failed to create server 2");

    let mut registry = McpServerRegistry::new(pool.clone());

    registry
        .register_server(&server1_uuid)
        .await
        .expect("Server 1 registration should succeed");
    registry
        .register_server(&server2_uuid)
        .await
        .expect("Server 2 registration should succeed");

    // Verify both are registered
    assert!(
        registry.get_instance(&server1_uuid).is_some(),
        "Server 1 should be registered"
    );
    assert!(
        registry.get_instance(&server2_uuid).is_some(),
        "Server 2 should be registered"
    );

    // Call shutdown_all
    let result = registry.shutdown_all().await;
    assert!(
        result.is_ok(),
        "shutdown_all should succeed: {:?}",
        result.err()
    );

    // Assert all instances are removed
    assert!(
        registry.get_instance(&server1_uuid).is_none(),
        "Server 1 should be removed after shutdown"
    );
    assert!(
        registry.get_instance(&server2_uuid).is_none(),
        "Server 2 should be removed after shutdown"
    );
}

// Test 12: Hot reload functionality
#[sqlx::test]
async fn test_hot_reload_functionality(pool: SqlitePool) {
    // Setup: Create user, toolkit, and server
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .expect("Failed to create test user");

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .expect("Failed to create toolkit");

    // Create 2 test tools
    let tool1_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "tool1",
        "GET",
        Some("https://api.example.com/{{string:endpoint}}"),
        None,
        None,
        30000,
    )
    .await
    .expect("Failed to create tool1");

    let tool2_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "tool2",
        "POST",
        Some("https://api.example.com/data"),
        None,
        Some(r#"{"key": "{{string:value}}"}"#),
        30000,
    )
    .await
    .expect("Failed to create tool2");

    let (server_id, server_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Test Server", None)
            .await
            .expect("Failed to create test server");

    // Link toolkit to server
    sqlx::query!(
        "INSERT INTO server_toolkits (server_id, toolkit_id) VALUES (?, ?)",
        server_id,
        toolkit_id
    )
    .execute(&pool)
    .await
    .expect("Failed to link toolkit to server");

    // Create 2 initial tool instances
    sqlx::query!(
        r#"
        INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
        VALUES (?, ?, ?, ?)
        "#,
        server_id,
        tool1_id,
        "get_data",
        "Get data from API"
    )
    .execute(&pool)
    .await
    .expect("Failed to create instance 1");

    let instance2_id = sqlx::query!(
        r#"
        INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
        VALUES (?, ?, ?, ?)
        "#,
        server_id,
        tool2_id,
        "post_data",
        "Post data to API"
    )
    .execute(&pool)
    .await
    .expect("Failed to create instance 2")
    .last_insert_rowid();

    // Create registry and register server
    let mut registry = McpServerRegistry::new(pool.clone());
    registry
        .register_server(&server_uuid)
        .await
        .expect("Failed to register server");

    let instance = registry
        .get_instance(&server_uuid)
        .expect("Server should be registered");

    // Verify initial tool count (2 tools)
    let service = instance.get_service();
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": {}
    });

    let response = service
        .handle_request(request)
        .await
        .expect("tools/list should succeed");

    let tools = response["result"]["tools"]
        .as_array()
        .expect("tools should be an array");
    assert_eq!(tools.len(), 2, "Should have 2 tools initially");

    // Now add a third tool instance (without restarting!)
    let tool3_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "tool3",
        "DELETE",
        Some("https://api.example.com/{{string:resource}}"),
        None,
        None,
        30000,
    )
    .await
    .expect("Failed to create tool3");

    sqlx::query!(
        r#"
        INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
        VALUES (?, ?, ?, ?)
        "#,
        server_id,
        tool3_id,
        "delete_data",
        "Delete data from API"
    )
    .execute(&pool)
    .await
    .expect("Failed to create instance 3");

    // Call reload_tools (THIS IS THE KEY TEST)
    registry
        .reload_tools(&server_uuid)
        .await
        .expect("reload_tools should succeed");

    // Verify tool count increased to 3 (HOT RELOAD WORKED!)
    let response = service
        .handle_request(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }))
        .await
        .expect("tools/list should succeed after reload");

    let tools = response["result"]["tools"]
        .as_array()
        .expect("tools should be an array");
    assert_eq!(
        tools.len(),
        3,
        "Should have 3 tools after hot reload (added 1)"
    );

    // Verify the new tool is in the list
    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(
        tool_names.contains(&"delete_data"),
        "New tool 'delete_data' should be in the list"
    );

    // Now delete an instance
    sqlx::query!("DELETE FROM tool_instances WHERE id = ?", instance2_id)
        .execute(&pool)
        .await
        .expect("Failed to delete instance");

    // Reload again
    registry
        .reload_tools(&server_uuid)
        .await
        .expect("reload_tools should succeed after delete");

    // Verify tool count decreased to 2
    let response = service
        .handle_request(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/list",
            "params": {}
        }))
        .await
        .expect("tools/list should succeed after delete");

    let tools = response["result"]["tools"]
        .as_array()
        .expect("tools should be an array");
    assert_eq!(
        tools.len(),
        2,
        "Should have 2 tools after deleting one instance"
    );

    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(
        !tool_names.contains(&"post_data"),
        "Deleted tool 'post_data' should not be in the list"
    );
}

// Test 13: Concurrent reload and tool calls
#[sqlx::test]
async fn test_concurrent_reload_and_tool_calls(pool: SqlitePool) {
    use tokio::task::JoinSet;

    // Setup: Create user, toolkit, tool, and server
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .expect("Failed to create test user");

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .expect("Failed to create toolkit");

    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "test_tool",
        "GET",
        Some("https://api.example.com/data"),
        None,
        None,
        30000,
    )
    .await
    .expect("Failed to create tool");

    let (server_id, server_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Test Server", None)
            .await
            .expect("Failed to create test server");

    // Link toolkit to server
    sqlx::query!(
        "INSERT INTO server_toolkits (server_id, toolkit_id) VALUES (?, ?)",
        server_id,
        toolkit_id
    )
    .execute(&pool)
    .await
    .expect("Failed to link toolkit to server");

    // Create initial tool instance
    sqlx::query!(
        r#"
        INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
        VALUES (?, ?, ?, ?)
        "#,
        server_id,
        tool_id,
        "test_instance",
        "Test instance"
    )
    .execute(&pool)
    .await
    .expect("Failed to create instance");

    // Create registry and register server
    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));
    registry
        .write()
        .await
        .register_server(&server_uuid)
        .await
        .expect("Failed to register server");

    let instance = registry
        .read()
        .await
        .get_instance(&server_uuid)
        .expect("Server should be registered");

    let service = instance.get_service();

    let mut join_set = JoinSet::new();

    // Spawn 10 concurrent tool list calls
    for i in 0..10 {
        let service_clone = Arc::clone(&service);
        join_set.spawn(async move {
            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "id": i,
                "method": "tools/list",
                "params": {}
            });

            let result = service_clone.handle_request(request).await;
            assert!(
                result.is_ok(),
                "Tool list call {} should succeed during concurrent access",
                i
            );
            Ok::<_, String>(())
        });
    }

    // Spawn 5 concurrent reload operations
    for i in 0..5 {
        let registry_clone = Arc::clone(&registry);
        let uuid = server_uuid.clone();
        join_set.spawn(async move {
            // Small delay to interleave with tool calls
            tokio::time::sleep(std::time::Duration::from_millis(10 * i)).await;

            let result = registry_clone.write().await.reload_tools(&uuid).await;
            assert!(
                result.is_ok(),
                "Reload {} should succeed during concurrent access",
                i
            );
            Ok::<_, String>(())
        });
    }

    // Join all tasks - verify no panics or deadlocks
    while let Some(result) = join_set.join_next().await {
        result
            .expect("Task should not panic")
            .expect("Task should succeed");
    }

    // Final verification: service is still functional
    let response = service
        .handle_request(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 999,
            "method": "tools/list",
            "params": {}
        }))
        .await
        .expect("Final tools/list should succeed after concurrent operations");

    let tools = response["result"]["tools"]
        .as_array()
        .expect("tools should be an array");
    assert_eq!(
        tools.len(),
        1,
        "Should still have 1 tool after concurrent operations"
    );
}

// Test 14: Concurrent access (thread safety)
#[sqlx::test]
async fn test_concurrent_access(pool: SqlitePool) {
    use tokio::task::JoinSet;

    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .expect("Failed to create test user");

    // Create initial test server
    let (_, server0_uuid) = test_helpers::create_test_server(&pool, user_id, "Server 0", None)
        .await
        .expect("Failed to create server 0");

    // Create 5 more servers for concurrent registration
    let mut server_uuids = Vec::new();
    for i in 1..=5 {
        let (_, uuid) =
            test_helpers::create_test_server(&pool, user_id, &format!("Server {}", i), None)
                .await
                .unwrap_or_else(|_| panic!("Failed to create server {}", i));
        server_uuids.push(uuid);
    }

    // Wrap registry in Arc<RwLock<>>
    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));

    // Register the initial server
    registry
        .write()
        .await
        .register_server(&server0_uuid)
        .await
        .expect("Initial server registration should succeed");

    let mut join_set = JoinSet::new();

    // Spawn 5 concurrent read tasks (get_instance)
    for i in 0..5 {
        let registry_clone = Arc::clone(&registry);
        let uuid = server0_uuid.clone();
        join_set.spawn(async move {
            let instance = registry_clone.read().await.get_instance(&uuid);
            assert!(instance.is_some(), "Read task {} should find instance", i);
            Ok::<_, String>(())
        });
    }

    // Spawn 5 concurrent write tasks (register_server)
    for (i, uuid) in server_uuids.iter().enumerate() {
        let registry_clone = Arc::clone(&registry);
        let uuid = uuid.clone();
        join_set.spawn(async move {
            let result = registry_clone.write().await.register_server(&uuid).await;
            assert!(
                result.is_ok(),
                "Write task {} should succeed: {:?}",
                i,
                result.err()
            );
            Ok::<_, String>(())
        });
    }

    // Join all tasks
    while let Some(result) = join_set.join_next().await {
        result
            .expect("Task should not panic")
            .expect("Task should succeed");
    }

    // Verify all operations succeeded
    // Check that initial server still exists
    assert!(
        registry.read().await.get_instance(&server0_uuid).is_some(),
        "Initial server should still exist after concurrent access"
    );

    // Check that all new servers were registered
    for (i, uuid) in server_uuids.iter().enumerate() {
        assert!(
            registry.read().await.get_instance(uuid).is_some(),
            "Server {} should be registered",
            i + 1
        );
    }
}
