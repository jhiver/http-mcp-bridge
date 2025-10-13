use saramcp::{
    repositories::user_repository::SqliteUserRepository,
    services::user_service::{CreateUserRequest, UpdatePasswordRequest, UserService},
    test_utils::test_helpers,
};
use std::sync::Arc;

#[tokio::test]
async fn test_create_user_success() {
    // Create isolated test database
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool));
    let service = UserService::new(repository);

    let request = CreateUserRequest {
        email: "test@example.com".to_string(),
        password: "password123".to_string(),
        password_confirm: Some("password123".to_string()),
        email_verified: false,
    };

    let result = service.create_user(request).await;
    assert!(result.is_ok());

    let user = result.unwrap();
    assert_eq!(user.email, "test@example.com");
    assert!(!user.email_verified);
}

#[tokio::test]
async fn test_create_user_duplicate_email() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let service = UserService::new(repository);

    // Create first user
    let request1 = CreateUserRequest {
        email: "duplicate@example.com".to_string(),
        password: "password123".to_string(),
        password_confirm: None,
        email_verified: false,
    };

    let result1 = service.create_user(request1).await;
    assert!(result1.is_ok());

    // Try to create second user with same email
    let request2 = CreateUserRequest {
        email: "duplicate@example.com".to_string(),
        password: "password456".to_string(),
        password_confirm: None,
        email_verified: false,
    };

    let result2 = service.create_user(request2).await;
    assert!(result2.is_err());
    assert!(matches!(
        result2.unwrap_err(),
        saramcp::services::user_service::UserServiceError::EmailTaken
    ));
}

#[tokio::test]
async fn test_update_password() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let service = UserService::new(repository);

    // Create user
    let create_request = CreateUserRequest {
        email: "password_test@example.com".to_string(),
        password: "oldpassword123".to_string(),
        password_confirm: None,
        email_verified: false,
    };

    let user = service.create_user(create_request).await.unwrap();

    // Update password
    let update_request = UpdatePasswordRequest {
        user_id: user.id,
        new_password: "newpassword456".to_string(),
        new_password_confirm: Some("newpassword456".to_string()),
    };

    let result = service.update_password(update_request).await;
    assert!(result.is_ok());

    // Fetch updated user and verify new password works
    let updated_user = service.find_user_by_id(user.id).await.unwrap().unwrap();
    assert!(service.verify_password("newpassword456", &updated_user.password_hash));
}

#[tokio::test]
async fn test_list_users() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let service = UserService::new(repository);

    // Create multiple users
    for i in 0..5 {
        let request = CreateUserRequest {
            email: format!("user{}@example.com", i),
            password: "password123".to_string(),
            password_confirm: None,
            email_verified: false,
        };
        service.create_user(request).await.unwrap();
    }

    // List all users
    let users = service.list_users(None, None).await.unwrap();
    assert_eq!(users.len(), 5);

    // List with limit
    let limited_users = service.list_users(Some(3), None).await.unwrap();
    assert_eq!(limited_users.len(), 3);

    // List with offset
    let offset_users = service.list_users(Some(10), Some(2)).await.unwrap();
    assert_eq!(offset_users.len(), 3);
}

#[tokio::test]
async fn test_delete_user() {
    let pool = test_helpers::create_test_db().await.unwrap();
    let repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let service = UserService::new(repository);

    // Create user
    let request = CreateUserRequest {
        email: "delete_me@example.com".to_string(),
        password: "password123".to_string(),
        password_confirm: None,
        email_verified: false,
    };

    let user = service.create_user(request).await.unwrap();

    // Delete user
    let result = service.delete_user(user.id).await;
    assert!(result.is_ok());

    // Verify user is deleted
    let find_result = service.find_user_by_id(user.id).await.unwrap();
    assert!(find_result.is_none());
}
