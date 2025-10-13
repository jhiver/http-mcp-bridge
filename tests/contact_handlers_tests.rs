use saramcp::{
    repositories::user_repository::SqliteUserRepository,
    services::{
        create_email_service,
        user_service::{CreateUserRequest, UserService},
        AuthTokenService,
    },
    test_utils::test_helpers,
};
use std::sync::Arc;

#[tokio::test]
async fn test_send_contact_form_valid_email() {
    // Create isolated test database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let user_service = Arc::new(UserService::new(user_repository.clone()));
    let email_service = create_email_service();

    let auth_token_service =
        AuthTokenService::new(pool, email_service, user_repository, user_service.clone());

    // Test sending contact form through the email service
    let result = auth_token_service
        .email_service()
        .send_contact_form(
            "test@example.com",
            Some("Test User"),
            "This is a test message",
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_send_contact_form_without_name() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let user_service = Arc::new(UserService::new(user_repository.clone()));
    let email_service = create_email_service();

    let auth_token_service =
        AuthTokenService::new(pool, email_service, user_repository, user_service.clone());

    // Test sending contact form without name
    let result = auth_token_service
        .email_service()
        .send_contact_form(
            "test@example.com",
            None,
            "This is a test message without name",
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_send_contact_form_multiline_message() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let user_service = Arc::new(UserService::new(user_repository.clone()));
    let email_service = create_email_service();

    let auth_token_service =
        AuthTokenService::new(pool, email_service, user_repository, user_service.clone());

    // Test with multiline message
    let multiline_message = "This is line 1\nThis is line 2\nThis is line 3";
    let result = auth_token_service
        .email_service()
        .send_contact_form("test@example.com", Some("Test User"), multiline_message)
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_authenticated_user_context() {
    // Create isolated test database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let user_service = Arc::new(UserService::new(user_repository.clone()));

    // Create a test user
    let create_request = CreateUserRequest {
        email: "contacttest@example.com".to_string(),
        password: "password123".to_string(),
        password_confirm: None,
        email_verified: true,
    };

    let created_user = user_service.create_user(create_request).await.unwrap();

    // Verify user exists and has correct email
    assert_eq!(created_user.email, "contacttest@example.com");

    // In a real handler, this user would be loaded from session
    // and their email would be pre-filled in the contact form
}

#[tokio::test]
async fn test_send_contact_form_special_characters() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let user_service = Arc::new(UserService::new(user_repository.clone()));
    let email_service = create_email_service();

    let auth_token_service =
        AuthTokenService::new(pool, email_service, user_repository, user_service.clone());

    // Test with special characters
    let message_with_special_chars = "Test with special chars: <>&\"'";
    let result = auth_token_service
        .email_service()
        .send_contact_form(
            "test@example.com",
            Some("Test User"),
            message_with_special_chars,
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_send_contact_form_long_message() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let user_service = Arc::new(UserService::new(user_repository.clone()));
    let email_service = create_email_service();

    let auth_token_service =
        AuthTokenService::new(pool, email_service, user_repository, user_service.clone());

    // Test with a very long message
    let long_message = "A".repeat(5000);
    let result = auth_token_service
        .email_service()
        .send_contact_form("test@example.com", Some("Test User"), &long_message)
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_send_contact_form_email_with_plus() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let user_service = Arc::new(UserService::new(user_repository.clone()));
    let email_service = create_email_service();

    let auth_token_service =
        AuthTokenService::new(pool, email_service, user_repository, user_service.clone());

    // Test with email containing + sign (like jhiver+test@gmail.com)
    let result = auth_token_service
        .email_service()
        .send_contact_form(
            "jhiver+test@gmail.com",
            Some("Test User"),
            "Testing email with plus sign",
        )
        .await;

    assert!(result.is_ok());
}
