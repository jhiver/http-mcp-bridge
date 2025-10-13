use saramcp::{
    models::oauth::{OAuthAuthorizationCode, OAuthClient},
    test_utils::test_helpers,
};

// ============================================================================
// OAuthClient Tests
// ============================================================================

#[tokio::test]
async fn test_create_oauth_client() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let (client_id, client_secret) = OAuthClient::create(
        &pool,
        Some(user_id),
        "Test Client",
        "http://localhost/callback",
    )
    .await
    .unwrap();

    // Should return client_id and client_secret
    assert!(client_id.starts_with("mcp_"));
    assert!(!client_secret.is_empty());

    // Should be retrievable from database
    let client = OAuthClient::get_by_client_id(&pool, &client_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(client.client_id, client_id);
    assert_eq!(client.name, "Test Client");
    assert_eq!(client.user_id, Some(user_id));
}

#[tokio::test]
async fn test_oauth_client_secret_hashed() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let (client_id, client_secret) = OAuthClient::create(
        &pool,
        Some(user_id),
        "Test Client",
        "http://localhost/callback",
    )
    .await
    .unwrap();

    // Stored secret should be hashed (not equal to plaintext)
    let client = OAuthClient::get_by_client_id(&pool, &client_id)
        .await
        .unwrap()
        .unwrap();

    assert_ne!(client.client_secret_hash, client_secret);
    assert_eq!(client.client_secret_hash.len(), 64); // SHA-256 hex = 64 chars
}

#[tokio::test]
async fn test_verify_client_secret_success() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let (client_id, client_secret) = OAuthClient::create(
        &pool,
        Some(user_id),
        "Test Client",
        "http://localhost/callback",
    )
    .await
    .unwrap();

    let client = OAuthClient::get_by_client_id(&pool, &client_id)
        .await
        .unwrap()
        .unwrap();

    // Correct secret should verify
    let is_valid = client.verify_secret(&client_secret);

    assert!(is_valid);
}

#[tokio::test]
async fn test_verify_client_secret_failure() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let (client_id, _) = OAuthClient::create(
        &pool,
        Some(user_id),
        "Test Client",
        "http://localhost/callback",
    )
    .await
    .unwrap();

    let client = OAuthClient::get_by_client_id(&pool, &client_id)
        .await
        .unwrap()
        .unwrap();

    // Wrong secret should fail
    let is_valid = client.verify_secret("wrong_secret");

    assert!(!is_valid);
}

#[tokio::test]
async fn test_list_clients_by_user() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    // Create multiple clients
    OAuthClient::create(
        &pool,
        Some(user_id),
        "Client 1",
        "http://localhost/callback1",
    )
    .await
    .unwrap();

    OAuthClient::create(
        &pool,
        Some(user_id),
        "Client 2",
        "http://localhost/callback2",
    )
    .await
    .unwrap();

    let clients = OAuthClient::list_by_user(&pool, user_id).await.unwrap();

    assert_eq!(clients.len(), 2);
    assert!(clients.iter().any(|c| c.name == "Client 1"));
    assert!(clients.iter().any(|c| c.name == "Client 2"));
}

// ============================================================================
// OAuthAuthorizationCode Tests
// ============================================================================

#[tokio::test]
async fn test_create_authorization_code() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let (client_id, _) = OAuthClient::create(
        &pool,
        Some(user_id),
        "Test Client",
        "http://localhost/callback",
    )
    .await
    .unwrap();

    let code = OAuthAuthorizationCode::create(
        &pool,
        &client_id,
        user_id,
        "http://localhost/callback",
        "mcp:read mcp:write",
        None,
        None,
    )
    .await
    .unwrap();

    // Code should have correct format
    assert!(code.starts_with("code_"));

    // Should be retrievable
    let auth_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(auth_code.code, code);
    assert_eq!(auth_code.client_id, client_id);
    assert_eq!(auth_code.user_id, user_id);
    assert_eq!(auth_code.redirect_uri, "http://localhost/callback");
    assert_eq!(auth_code.scope, "mcp:read mcp:write");
}

#[tokio::test]
async fn test_authorization_code_with_pkce() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let (client_id, _) = OAuthClient::create(
        &pool,
        Some(user_id),
        "Test Client",
        "http://localhost/callback",
    )
    .await
    .unwrap();

    let code = OAuthAuthorizationCode::create(
        &pool,
        &client_id,
        user_id,
        "http://localhost/callback",
        "mcp:read mcp:write",
        Some("test_challenge"),
        Some("S256"),
    )
    .await
    .unwrap();

    let auth_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(auth_code.code_challenge, Some("test_challenge".to_string()));
    assert_eq!(auth_code.code_challenge_method, Some("S256".to_string()));
}

#[tokio::test]
async fn test_authorization_code_expiration() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let (client_id, _) = OAuthClient::create(
        &pool,
        Some(user_id),
        "Test Client",
        "http://localhost/callback",
    )
    .await
    .unwrap();

    let code = OAuthAuthorizationCode::create(
        &pool,
        &client_id,
        user_id,
        "http://localhost/callback",
        "mcp:read mcp:write",
        None,
        None,
    )
    .await
    .unwrap();

    let auth_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();

    // Should not be expired immediately
    assert!(!auth_code.is_expired());

    // Expires_at should be in the future (10 minutes from now)
    let now = chrono::Utc::now().timestamp();
    assert!(auth_code.expires_at > now);
    assert!(auth_code.expires_at <= now + 600); // 10 minutes
}

#[tokio::test]
async fn test_authorization_code_mark_used() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let (client_id, _) = OAuthClient::create(
        &pool,
        Some(user_id),
        "Test Client",
        "http://localhost/callback",
    )
    .await
    .unwrap();

    let code = OAuthAuthorizationCode::create(
        &pool,
        &client_id,
        user_id,
        "http://localhost/callback",
        "mcp:read mcp:write",
        None,
        None,
    )
    .await
    .unwrap();

    // Initially not used
    let auth_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();
    assert!(!auth_code.is_used());

    // Mark as used
    auth_code.mark_used(&pool).await.unwrap();

    // Should now be marked as used
    let auth_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();
    assert!(auth_code.is_used());
}

// ============================================================================
// OAuthAccessToken and OAuthRefreshToken Tests
// ============================================================================
// FIXME: These tests have been commented out because they rely on
// OAuthAccessToken::create() and OAuthRefreshToken::create() methods
// that don't exist. These should be rewritten to use OAuthService methods
// or the model methods should be implemented.
//
// Tests that need rewriting:
// - test_create_access_token
// - test_access_token_hashed
// - test_access_token_expiration
// - test_access_token_update_last_used
// - test_create_refresh_token
// - test_refresh_token_hashed
// - test_refresh_token_expiration
// - test_refresh_token_rotation
// - test_refresh_token_used_flag
// - test_client_deletion_cascades
// - test_token_hash_consistency
