use saramcp::{models::server::Server, test_utils::test_helpers};
use uuid::Uuid;

#[tokio::test]
async fn test_create_server_generates_uuid() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server_id = Server::create(&pool, user_id, "Test Server", Some("Description"))
        .await
        .unwrap();

    let server = Server::get_by_id(&pool, server_id).await.unwrap().unwrap();

    // Verify UUID is generated
    assert!(!server.uuid.is_empty());

    // Verify UUID is valid v4 format
    let parsed_uuid = Uuid::parse_str(&server.uuid);
    assert!(parsed_uuid.is_ok(), "UUID should be in valid format");
}

#[tokio::test]
async fn test_uuid_uniqueness() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    // Create multiple servers
    let server1_id = Server::create(&pool, user_id, "Server 1", None)
        .await
        .unwrap();
    let server2_id = Server::create(&pool, user_id, "Server 2", None)
        .await
        .unwrap();
    let server3_id = Server::create(&pool, user_id, "Server 3", None)
        .await
        .unwrap();

    let server1 = Server::get_by_id(&pool, server1_id).await.unwrap().unwrap();
    let server2 = Server::get_by_id(&pool, server2_id).await.unwrap().unwrap();
    let server3 = Server::get_by_id(&pool, server3_id).await.unwrap().unwrap();

    // Verify all UUIDs are different
    assert_ne!(server1.uuid, server2.uuid);
    assert_ne!(server1.uuid, server3.uuid);
    assert_ne!(server2.uuid, server3.uuid);
}

#[tokio::test]
async fn test_get_server_by_uuid() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server_id = Server::create(&pool, user_id, "Test Server", Some("Description"))
        .await
        .unwrap();

    let created_server = Server::get_by_id(&pool, server_id).await.unwrap().unwrap();
    let uuid = created_server.uuid.clone();

    // Retrieve by UUID
    let retrieved_server = Server::get_by_uuid(&pool, &uuid).await.unwrap();

    assert!(retrieved_server.is_some());
    let retrieved = retrieved_server.unwrap();
    assert_eq!(retrieved.id, Some(server_id));
    assert_eq!(retrieved.uuid, uuid);
    assert_eq!(retrieved.name, "Test Server");
    assert_eq!(retrieved.description, Some("Description".to_string()));
}

#[tokio::test]
async fn test_get_server_by_invalid_uuid() {
    let pool = test_helpers::create_test_db().await.unwrap();

    let result = Server::get_by_uuid(&pool, "invalid-uuid").await.unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_server_by_nonexistent_uuid() {
    let pool = test_helpers::create_test_db().await.unwrap();

    let nonexistent_uuid = Uuid::new_v4().to_string();
    let result = Server::get_by_uuid(&pool, &nonexistent_uuid).await.unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn test_uuid_format_is_v4() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server_id = Server::create(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    let server = Server::get_by_id(&pool, server_id).await.unwrap().unwrap();

    // Parse UUID and verify it's version 4
    let uuid = Uuid::parse_str(&server.uuid).unwrap();
    assert_eq!(uuid.get_version_num(), 4);
}

#[tokio::test]
async fn test_uuid_persists_across_queries() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server_id = Server::create(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    // Get server multiple times
    let server1 = Server::get_by_id(&pool, server_id).await.unwrap().unwrap();
    let server2 = Server::get_by_id(&pool, server_id).await.unwrap().unwrap();
    let server3 = Server::get_by_id_and_user(&pool, server_id, user_id)
        .await
        .unwrap()
        .unwrap();

    // UUID should be identical across queries
    assert_eq!(server1.uuid, server2.uuid);
    assert_eq!(server1.uuid, server3.uuid);
}

#[tokio::test]
async fn test_uuid_in_server_list() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server1_id = Server::create(&pool, user_id, "Server 1", None)
        .await
        .unwrap();
    let server2_id = Server::create(&pool, user_id, "Server 2", None)
        .await
        .unwrap();

    let server1 = Server::get_by_id(&pool, server1_id).await.unwrap().unwrap();
    let server2 = Server::get_by_id(&pool, server2_id).await.unwrap().unwrap();

    // Get list of servers
    let servers = Server::list_by_user(&pool, user_id).await.unwrap();

    assert_eq!(servers.len(), 2);

    // Verify UUIDs are present in the list
    let uuids: Vec<String> = servers.iter().map(|s| s.uuid.clone()).collect();
    assert!(uuids.contains(&server1.uuid));
    assert!(uuids.contains(&server2.uuid));
}
