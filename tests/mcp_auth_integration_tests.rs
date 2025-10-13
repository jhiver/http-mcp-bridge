use chrono::Utc;
use saramcp::{services::OAuthService, test_utils::test_helpers};
use sqlx::SqlitePool;
use std::sync::Arc;

/// Helper: Update server access level
async fn set_server_access_level(
    pool: &SqlitePool,
    server_id: i64,
    access_level: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE servers SET access_level = ? WHERE id = ?",
        access_level,
        server_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Helper: Create OAuth client and return client_id
async fn create_oauth_client(oauth_service: &OAuthService, user_id: i64) -> anyhow::Result<String> {
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test MCP Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;
    Ok(client.client_id)
}

/// Helper: Create access token and return plaintext token
async fn create_access_token(
    oauth_service: &OAuthService,
    client_id: &str,
    user_id: i64,
) -> anyhow::Result<String> {
    let (token, _expires_at) = oauth_service
        .create_access_token(client_id, user_id, "mcp:read")
        .await?;
    Ok(token)
}

/// Helper: Create expired access token
async fn create_expired_access_token(
    pool: &SqlitePool,
    oauth_service: &OAuthService,
    client_id: &str,
    user_id: i64,
) -> anyhow::Result<String> {
    // Create a normal token first
    let (token, _) = oauth_service
        .create_access_token(client_id, user_id, "mcp:read")
        .await?;

    // Update it to be expired
    let token_hash = oauth_service.hash_token(&token);
    let expired_at = Utc::now().timestamp() - 3600; // 1 hour ago

    sqlx::query!(
        "UPDATE oauth_access_tokens SET expires_at = ? WHERE token_hash = ?",
        expired_at,
        token_hash
    )
    .execute(pool)
    .await?;

    Ok(token)
}

/// Test 1: Validate access token - valid token
#[tokio::test]
async fn test_validate_access_token_success() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and OAuth client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let client_id = create_oauth_client(&oauth_service, user_id).await?;

    // Create access token
    let token = create_access_token(&oauth_service, &client_id, user_id).await?;

    // Validate token
    let validated = oauth_service.validate_access_token(&token).await?;

    assert_eq!(validated.user_id, user_id);
    assert_eq!(validated.client_id, client_id);
    assert_eq!(validated.scope, "mcp:read");
    assert!(validated.expires_at > Utc::now().timestamp());

    Ok(())
}

/// Test 2: Validate access token - invalid token
#[tokio::test]
async fn test_validate_access_token_invalid() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Try to validate non-existent token
    let result = oauth_service
        .validate_access_token("invalid_token_123")
        .await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid access token"));

    Ok(())
}

/// Test 3: Validate access token - expired token
#[tokio::test]
async fn test_validate_access_token_expired() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and OAuth client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let client_id = create_oauth_client(&oauth_service, user_id).await?;

    // Create expired access token
    let token = create_expired_access_token(&pool, &oauth_service, &client_id, user_id).await?;

    // Try to validate expired token
    let result = oauth_service.validate_access_token(&token).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("expired"));

    Ok(())
}

/// Test 4: Can access server - public server (any user)
#[tokio::test]
async fn test_can_access_server_public() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and server
    let user_id =
        test_helpers::insert_test_user(&pool, "owner@example.com", "password", true).await?;
    let (server_id, server_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Test Server", None).await?;

    // Set access level to public
    set_server_access_level(&pool, server_id, "public").await?;

    // Different user (not owner)
    let other_user_id =
        test_helpers::insert_test_user(&pool, "other@example.com", "password", true).await?;

    // Check access
    let can_access = oauth_service
        .can_access_server(&server_uuid, other_user_id)
        .await?;

    assert!(
        can_access,
        "Any user should be able to access public server"
    );

    Ok(())
}

/// Test 5: Can access server - organization server (any authenticated user)
#[tokio::test]
async fn test_can_access_server_organization() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and server
    let user_id =
        test_helpers::insert_test_user(&pool, "owner@example.com", "password", true).await?;
    let (server_id, server_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Test Server", None).await?;

    // Set access level to organization
    set_server_access_level(&pool, server_id, "organization").await?;

    // Different user (not owner)
    let other_user_id =
        test_helpers::insert_test_user(&pool, "other@example.com", "password", true).await?;

    // Check access
    let can_access = oauth_service
        .can_access_server(&server_uuid, other_user_id)
        .await?;

    assert!(
        can_access,
        "Any authenticated user should be able to access organization server"
    );

    Ok(())
}

