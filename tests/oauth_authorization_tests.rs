use saramcp::{
    models::oauth::OAuthAuthorizationCode,
    services::{oauth_service::parse_scopes, ClientRegistrationRequest, OAuthService},
    test_utils::test_helpers,
};

// ============================================================================
// OAuth Authorization Code Tests
// ============================================================================

#[tokio::test]
async fn test_create_authorization_code_success() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // Register a client first
    let request = ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let client_response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Create authorization code
    let code = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await
        .unwrap();

    // Verify code format
    assert!(code.starts_with("code_"));

    // Verify code is stored in database
    let stored_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(stored_code.client_id, client_response.client_id);
    assert_eq!(stored_code.user_id, user_id);
    assert_eq!(stored_code.redirect_uri, "http://localhost:3000/callback");
    assert_eq!(stored_code.scope, "mcp:read");
    assert!(!stored_code.is_used());
    assert!(!stored_code.is_expired());
}

#[tokio::test]
async fn test_authorization_code_with_pkce_challenge() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // Register client
    let request = ClientRegistrationRequest {
        client_name: "PKCE Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let client_response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Create authorization code with PKCE
    let code = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read mcp:write",
            Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"),
            Some("S256"),
        )
        .await
        .unwrap();

    // Verify PKCE stored correctly
    let stored_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        stored_code.code_challenge,
        Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string())
    );
    assert_eq!(stored_code.code_challenge_method, Some("S256".to_string()));
}

#[tokio::test]
async fn test_authorization_code_with_plain_pkce() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // Register client
    let request = ClientRegistrationRequest {
        client_name: "Plain PKCE Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let client_response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Create authorization code with plain PKCE
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let code = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            Some(verifier),
            Some("plain"),
        )
        .await
        .unwrap();

    // Verify plain PKCE stored correctly
    let stored_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(stored_code.code_challenge, Some(verifier.to_string()));
    assert_eq!(stored_code.code_challenge_method, Some("plain".to_string()));
}

#[tokio::test]
async fn test_authorization_code_expires_after_10_minutes() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // Register client
    let request = ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let client_response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Create authorization code
    let code = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await
        .unwrap();

    let stored_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();

    // Verify expires_at is set (Unix timestamp > 0)
    assert!(stored_code.expires_at > 0);

    // Verify expiry is approximately 10 minutes from now
    let now = chrono::Utc::now().timestamp();
    let diff_seconds = stored_code.expires_at - now;

    // Should be between 9 and 11 minutes (allowing for test execution time)
    // 540 seconds = 9 minutes, 660 seconds = 11 minutes
    assert!((540..=660).contains(&diff_seconds));
}

#[tokio::test]
async fn test_authorization_code_can_be_marked_as_used() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // Register client
    let request = ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let client_response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Create authorization code
    let code = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await
        .unwrap();

    // Verify not used initially
    let stored_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();
    assert!(!stored_code.is_used());

    // Mark as used (instance method)
    stored_code.mark_used(&pool).await.unwrap();

    // Verify now marked as used
    let used_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();
    assert!(used_code.is_used());
}

#[tokio::test]
async fn test_authorization_code_detects_expiration() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // Register client
    let request = ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let client_response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Create an expired authorization code (expires_at in the past)
    let expired_time = (chrono::Utc::now() - chrono::Duration::minutes(5)).timestamp();

    sqlx::query!(
        r#"
        INSERT INTO oauth_authorization_codes
        (code, client_id, user_id, redirect_uri, scope, expires_at)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
        "code_expired",
        client_response.client_id,
        user_id,
        "http://localhost:3000/callback",
        "mcp:read",
        expired_time
    )
    .execute(&pool)
    .await
    .unwrap();

    // Retrieve and check expiration
    let code = OAuthAuthorizationCode::get_by_code(&pool, "code_expired")
        .await
        .unwrap()
        .unwrap();

    assert!(code.is_expired());
}

#[tokio::test]
async fn test_authorization_code_with_multiple_scopes() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // Register client
    let request = ClientRegistrationRequest {
        client_name: "Multi-Scope Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let client_response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Create authorization code with multiple scopes
    let scopes = "mcp:read mcp:write mcp:admin";
    let code = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user_id,
            "http://localhost:3000/callback",
            scopes,
            None,
            None,
        )
        .await
        .unwrap();

    // Verify scopes stored correctly
    let stored_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(stored_code.scope, scopes);
}

#[tokio::test]
async fn test_get_client_by_client_id() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // Register client
    let request = ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let client_response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Retrieve client by client_id
    let client = oauth_service
        .get_client(&client_response.client_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(client.client_id, client_response.client_id);
    assert_eq!(client.name, "Test Client");
}

#[tokio::test]
async fn test_get_client_returns_none_for_unknown_id() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let oauth_service = OAuthService::new(pool.clone());

    // Try to get non-existent client
    let result = oauth_service.get_client("mcp_nonexistent").await.unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn test_authorization_code_unique_per_creation() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // Register client
    let request = ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let client_response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Create two authorization codes
    let code1 = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await
        .unwrap();

    let code2 = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await
        .unwrap();

    // Codes should be unique
    assert_ne!(code1, code2);
}

