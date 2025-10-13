use saramcp::{services::OAuthService, test_utils::test_helpers};
use std::sync::Arc;

/// Test authorization code grant: basic code-to-token exchange
#[tokio::test]
async fn test_authorization_code_grant_basic() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;

    // Register OAuth client
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create authorization code (no PKCE)
    let code = oauth_service
        .create_authorization_code(
            &client.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await?;

    // Exchange code for tokens
    let consumed = oauth_service
        .consume_authorization_code(&code, &client.client_id, "http://localhost:3000/callback")
        .await?;

    assert_eq!(consumed.user_id, user_id);
    assert_eq!(consumed.scope, "mcp:read");
    assert!(consumed.code_challenge.is_none());

    // Generate tokens
    let (access_token, _) = oauth_service
        .create_access_token(&client.client_id, consumed.user_id, &consumed.scope)
        .await?;
    let refresh_token = oauth_service
        .create_refresh_token(&client.client_id, consumed.user_id, &consumed.scope)
        .await?;

    assert!(access_token.starts_with("mcp_token_"));
    assert!(refresh_token.starts_with("mcp_refresh_"));

    Ok(())
}

/// Test PKCE S256 validation success
#[tokio::test]
async fn test_pkce_s256_validation_success() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create verifier and compute S256 challenge
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

    use base64::Engine;
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize());

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create authorization code with PKCE challenge
    let code = oauth_service
        .create_authorization_code(
            &client.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            Some(&challenge),
            Some("S256"),
        )
        .await?;

    // Consume code
    let consumed = oauth_service
        .consume_authorization_code(&code, &client.client_id, "http://localhost:3000/callback")
        .await?;

    // Validate PKCE
    let result = oauth_service.validate_pkce(
        verifier,
        &consumed.code_challenge.unwrap(),
        &consumed.code_challenge_method.unwrap(),
    );
    assert!(result.is_ok());

    Ok(())
}

/// Test PKCE plain method validation
#[tokio::test]
async fn test_pkce_plain_method_validation() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    let verifier = "plain-text-verifier-12345";
    let challenge = verifier; // In plain method, challenge == verifier

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create authorization code with plain PKCE
    let code = oauth_service
        .create_authorization_code(
            &client.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            Some(challenge),
            Some("plain"),
        )
        .await?;

    let consumed = oauth_service
        .consume_authorization_code(&code, &client.client_id, "http://localhost:3000/callback")
        .await?;

    // Validate PKCE
    let result = oauth_service.validate_pkce(
        verifier,
        &consumed.code_challenge.unwrap(),
        &consumed.code_challenge_method.unwrap(),
    );
    assert!(result.is_ok());

    Ok(())
}

/// Test invalid code_verifier rejection
#[tokio::test]
async fn test_invalid_code_verifier_rejection() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Compute correct challenge
    let correct_verifier = "correct-verifier-12345";
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(correct_verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize());

    // Try to validate with wrong verifier
    let wrong_verifier = "wrong-verifier-12345";

    let result = oauth_service.validate_pkce(wrong_verifier, &challenge, "S256");
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid code_verifier"));

    Ok(())
}

/// Test expired authorization code rejection
#[tokio::test]
async fn test_expired_authorization_code_rejection() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create authorization code that's already expired
    let code = format!("code_{}", uuid::Uuid::new_v4());
    let now = chrono::Utc::now().timestamp();
    let expired_at = now - 600; // 10 minutes ago

    sqlx::query!(
        r#"
        INSERT INTO oauth_authorization_codes
        (code, client_id, user_id, redirect_uri, scope, expires_at)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
        code,
        client.client_id,
        user_id,
        "http://localhost:3000/callback",
        "mcp:read",
        expired_at
    )
    .execute(&pool)
    .await?;

    // Try to consume expired code
    let result = oauth_service
        .consume_authorization_code(&code, &client.client_id, "http://localhost:3000/callback")
        .await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Authorization code expired"));

    Ok(())
}

/// Test used authorization code rejection (single-use)
#[tokio::test]
async fn test_used_authorization_code_rejection() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create authorization code
    let code = oauth_service
        .create_authorization_code(
            &client.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await?;

    // First use - should succeed
    let result1 = oauth_service
        .consume_authorization_code(&code, &client.client_id, "http://localhost:3000/callback")
        .await;
    assert!(result1.is_ok());

    // Second use - should fail
    let result2 = oauth_service
        .consume_authorization_code(&code, &client.client_id, "http://localhost:3000/callback")
        .await;
    assert!(result2.is_err());
    assert!(result2
        .unwrap_err()
        .to_string()
        .contains("Authorization code already used"));

    Ok(())
}