/// Test 6: Can access server - private server (owner only)
#[tokio::test]
async fn test_can_access_server_private_owner() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and server
    let user_id =
        test_helpers::insert_test_user(&pool, "owner@example.com", "password", true).await?;
    let (server_id, server_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Test Server", None).await?;

    // Set access level to private
    set_server_access_level(&pool, server_id, "private").await?;

    // Check owner access
    let can_access = oauth_service
        .can_access_server(&server_uuid, user_id)
        .await?;

    assert!(can_access, "Owner should be able to access private server");

    Ok(())
}

/// Test 7: Can access server - private server (non-owner denied)
#[tokio::test]
async fn test_can_access_server_private_non_owner() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and server
    let user_id =
        test_helpers::insert_test_user(&pool, "owner@example.com", "password", true).await?;
    let (server_id, server_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Test Server", None).await?;

    // Set access level to private
    set_server_access_level(&pool, server_id, "private").await?;

    // Different user (not owner)
    let other_user_id =
        test_helpers::insert_test_user(&pool, "other@example.com", "password", true).await?;

    // Check access
    let can_access = oauth_service
        .can_access_server(&server_uuid, other_user_id)
        .await?;

    assert!(
        !can_access,
        "Non-owner should NOT be able to access private server"
    );

    Ok(())
}

/// Test 8: Can access server - server not found
#[tokio::test]
async fn test_can_access_server_not_found() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Try to check access for non-existent server
    let result = oauth_service
        .can_access_server("non-existent-uuid", 1)
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Server not found"));

    Ok(())
}

/// Test 9: Can access server - default access level (private)
#[tokio::test]
async fn test_can_access_server_default_is_private() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and server (no access level explicitly set, defaults to 'private' per migration)
    let user_id =
        test_helpers::insert_test_user(&pool, "owner@example.com", "password", true).await?;
    let (_server_id, server_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Test Server", None).await?;

    // Different user (not owner)
    let other_user_id =
        test_helpers::insert_test_user(&pool, "other@example.com", "password", true).await?;

    // Check access (default should be private, so non-owner cannot access)
    let can_access = oauth_service
        .can_access_server(&server_uuid, other_user_id)
        .await?;

    assert!(
        !can_access,
        "Default access level is private, non-owner should be denied"
    );

    Ok(())
}

/// Test 10: Token validation updates last_used_at
#[tokio::test]
async fn test_validate_token_updates_last_used() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and OAuth client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let client_id = create_oauth_client(&oauth_service, user_id).await?;

    // Create access token
    let token = create_access_token(&oauth_service, &client_id, user_id).await?;

    // Get initial last_used_at (should be NULL)
    let token_hash = oauth_service.hash_token(&token);
    let initial_last_used: Option<i64> =
        sqlx::query_scalar("SELECT last_used_at FROM oauth_access_tokens WHERE token_hash = ?")
            .bind(&token_hash)
            .fetch_one(&pool)
            .await?;

    assert!(
        initial_last_used.is_none(),
        "Initial last_used_at should be NULL"
    );

    // Validate token (should update last_used_at)
    oauth_service.validate_access_token(&token).await?;

    // Give async update a moment to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Check last_used_at was updated
    let updated_last_used: Option<i64> =
        sqlx::query_scalar("SELECT last_used_at FROM oauth_access_tokens WHERE token_hash = ?")
            .bind(&token_hash)
            .fetch_one(&pool)
            .await?;

    assert!(
        updated_last_used.is_some(),
        "last_used_at should be updated after validation"
    );
    assert!(
        updated_last_used.unwrap() > 0,
        "last_used_at should be a valid timestamp"
    );

    Ok(())
}

/// Test 11: Hash token produces consistent results
#[tokio::test]
async fn test_hash_token_consistency() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    let token = "test_token_123";
    let hash1 = oauth_service.hash_token(token);
    let hash2 = oauth_service.hash_token(token);

    assert_eq!(hash1, hash2, "Hash should be consistent for same input");
    assert_eq!(hash1.len(), 64, "SHA-256 hash should be 64 hex characters");

    Ok(())
}

/// Test 12: Multiple concurrent token validations
#[tokio::test]
async fn test_concurrent_token_validations() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and OAuth client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let client_id = create_oauth_client(&oauth_service, user_id).await?;

    // Create access token
    let token = create_access_token(&oauth_service, &client_id, user_id).await?;

    // Validate token concurrently 10 times
    let mut handles = vec![];
    for _ in 0..10 {
        let service = oauth_service.clone();
        let token_clone = token.clone();
        let handle = tokio::spawn(async move { service.validate_access_token(&token_clone).await });
        handles.push(handle);
    }

    // Wait for all validations to complete
    for handle in handles {
        let result = handle.await?;
        assert!(result.is_ok(), "Concurrent validation should succeed");
    }

    Ok(())
}