// ============================================================================
// Scope Parsing Tests
// ============================================================================

#[test]
fn test_parse_scopes_single_scope() {
    let scopes = parse_scopes("mcp:read");
    assert_eq!(scopes, vec!["mcp:read"]);
}

#[test]
fn test_parse_scopes_multiple_scopes() {
    let scopes = parse_scopes("mcp:read mcp:write mcp:admin");
    assert_eq!(scopes, vec!["mcp:read", "mcp:write", "mcp:admin"]);
}

#[test]
fn test_parse_scopes_with_extra_whitespace() {
    let scopes = parse_scopes("  mcp:read   mcp:write  ");
    assert_eq!(scopes, vec!["mcp:read", "mcp:write"]);
}

#[test]
fn test_parse_scopes_empty_string() {
    let scopes = parse_scopes("");
    assert_eq!(scopes, Vec::<String>::new());
}

#[test]
fn test_parse_scopes_mixed_separators() {
    // Should handle multiple spaces, tabs, etc.
    let scopes = parse_scopes("mcp:read\tmcp:write\t\tmcp:admin");
    assert_eq!(scopes, vec!["mcp:read", "mcp:write", "mcp:admin"]);
}

#[test]
fn test_parse_scopes_preserves_order() {
    let scopes = parse_scopes("admin write read");
    assert_eq!(scopes, vec!["admin", "write", "read"]);
}

// ============================================================================
// Authorization Code Lifecycle Tests
// ============================================================================

#[tokio::test]
async fn test_authorization_code_full_lifecycle() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // 1. Register client
    let request = ClientRegistrationRequest {
        client_name: "Lifecycle Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let client_response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // 2. Create authorization code
    let code = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read mcp:write",
            Some("challenge123"),
            Some("S256"),
        )
        .await
        .unwrap();

    // 3. Retrieve code (simulating authorization check)
    let auth_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();

    assert!(!auth_code.is_used());
    assert!(!auth_code.is_expired());
    assert_eq!(auth_code.client_id, client_response.client_id);
    assert_eq!(auth_code.scope, "mcp:read mcp:write");

    // 4. Mark as used (simulating token exchange - instance method)
    auth_code.mark_used(&pool).await.unwrap();

    // 5. Verify cannot be reused
    let used_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();

    assert!(used_code.is_used());
}

#[tokio::test]
async fn test_authorization_code_different_redirect_uris() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // Register client with multiple redirect URIs
    let request = ClientRegistrationRequest {
        client_name: "Multi-URI Client".to_string(),
        redirect_uris: vec![
            "http://localhost:3000/callback".to_string(),
            "http://localhost:8080/oauth/callback".to_string(),
        ],
    };

    let client_response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Create codes for different redirect URIs
    let code1 = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await
        .unwrap();

    let code2 = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user_id,
            "http://localhost:8080/oauth/callback",
            "mcp:read",
            None,
            None,
        )
        .await
        .unwrap();

    // Verify each code has correct redirect_uri
    let stored_code1 = OAuthAuthorizationCode::get_by_code(&pool, &code1)
        .await
        .unwrap()
        .unwrap();

    let stored_code2 = OAuthAuthorizationCode::get_by_code(&pool, &code2)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(stored_code1.redirect_uri, "http://localhost:3000/callback");
    assert_eq!(
        stored_code2.redirect_uri,
        "http://localhost:8080/oauth/callback"
    );
}

#[tokio::test]
async fn test_authorization_code_for_different_users() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create two users
    let user1_id = test_helpers::insert_test_user(&pool, "user1@example.com", "password", true)
        .await
        .unwrap();

    let user2_id = test_helpers::insert_test_user(&pool, "user2@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // Register client for user1
    let request = ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let client_response = oauth_service
        .register_client(Some(user1_id), request)
        .await
        .unwrap();

    // Create authorization codes for different users
    let code1 = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user1_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await
        .unwrap();

    let code2 = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user2_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await
        .unwrap();

    // Verify user association
    let stored_code1 = OAuthAuthorizationCode::get_by_code(&pool, &code1)
        .await
        .unwrap()
        .unwrap();

    let stored_code2 = OAuthAuthorizationCode::get_by_code(&pool, &code2)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(stored_code1.user_id, user1_id);
    assert_eq!(stored_code2.user_id, user2_id);
}

#[tokio::test]
async fn test_authorization_code_default_scope_applied() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // Register client
    let request = ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let client_response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Create authorization code with default scope (mcp:read is typical default)
    let code = oauth_service
        .create_authorization_code(
            &client_response.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read", // Explicitly using the default
            None,
            None,
        )
        .await
        .unwrap();

    let stored_code = OAuthAuthorizationCode::get_by_code(&pool, &code)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(stored_code.scope, "mcp:read");
}
