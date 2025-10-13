use saramcp::{
    models::oauth::OAuthClient,
    services::{ClientRegistrationRequest, OAuthService},
    test_utils::test_helpers,
};

// ============================================================================
// OAuth Client Registration Service Tests
// ============================================================================

#[tokio::test]
async fn test_register_client_success() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "My Test Application".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Verify response structure
    assert!(response.client_id.starts_with("mcp_"));
    assert!(!response.client_secret.is_empty());
    assert_eq!(response.client_name, "My Test Application");
    assert_eq!(response.redirect_uris.len(), 1);
    assert_eq!(response.redirect_uris[0], "http://localhost:3000/callback");
    assert!(response.client_id_issued_at > 0);
    assert_eq!(response.client_secret_expires_at, 0); // Never expires
}

#[tokio::test]
async fn test_register_client_secret_is_hashed_in_database() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "My Test Application".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Verify client exists in database with hashed secret
    let client = OAuthClient::get_by_client_id(&pool, &response.client_id)
        .await
        .unwrap()
        .unwrap();

    // Secret in database should NOT match the plaintext secret returned
    assert_ne!(client.client_secret_hash, response.client_secret);

    // Should be SHA-256 hex (64 characters)
    assert_eq!(client.client_secret_hash.len(), 64);

    // But verify_secret should work with the original secret
    assert!(client.verify_secret(&response.client_secret));
}

#[tokio::test]
async fn test_register_client_with_multiple_redirect_uris() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "Multi-URI App".to_string(),
        redirect_uris: vec![
            "http://localhost:3000/callback".to_string(),
            "http://localhost:8080/oauth/callback".to_string(),
            "https://app.example.com/callback".to_string(),
        ],
    };

    let response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    assert_eq!(response.redirect_uris.len(), 3);

    // Verify stored in database correctly
    let client = OAuthClient::get_by_client_id(&pool, &response.client_id)
        .await
        .unwrap()
        .unwrap();

    let stored_uris: Vec<String> = serde_json::from_str(&client.redirect_uris).unwrap();
    assert_eq!(stored_uris.len(), 3);
    assert!(stored_uris.contains(&"http://localhost:3000/callback".to_string()));
    assert!(stored_uris.contains(&"http://localhost:8080/oauth/callback".to_string()));
    assert!(stored_uris.contains(&"https://app.example.com/callback".to_string()));
}

#[tokio::test]
async fn test_register_client_empty_name_fails() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "".to_string(), // Empty name
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let result = oauth_service.register_client(Some(user_id), request).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("client_name"));
}

#[tokio::test]
async fn test_register_client_whitespace_only_name_fails() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "   ".to_string(), // Whitespace only
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let result = oauth_service.register_client(Some(user_id), request).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("client_name"));
}

#[tokio::test]
async fn test_register_client_no_redirect_uris_fails() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "Test App".to_string(),
        redirect_uris: vec![], // Empty redirect URIs
    };

    let result = oauth_service.register_client(Some(user_id), request).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("redirect_uri"));
}

#[tokio::test]
async fn test_register_client_invalid_redirect_uri_fails() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "Test App".to_string(),
        redirect_uris: vec!["not-a-valid-uri".to_string()],
    };

    let result = oauth_service.register_client(Some(user_id), request).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("URI"));
}

#[tokio::test]
async fn test_register_client_javascript_uri_rejected() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "Test App".to_string(),
        redirect_uris: vec!["javascript:alert('xss')".to_string()],
    };

    let result = oauth_service.register_client(Some(user_id), request).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("scheme"));
}

#[tokio::test]
async fn test_register_client_data_uri_rejected() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "Test App".to_string(),
        redirect_uris: vec!["data:text/html,<script>alert('xss')</script>".to_string()],
    };

    let result = oauth_service.register_client(Some(user_id), request).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("scheme"));
}

