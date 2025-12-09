use axum::{
    body::{to_bytes, Body},
    extract::{Form, State},
    http::{header, HeaderMap, Request, StatusCode},
    middleware,
    routing::{get, post},
    Router,
};
use saramcp::{
    handlers,
    handlers::oauth_handlers::{OAuthError, TokenRequest},
    middleware as saramcp_middleware,
    repositories::SqliteUserRepository,
    services::{AuthService, AuthTokenService, OAuthService, UserService},
    test_utils::test_helpers,
    AppState,
};
use sqlx::SqlitePool;
use std::sync::Arc;
use tower::ServiceExt;

async fn build_app_state(pool: &SqlitePool) -> AppState {
    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let user_service = Arc::new(UserService::new(user_repository.clone()));
    let auth_service = Arc::new(AuthService::new(user_repository.clone()));
    let email_service = saramcp::services::create_email_service();
    let auth_token_service = Arc::new(AuthTokenService::new(
        pool.clone(),
        email_service,
        user_repository.clone(),
        user_service.clone(),
    ));

    AppState {
        user_service,
        auth_service,
        auth_token_service,
        toolkit_service: None,
        tool_service: None,
        server_service: None,
        instance_service: None,
        oauth_service: Arc::new(OAuthService::new(pool.clone())),
        toolkit_repository: None,
        tool_repository: None,
        mcp_registry: None,
        pool: pool.clone(),
    }
}

#[tokio::test]
async fn authorization_metadata_includes_client_secret_post() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let state = build_app_state(&pool).await;

    let app = Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(handlers::authorization_server_metadata),
        )
        .with_state(state);

    let request = Request::builder()
        .method("GET")
        .uri("/.well-known/oauth-authorization-server")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let metadata: serde_json::Value = serde_json::from_slice(&body)?;
    let methods = metadata["token_endpoint_auth_methods_supported"]
        .as_array()
        .expect("expected auth methods array");

    assert!(
        methods
            .iter()
            .any(|value| value.as_str() == Some("client_secret_post")),
        "metadata should advertise client_secret_post: {:?}",
        methods
    );

    Ok(())
}

#[tokio::test]
async fn token_endpoint_rejects_missing_client_secret() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let state = build_app_state(&pool).await;

    let user_id =
        test_helpers::insert_test_user(&pool, "owner@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = state
        .oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    let code = state
        .oauth_service
        .create_authorization_code(
            &client.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await?;

    let headers = HeaderMap::new();
    let request = TokenRequest {
        grant_type: "authorization_code".to_string(),
        code: Some(code),
        redirect_uri: Some("http://localhost:3000/callback".to_string()),
        code_verifier: None,
        refresh_token: None,
        client_id: Some(client.client_id.clone()),
        client_secret: None,
    };

    let result = handlers::token(State(state.clone()), headers, Form(request)).await;

    match result {
        Err(OAuthError::InvalidClient(message)) => {
            assert!(
                message.contains("Client authentication required"),
                "unexpected error: {}",
                message
            );
        }
        other => panic!("expected InvalidClient error, got {:?}", other),
    }

    Ok(())
}

#[tokio::test]
async fn token_endpoint_accepts_client_secret_post() -> anyhow::Result<()> {
    let pool = test_helpers::create_test_db().await?;
    let state = build_app_state(&pool).await;

    let user_id =
        test_helpers::insert_test_user(&pool, "owner@example.com", "password", true).await?;
    let registration = saramcp::services::ClientRegistrationRequest {
        client_name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
    };
    let client = state
        .oauth_service
        .register_client(Some(user_id), registration)
        .await?;

    let code = state
        .oauth_service
        .create_authorization_code(
            &client.client_id,
            user_id,
            "http://localhost:3000/callback",
            "mcp:read",
            None,
            None,
        )
        .await?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "application/x-www-form-urlencoded".parse().unwrap(),
    );

    let request = TokenRequest {
        grant_type: "authorization_code".to_string(),
        code: Some(code),
        redirect_uri: Some("http://localhost:3000/callback".to_string()),
        code_verifier: None,
        refresh_token: None,
        client_id: Some(client.client_id.clone()),
        client_secret: Some(client.client_secret.clone()),
    };

    let response = handlers::token(State(state.clone()), headers, Form(request))
        .await
        .expect("token endpoint should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let payload: serde_json::Value = serde_json::from_slice(&body)?;
    assert!(
        payload.get("access_token").is_some(),
        "response should include access_token"
    );

    Ok(())
}

#[tokio::test]
async fn http_transport_sets_www_authenticate_header() -> anyhow::Result<()> {
    std::env::set_var("BASE_URL", "http://localhost:8080");
    let pool = test_helpers::create_test_db().await?;
    let state = build_app_state(&pool).await;

    let user_id =
        test_helpers::insert_test_user(&pool, "owner@example.com", "password", true).await?;
    let (server_id, server_uuid) =
        test_helpers::create_test_server(&pool, user_id, "Secured Server", None).await?;

    sqlx::query!(
        "UPDATE servers SET access_level = 'organization' WHERE id = ?",
        server_id
    )
    .execute(&pool)
    .await?;

    let app = Router::new()
        .route("/s/{uuid}", post(|| async { StatusCode::OK }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            saramcp_middleware::mcp_auth_middleware,
        ))
        .with_state(state.clone());

    let request = Request::builder()
        .method("POST")
        .uri(format!("/s/{}", server_uuid))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let header_value = response
        .headers()
        .get(header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .expect("WWW-Authenticate header missing");

    assert!(
        header_value.contains(&format!(
            "resource=\"http://localhost:8080/s/{}\"",
            server_uuid
        )),
        "header missing resource description: {}",
        header_value
    );
    assert!(
        header_value.contains(&format!(
            "resource_metadata=\"http://localhost:8080/.well-known/oauth-protected-resource/s/{}\"",
            server_uuid
        )),
        "header missing resource metadata: {}",
        header_value
    );

    Ok(())
}