/// Test client mismatch rejection
#[tokio::test]
async fn test_client_mismatch_rejection() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and two clients
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;

    let registration1 = saramcp::services::ClientRegistrationRequest {
        client_name: "Client 1".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client1 = oauth_service
        .register_client(Some(user_id), registration1)
        .await?;

    let registration2 = saramcp::services::ClientRegistrationRequest {
        client_name: "Client 2".to_string(),
        redirect_uris: vec!["http://localhost:3001/callback".to_string()],
    };
    let client2 = oauth_service
        .register_client(Some(user_id), registration2)
        .await?;

    // Create authorization code for client1
    let code = oauth_service
        .create_authorization_code(
            &client1.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await?;

    // Try to consume with client2 - should fail
    let result = oauth_service
        .consume_authorization_code(&code, &client2.client_id, "http://localhost:3000/callback")
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Client mismatch"));

    Ok(())
}

/// Test redirect URI mismatch rejection
#[tokio::test]
async fn test_redirect_uri_mismatch_rejection() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create authorization code with specific redirect_uri
    let code = oauth_service
        .create_authorization_code(
            &client.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await?;

    // Try to consume with different redirect_uri
    let result = oauth_service
        .consume_authorization_code(
            &code,
            &client.client_id,
            "http://localhost:3000/different-callback",
        )
        .await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Redirect URI mismatch"));

    Ok(())
}

/// Test access token format and expiry
#[tokio::test]
async fn test_access_token_format_and_expiry() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create access token
    let before = chrono::Utc::now().timestamp();
    let (access_token, expires_at) = oauth_service
        .create_access_token(&client.client_id, user_id, "mcp:read")
        .await?;
    let after = chrono::Utc::now().timestamp();

    // Verify format
    assert!(access_token.starts_with("mcp_token_"));

    // Verify expiry is 1 hour from now (with small tolerance)
    let expected_min = before + 3600;
    let expected_max = after + 3600;
    assert!(
        (expected_min..=expected_max).contains(&expires_at),
        "Expiry {} not in range {}..{}",
        expires_at,
        expected_min,
        expected_max
    );

    Ok(())
}

/// Test refresh token format and expiry
#[tokio::test]
async fn test_refresh_token_format_and_expiry() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create refresh token
    let refresh_token = oauth_service
        .create_refresh_token(&client.client_id, user_id, "mcp:read")
        .await?;

    // Verify format
    assert!(refresh_token.starts_with("mcp_refresh_"));

    // Verify it was stored with 30 day expiry
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(refresh_token.as_bytes());
    let token_hash = format!("{:x}", hasher.finalize());

    let stored = sqlx::query!(
        "SELECT expires_at FROM oauth_refresh_tokens WHERE token_hash = ?",
        token_hash
    )
    .fetch_one(&pool)
    .await?;

    let now = chrono::Utc::now().timestamp();
    let expected_min = now + (30 * 24 * 3600) - 5; // 30 days minus 5 sec tolerance
    let expected_max = now + (30 * 24 * 3600) + 5; // 30 days plus 5 sec tolerance

    assert!(
        (expected_min..=expected_max).contains(&stored.expires_at),
        "Expiry {} not in 30-day range",
        stored.expires_at
    );

    Ok(())
}

/// Test basic refresh token flow
#[tokio::test]
async fn test_refresh_token_basic_flow() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create refresh token
    let refresh_token = oauth_service
        .create_refresh_token(&client.client_id, user_id, "mcp:read mcp:write")
        .await?;

    // Consume refresh token
    let consumed = oauth_service.consume_refresh_token(&refresh_token).await?;

    assert_eq!(consumed.client_id, client.client_id);
    assert_eq!(consumed.user_id, user_id);
    assert_eq!(consumed.scope, "mcp:read mcp:write");

    Ok(())
}

/// Test refresh token rotation
#[tokio::test]
async fn test_refresh_token_rotation() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create initial refresh token
    let refresh_token_1 = oauth_service
        .create_refresh_token(&client.client_id, user_id, "mcp:read")
        .await?;

    // Consume it (marks as used)
    let consumed = oauth_service
        .consume_refresh_token(&refresh_token_1)
        .await?;

    // Create NEW refresh token (rotation)
    let refresh_token_2 = oauth_service
        .create_refresh_token(&consumed.client_id, consumed.user_id, &consumed.scope)
        .await?;

    // Old token should fail
    let result = oauth_service.consume_refresh_token(&refresh_token_1).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Refresh token already used"));

    // New token should work
    let result2 = oauth_service.consume_refresh_token(&refresh_token_2).await;
    assert!(result2.is_ok());

    Ok(())
}

/// Test expired refresh token rejection
#[tokio::test]
async fn test_expired_refresh_token_rejection() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create expired refresh token
    let token = format!("mcp_refresh_{}", uuid::Uuid::new_v4());
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let token_hash = format!("{:x}", hasher.finalize());

    let now = chrono::Utc::now().timestamp();
    let expired_at = now - 86400; // 1 day ago

    sqlx::query!(
        r#"
        INSERT INTO oauth_refresh_tokens (token_hash, client_id, user_id, scope, expires_at)
        VALUES (?, ?, ?, ?, ?)
        "#,
        token_hash,
        client.client_id,
        user_id,
        "mcp:read",
        expired_at
    )
    .execute(&pool)
    .await?;

    // Try to consume expired token
    let result = oauth_service.consume_refresh_token(&token).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Refresh token expired"));

    Ok(())
}