#[tokio::test]
async fn test_register_client_empty_redirect_uri_fails() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "Test App".to_string(),
        redirect_uris: vec!["".to_string()],
    };

    let result = oauth_service.register_client(Some(user_id), request).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("redirect_uri"));
}

#[tokio::test]
async fn test_register_client_http_uri_allowed() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "Test App".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let result = oauth_service.register_client(Some(user_id), request).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_register_client_https_uri_allowed() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "Test App".to_string(),
        redirect_uris: vec!["https://app.example.com/callback".to_string()],
    };

    let result = oauth_service.register_client(Some(user_id), request).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_register_client_custom_scheme_allowed() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    // Custom URI schemes for native apps (e.g., com.example.app://)
    let request = ClientRegistrationRequest {
        client_name: "Native App".to_string(),
        redirect_uris: vec!["com.example.app://callback".to_string()],
    };

    let result = oauth_service.register_client(Some(user_id), request).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_register_client_unique_client_ids() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request1 = ClientRegistrationRequest {
        client_name: "App 1".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let request2 = ClientRegistrationRequest {
        client_name: "App 2".to_string(),
        redirect_uris: vec!["http://localhost:8080/callback".to_string()],
    };

    let response1 = oauth_service
        .register_client(Some(user_id), request1)
        .await
        .unwrap();

    let response2 = oauth_service
        .register_client(Some(user_id), request2)
        .await
        .unwrap();

    // Client IDs must be unique
    assert_ne!(response1.client_id, response2.client_id);

    // Client secrets must be unique
    assert_ne!(response1.client_secret, response2.client_secret);
}

#[tokio::test]
async fn test_register_client_stores_user_association() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "Test App".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    // Verify client is associated with correct user
    let client = OAuthClient::get_by_client_id(&pool, &response.client_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(client.user_id, Some(user_id));
}

#[tokio::test]
async fn test_register_client_multiple_users_isolated() {
    let pool = test_helpers::create_test_db().await.unwrap();

    let user1_id = test_helpers::insert_test_user(&pool, "user1@example.com", "password", true)
        .await
        .unwrap();

    let user2_id = test_helpers::insert_test_user(&pool, "user2@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let request = ClientRegistrationRequest {
        client_name: "Test App".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    // User 1 registers a client
    let response1 = oauth_service
        .register_client(Some(user1_id), request.clone())
        .await
        .unwrap();

    // User 2 registers a client with same name
    let response2 = oauth_service
        .register_client(Some(user2_id), request.clone())
        .await
        .unwrap();

    // Both should succeed with different client_ids
    assert_ne!(response1.client_id, response2.client_id);

    // Verify user 1's client
    let client1 = OAuthClient::get_by_client_id(&pool, &response1.client_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(client1.user_id, Some(user1_id));

    // Verify user 2's client
    let client2 = OAuthClient::get_by_client_id(&pool, &response2.client_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(client2.user_id, Some(user2_id));

    // List clients - each user should only see their own
    let user1_clients = OAuthClient::list_by_user(&pool, user1_id).await.unwrap();
    let user2_clients = OAuthClient::list_by_user(&pool, user2_id).await.unwrap();

    assert_eq!(user1_clients.len(), 1);
    assert_eq!(user2_clients.len(), 1);
    assert_eq!(user1_clients[0].client_id, response1.client_id);
    assert_eq!(user2_clients[0].client_id, response2.client_id);
}

#[tokio::test]
async fn test_register_client_timestamp_accuracy() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let oauth_service = OAuthService::new(pool.clone());

    let before = chrono::Utc::now().timestamp();

    let request = ClientRegistrationRequest {
        client_name: "Test App".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };

    let response = oauth_service
        .register_client(Some(user_id), request)
        .await
        .unwrap();

    let after = chrono::Utc::now().timestamp();

    // client_id_issued_at should be within the test execution time
    assert!(response.client_id_issued_at >= before);
    assert!(response.client_id_issued_at <= after);
}
