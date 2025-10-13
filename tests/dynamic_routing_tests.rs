use saramcp::mcp::registry::McpServerRegistry;
use saramcp::models::instance::ToolInstance;
use saramcp::models::server::Server;
use saramcp::test_utils::test_helpers;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;

// Test 1: End-to-end server lifecycle
#[sqlx::test]
async fn test_end_to_end_server_lifecycle(pool: SqlitePool) {
    // 1. Create test user
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .expect("Failed to create test user");

    // 2. Create registry
    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));

    // 3. Create server in DB
    let (server_id, server_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Test Server", Some("Lifecycle test"))
            .await
            .expect("Failed to create test server");

    let server = Server::get_by_id(&pool, server_id)
        .await
        .expect("Failed to get server")
        .expect("Server should exist");

    // 4. Register with MCP registry
    registry
        .write()
        .await
        .register_server(&server.uuid)
        .await
        .expect("Server registration should succeed");

    // 5. Verify server is available
    let instance = registry.read().await.get_instance(&server.uuid);
    assert!(instance.is_some(), "Server should be registered");

    let instance = instance.unwrap();
    assert_eq!(
        instance.server_id, server_id,
        "Instance should have correct server_id"
    );
    assert_eq!(
        instance.server_uuid, server_uuid,
        "Instance should have correct UUID"
    );

    // 6. Unregister server
    registry
        .write()
        .await
        .unregister_server(&server.uuid)
        .await
        .expect("Server unregistration should succeed");

    // 7. Verify server is gone
    let instance = registry.read().await.get_instance(&server.uuid);
    assert!(instance.is_none(), "Server should be unregistered");

    // 8. Delete server from DB
    Server::delete(&pool, server_id, user_id)
        .await
        .expect("Server deletion should succeed");

    // 9. Verify server is deleted from DB
    let server = Server::get_by_id(&pool, server_id)
        .await
        .expect("Query should succeed");
    assert!(server.is_none(), "Server should be deleted from database");
}

// Test 2: Hot reload tools
#[sqlx::test]
async fn test_hot_reload_tools(pool: SqlitePool) {
    // 1. Create user, toolkit, tool
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
        Some("https://example.com/{{param}}"),
        None,
        None,
        5000,
    )
    .await
    .expect("Failed to create tool");

    // 2. Create server and register
    let (server_id, server_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Test Server", None)
            .await
            .expect("Failed to create server");

    // Link toolkit to server
    sqlx::query!(
        "INSERT INTO server_toolkits (server_id, toolkit_id) VALUES (?, ?)",
        server_id,
        toolkit_id
    )
    .execute(&pool)
    .await
    .expect("Failed to link toolkit to server");

    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));
    registry
        .write()
        .await
        .register_server(&server_uuid)
        .await
        .expect("Server registration should succeed");

    // 3. Add tool instance
    let instance_id =
        ToolInstance::create(&pool, server_id, tool_id, "my_tool", Some("Test instance"))
            .await
            .expect("Failed to create tool instance");

    // 4. Trigger reload
    let result = registry.write().await.reload_tools(&server_uuid).await;
    assert!(result.is_ok(), "Reload should succeed: {:?}", result.err());

    // Note: Full tool verification will be done in Task 003
    // For now, we're just testing the reload mechanism exists and doesn't error

    // 5. Verify instance still exists in DB after reload
    let instance_check = ToolInstance::get_by_id(&pool, instance_id)
        .await
        .expect("Failed to get instance")
        .expect("Instance should exist");

    assert_eq!(
        instance_check.instance_name, "my_tool",
        "Instance name should match"
    );
}

// Test 3: Multiple servers isolation
#[sqlx::test]
async fn test_multiple_servers_isolation(pool: SqlitePool) {
    // Create 3 servers
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .expect("Failed to create test user");

    let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));

    let mut server_uuids = vec![];
    let mut server_ids = vec![];

    for i in 1..=3 {
        let (server_id, server_uuid) =
            test_helpers::create_test_server(&pool, user_id, &format!("Server {}", i), None)
                .await
                .unwrap_or_else(|_| panic!("Failed to create server {}", i));

        registry
            .write()
            .await
            .register_server(&server_uuid)
            .await
            .unwrap_or_else(|_| panic!("Failed to register server {}", i));

        server_uuids.push(server_uuid.clone());
        server_ids.push(server_id);
    }

    // Verify all 3 are registered and isolated
    for (i, uuid) in server_uuids.iter().enumerate() {
        let instance = registry.read().await.get_instance(uuid);
        assert!(instance.is_some(), "Server {} should be registered", i + 1);

        let instance = instance.unwrap();
        assert_eq!(
            instance.server_id,
            server_ids[i],
            "Server {} should have correct server_id",
            i + 1
        );
        assert_eq!(
            instance.server_uuid,
            *uuid,
            "Server {} should have correct UUID",
            i + 1
        );
    }

    // Unregister server 2
    registry
        .write()
        .await
        .unregister_server(&server_uuids[1])
        .await
        .expect("Unregistering server 2 should succeed");

    // Verify server 1 and 3 still exist, but server 2 is gone
    assert!(
        registry
            .read()
            .await
            .get_instance(&server_uuids[0])
            .is_some(),
        "Server 1 should still exist"
    );
    assert!(
        registry
            .read()
            .await
            .get_instance(&server_uuids[1])
            .is_none(),
        "Server 2 should be removed"
    );
    assert!(
        registry
            .read()
            .await
            .get_instance(&server_uuids[2])
            .is_some(),
        "Server 3 should still exist"
    );

    // Verify server 1 and 3 have correct data
    let instance1 = registry
        .read()
        .await
        .get_instance(&server_uuids[0])
        .unwrap();
    assert_eq!(
        instance1.server_id, server_ids[0],
        "Server 1 should still have correct server_id"
    );

    let instance3 = registry
        .read()
        .await
        .get_instance(&server_uuids[2])
        .unwrap();
    assert_eq!(
        instance3.server_id, server_ids[2],
        "Server 3 should still have correct server_id"
    );

    // Cleanup: Unregister remaining servers
    registry
        .write()
        .await
        .unregister_server(&server_uuids[0])
        .await
        .expect("Unregistering server 1 should succeed");

    registry
        .write()
        .await
        .unregister_server(&server_uuids[2])
        .await
        .expect("Unregistering server 3 should succeed");

    // Verify all are gone
    assert!(
        registry
            .read()
            .await
            .get_instance(&server_uuids[0])
            .is_none(),
        "Server 1 should be removed after cleanup"
    );
    assert!(
        registry
            .read()
            .await
            .get_instance(&server_uuids[2])
            .is_none(),
        "Server 3 should be removed after cleanup"
    );
}