/// Test used refresh token rejection
#[tokio::test]
async fn test_used_refresh_token_rejection() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create refresh token
    let refresh_token = oauth_service
        .create_refresh_token(&client.client_id, user_id, "mcp:read")
        .await?;

    // First use - should succeed
    let result1 = oauth_service.consume_refresh_token(&refresh_token).await;
    assert!(result1.is_ok());

    // Second use - should fail
    let result2 = oauth_service.consume_refresh_token(&refresh_token).await;
    assert!(result2.is_err());
    assert!(result2
        .unwrap_err()
        .to_string()
        .contains("Refresh token already used"));

    Ok(())
}

/// Test tokens are hashed in database
#[tokio::test]
async fn test_tokens_are_hashed_in_database() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create access token
    let (access_token, _) = oauth_service
        .create_access_token(&client.client_id, user_id, "mcp:read")
        .await?;

    // Verify plaintext token is NOT in database
    let count = sqlx::query!(
        "SELECT COUNT(*) as count FROM oauth_access_tokens WHERE token_hash = ?",
        access_token
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(count.count, 0, "Plaintext token should not be in database");

    // Verify hash IS in database
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(access_token.as_bytes());
    let token_hash = format!("{:x}", hasher.finalize());

    let count2 = sqlx::query!(
        "SELECT COUNT(*) as count FROM oauth_access_tokens WHERE token_hash = ?",
        token_hash
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(count2.count, 1, "Hashed token should be in database");

    Ok(())
}

/// Test different tokens each time (UUID randomness)
#[tokio::test]
async fn test_different_tokens_each_time() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create multiple access tokens
    let (token1, _) = oauth_service
        .create_access_token(&client.client_id, user_id, "mcp:read")
        .await?;
    let (token2, _) = oauth_service
        .create_access_token(&client.client_id, user_id, "mcp:read")
        .await?;

    assert_ne!(token1, token2, "Tokens should be unique");

    // Create multiple refresh tokens
    let refresh1 = oauth_service
        .create_refresh_token(&client.client_id, user_id, "mcp:read")
        .await?;
    let refresh2 = oauth_service
        .create_refresh_token(&client.client_id, user_id, "mcp:read")
        .await?;

    assert_ne!(refresh1, refresh2, "Refresh tokens should be unique");

    Ok(())
}

/// Test scope inheritance through flow
#[tokio::test]
async fn test_scope_inheritance() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create authorization code with specific scope
    let code = oauth_service
        .create_authorization_code(
            &client.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read mcp:write mcp:admin",
            None,
            None,
        )
        .await?;

    // Consume code
    let consumed = oauth_service
        .consume_authorization_code(&code, &client.client_id, "http://localhost:3000/callback")
        .await?;

    // Verify scope is inherited
    assert_eq!(consumed.scope, "mcp:read mcp:write mcp:admin");

    // Create tokens with same scope
    let (access_token, _) = oauth_service
        .create_access_token(&client.client_id, consumed.user_id, &consumed.scope)
        .await?;
    let refresh_token = oauth_service
        .create_refresh_token(&client.client_id, consumed.user_id, &consumed.scope)
        .await?;

    // Verify access token has correct scope
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(access_token.as_bytes());
    let access_hash = format!("{:x}", hasher.finalize());

    let access_row = sqlx::query!(
        "SELECT scope FROM oauth_access_tokens WHERE token_hash = ?",
        access_hash
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(access_row.scope, "mcp:read mcp:write mcp:admin");

    // Verify refresh token has correct scope
    let mut hasher2 = Sha256::new();
    hasher2.update(refresh_token.as_bytes());
    let refresh_hash = format!("{:x}", hasher2.finalize());

    let refresh_row = sqlx::query!(
        "SELECT scope FROM oauth_refresh_tokens WHERE token_hash = ?",
        refresh_hash
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(refresh_row.scope, "mcp:read mcp:write mcp:admin");

    Ok(())
}

/// Test multiple refresh operations
#[tokio::test]
async fn test_multiple_refresh_operations() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    // Create user and client
    let user_id =
        test_helpers::insert_test_user(&pool, "user@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    // Create initial refresh token
    let mut current_refresh = oauth_service
        .create_refresh_token(&client.client_id, user_id, "mcp:read")
        .await?;

    // Perform 3 refresh operations
    for _ in 0..3 {
        // Consume current token
        let consumed = oauth_service
            .consume_refresh_token(&current_refresh)
            .await?;

        // Create new tokens
        let (_access, _) = oauth_service
            .create_access_token(&consumed.client_id, consumed.user_id, &consumed.scope)
            .await?;

        // Create new refresh token (rotation)
        current_refresh = oauth_service
            .create_refresh_token(&consumed.client_id, consumed.user_id, &consumed.scope)
            .await?;
    }

    // Final refresh token should work
    let final_result = oauth_service.consume_refresh_token(&current_refresh).await;
    assert!(final_result.is_ok());

    Ok(())
}
