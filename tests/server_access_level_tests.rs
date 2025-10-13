use saramcp::{models::server::Server, test_utils::test_helpers};

#[tokio::test]
async fn test_server_defaults_to_private() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server_id = Server::create(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    let server = Server::get_by_id(&pool, server_id).await.unwrap().unwrap();

    assert_eq!(server.access_level, Some("private".to_string()));
}

#[tokio::test]
async fn test_create_server_with_description_defaults_to_private() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server_id = Server::create(
        &pool,
        user_id,
        "Test Server",
        Some("A test server description"),
    )
    .await
    .unwrap();

    let server = Server::get_by_id(&pool, server_id).await.unwrap().unwrap();

    assert_eq!(server.access_level, Some("private".to_string()));
}

#[tokio::test]
async fn test_update_server_access_level_to_organization() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server_id = Server::create(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    // Update to organization
    sqlx::query!(
        "UPDATE servers SET access_level = ? WHERE id = ?",
        "organization",
        server_id
    )
    .execute(&pool)
    .await
    .unwrap();

    let server = Server::get_by_id(&pool, server_id).await.unwrap().unwrap();
    assert_eq!(server.access_level, Some("organization".to_string()));
}

#[tokio::test]
async fn test_update_server_access_level_to_public() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server_id = Server::create(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    // Update to public
    sqlx::query!(
        "UPDATE servers SET access_level = ? WHERE id = ?",
        "public",
        server_id
    )
    .execute(&pool)
    .await
    .unwrap();

    let server = Server::get_by_id(&pool, server_id).await.unwrap().unwrap();
    assert_eq!(server.access_level, Some("public".to_string()));
}

#[tokio::test]
async fn test_invalid_access_level_rejected_by_check_constraint() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server_id = Server::create(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    // Try to set invalid access level
    let result = sqlx::query!(
        "UPDATE servers SET access_level = ? WHERE id = ?",
        "invalid_level",
        server_id
    )
    .execute(&pool)
    .await;

    assert!(
        result.is_err(),
        "Expected CHECK constraint to reject invalid access level"
    );
}

#[tokio::test]
async fn test_get_by_uuid_includes_access_level() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server_id = Server::create(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    let server = Server::get_by_id(&pool, server_id).await.unwrap().unwrap();
    let uuid = server.uuid.clone();

    let server_by_uuid = Server::get_by_uuid(&pool, &uuid).await.unwrap().unwrap();

    assert_eq!(server_by_uuid.access_level, Some("private".to_string()));
}

#[tokio::test]
async fn test_get_by_id_and_user_includes_access_level() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server_id = Server::create(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    let server = Server::get_by_id_and_user(&pool, server_id, user_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(server.access_level, Some("private".to_string()));
}

#[tokio::test]
async fn test_list_by_user_includes_access_level() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    Server::create(&pool, user_id, "Server 1", None)
        .await
        .unwrap();
    Server::create(&pool, user_id, "Server 2", None)
        .await
        .unwrap();

    let servers = Server::list_by_user(&pool, user_id).await.unwrap();

    assert_eq!(servers.len(), 2);
    for server in servers {
        assert_eq!(server.access_level, Some("private".to_string()));
    }
}

#[tokio::test]
async fn test_server_service_update_access_level() {
    use saramcp::services::{SecretsManager, ServerService};

    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server_id = Server::create(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    let secrets = SecretsManager::new().unwrap();
    let service = ServerService::new(pool.clone(), secrets);

    // Update to organization
    service
        .update_server_access(server_id, user_id, "organization")
        .await
        .unwrap();

    let server = Server::get_by_id(&pool, server_id).await.unwrap().unwrap();
    assert_eq!(server.access_level, Some("organization".to_string()));

    // Update to public
    service
        .update_server_access(server_id, user_id, "public")
        .await
        .unwrap();

    let server = Server::get_by_id(&pool, server_id).await.unwrap().unwrap();
    assert_eq!(server.access_level, Some("public".to_string()));

    // Update back to private
    service
        .update_server_access(server_id, user_id, "private")
        .await
        .unwrap();

    let server = Server::get_by_id(&pool, server_id).await.unwrap().unwrap();
    assert_eq!(server.access_level, Some("private".to_string()));
}

#[tokio::test]
async fn test_server_service_rejects_invalid_access_level() {
    use saramcp::services::{SecretsManager, ServerService};

    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let server_id = Server::create(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    let secrets = SecretsManager::new().unwrap();
    let service = ServerService::new(pool.clone(), secrets);

    // Try to set invalid access level
    let result = service
        .update_server_access(server_id, user_id, "invalid")
        .await;

    assert!(result.is_err(), "Expected service to reject invalid level");
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid access level"));
}

#[tokio::test]
async fn test_server_service_rejects_unauthorized_access_update() {
    use saramcp::services::{SecretsManager, ServerService};

    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "owner@example.com", "password", true)
        .await
        .unwrap();
    let other_user_id =
        test_helpers::insert_test_user(&pool, "other@example.com", "password", true)
            .await
            .unwrap();

    let server_id = Server::create(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    let secrets = SecretsManager::new().unwrap();
    let service = ServerService::new(pool, secrets);

    // Try to update as different user
    let result = service
        .update_server_access(server_id, other_user_id, "public")
        .await;

    assert!(
        result.is_err(),
        "Expected service to reject unauthorized update"
    );
}
