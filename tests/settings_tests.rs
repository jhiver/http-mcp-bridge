use saramcp::{
    repositories::user_repository::SqliteUserRepository,
    services::user_service::{
        CreateUserRequest, UpdateEmailRequest, UpdatePasswordRequest, UserService,
    },
    test_utils::test_helpers,
};
use std::sync::Arc;

#[tokio::test]
async fn test_update_email_success() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let service = UserService::new(repository);

    // Create user
    let create_request = CreateUserRequest {
        email: "old@example.com".to_string(),
        password: "password123".to_string(),
        password_confirm: None,
        email_verified: false,
    };

    let user = service.create_user(create_request).await.unwrap();

    // Update email
    let update_request = UpdateEmailRequest {
        user_id: user.id,
        new_email: "new@example.com".to_string(),
    };

    let result = service.update_email(update_request).await;
    assert!(result.is_ok());

    // Verify email was updated
    let updated_user = service.find_user_by_id(user.id).await.unwrap().unwrap();
    assert_eq!(updated_user.email, "new@example.com");
}

#[tokio::test]
async fn test_update_email_duplicate() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let service = UserService::new(repository);

    // Create first user
    let request1 = CreateUserRequest {
        email: "user1@example.com".to_string(),
        password: "password123".to_string(),
        password_confirm: None,
        email_verified: false,
    };
    service.create_user(request1).await.unwrap();

    // Create second user
    let request2 = CreateUserRequest {
        email: "user2@example.com".to_string(),
        password: "password123".to_string(),
        password_confirm: None,
        email_verified: false,
    };
    let user2 = service.create_user(request2).await.unwrap();

    // Try to update user2's email to user1's email
    let update_request = UpdateEmailRequest {
        user_id: user2.id,
        new_email: "user1@example.com".to_string(),
    };

    let result = service.update_email(update_request).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        saramcp::services::user_service::UserServiceError::EmailTaken
    ));
}

#[tokio::test]
async fn test_update_email_invalid_format() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let service = UserService::new(repository);

    // Create user
    let create_request = CreateUserRequest {
        email: "valid@example.com".to_string(),
        password: "password123".to_string(),
        password_confirm: None,
        email_verified: false,
    };
    let user = service.create_user(create_request).await.unwrap();

    // Try to update to invalid email
    let update_request = UpdateEmailRequest {
        user_id: user.id,
        new_email: "not-an-email".to_string(),
    };

    let result = service.update_email(update_request).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        saramcp::services::user_service::UserServiceError::InvalidEmail
    ));
}

#[tokio::test]
async fn test_update_email_empty() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let service = UserService::new(repository);

    // Create user
    let create_request = CreateUserRequest {
        email: "valid@example.com".to_string(),
        password: "password123".to_string(),
        password_confirm: None,
        email_verified: false,
    };
    let user = service.create_user(create_request).await.unwrap();

    // Try to update to empty email
    let update_request = UpdateEmailRequest {
        user_id: user.id,
        new_email: "".to_string(),
    };

    let result = service.update_email(update_request).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        saramcp::services::user_service::UserServiceError::InvalidEmail
    ));
}

#[tokio::test]
async fn test_update_email_nonexistent_user() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let service = UserService::new(repository);

    // Try to update email for nonexistent user
    let update_request = UpdateEmailRequest {
        user_id: 99999,
        new_email: "new@example.com".to_string(),
    };

    let result = service.update_email(update_request).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_update_password_with_confirmation() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let service = UserService::new(repository);

    // Create user
    let create_request = CreateUserRequest {
        email: "password_update@example.com".to_string(),
        password: "oldpassword123".to_string(),
        password_confirm: None,
        email_verified: false,
    };
    let user = service.create_user(create_request).await.unwrap();

    // Update password with confirmation
    let update_request = UpdatePasswordRequest {
        user_id: user.id,
        new_password: "newpassword456".to_string(),
        new_password_confirm: Some("newpassword456".to_string()),
    };

    let result = service.update_password(update_request).await;
    assert!(result.is_ok());

    // Verify new password works
    let updated_user = service.find_user_by_id(user.id).await.unwrap().unwrap();
    assert!(service.verify_password("newpassword456", &updated_user.password_hash));
    assert!(!service.verify_password("oldpassword123", &updated_user.password_hash));
}

#[tokio::test]
async fn test_update_password_mismatch() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let service = UserService::new(repository);

    // Create user
    let create_request = CreateUserRequest {
        email: "mismatch@example.com".to_string(),
        password: "password123".to_string(),
        password_confirm: None,
        email_verified: false,
    };
    let user = service.create_user(create_request).await.unwrap();

    // Try to update with mismatched passwords
    let update_request = UpdatePasswordRequest {
        user_id: user.id,
        new_password: "newpassword456".to_string(),
        new_password_confirm: Some("differentpassword".to_string()),
    };

    let result = service.update_password(update_request).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        saramcp::services::user_service::UserServiceError::PasswordMismatch
    ));
}
