use saramcp::{
    repositories::user_repository::SqliteUserRepository,
    services::{
        auth_service::{AuthService, LoginRequest},
        user_service::{CreateUserRequest, UserService},
    },
    test_utils::test_helpers,
};
use std::sync::Arc;

#[tokio::test]
async fn test_authenticate_success() {
    // Create isolated test database
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool));
    let user_service = UserService::new(repository.clone());
    let auth_service = AuthService::new(repository);

    // Create a test user
    let create_request = CreateUserRequest {
        email: "auth@example.com".to_string(),
        password: "correctpassword".to_string(),
        password_confirm: None,
        email_verified: true,
    };

    let created_user = user_service.create_user(create_request).await.unwrap();

    // Try to authenticate with correct credentials
    let login_request = LoginRequest {
        email: "auth@example.com".to_string(),
        password: "correctpassword".to_string(),
    };

    let result = auth_service.authenticate(login_request).await;
    assert!(result.is_ok());

    let authenticated_user = result.unwrap();
    assert_eq!(authenticated_user.id, created_user.id);
    assert_eq!(authenticated_user.email, "auth@example.com");
}

#[tokio::test]
async fn test_authenticate_wrong_password() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool));
    let user_service = UserService::new(repository.clone());
    let auth_service = AuthService::new(repository);

    // Create a test user
    let create_request = CreateUserRequest {
        email: "wrongpass@example.com".to_string(),
        password: "correctpassword".to_string(),
        password_confirm: None,
        email_verified: true,
    };

    user_service.create_user(create_request).await.unwrap();

    // Try to authenticate with wrong password
    let login_request = LoginRequest {
        email: "wrongpass@example.com".to_string(),
        password: "wrongpassword".to_string(),
    };

    let result = auth_service.authenticate(login_request).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        saramcp::services::auth_service::AuthServiceError::InvalidCredentials
    ));
}

#[tokio::test]
async fn test_authenticate_nonexistent_user() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool));
    let auth_service = AuthService::new(repository);

    // Try to authenticate with non-existent email
    let login_request = LoginRequest {
        email: "nonexistent@example.com".to_string(),
        password: "anypassword".to_string(),
    };

    let result = auth_service.authenticate(login_request).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        saramcp::services::auth_service::AuthServiceError::InvalidCredentials
    ));
}

#[tokio::test]
async fn test_get_user_by_id() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool));
    let user_service = UserService::new(repository.clone());
    let auth_service = AuthService::new(repository);

    // Create a test user
    let create_request = CreateUserRequest {
        email: "byid@example.com".to_string(),
        password: "password123".to_string(),
        password_confirm: None,
        email_verified: false,
    };

    let created_user = user_service.create_user(create_request).await.unwrap();

    // Get user by ID
    let result = auth_service.get_user_by_id(created_user.id).await;
    assert!(result.is_ok());

    let found_user = result.unwrap();
    assert_eq!(found_user.id, created_user.id);
    assert_eq!(found_user.email, "byid@example.com");
}

#[tokio::test]
async fn test_get_user_by_id_not_found() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool));
    let auth_service = AuthService::new(repository);

    // Try to get non-existent user
    let result = auth_service.get_user_by_id(9999).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        saramcp::services::auth_service::AuthServiceError::UserNotFound
    ));
}
