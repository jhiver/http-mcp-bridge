use crate::models::user::User;
use crate::repositories::user_repository::UserRepository;
use argon2::{password_hash::PasswordHash, Argon2, PasswordVerifier};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum AuthServiceError {
    #[error("Invalid credentials")]
    InvalidCredentials,
    #[error("Account not verified")]
    EmailNotVerified,
    #[error("User not found")]
    UserNotFound,
    #[error("Session error: {0}")]
    SessionError(String),
    #[error("Repository error: {0}")]
    RepositoryError(#[from] crate::repositories::user_repository::RepositoryError),
}

pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

pub struct AuthService {
    user_repository: Arc<dyn UserRepository>,
}

impl AuthService {
    pub fn new(user_repository: Arc<dyn UserRepository>) -> Self {
        Self { user_repository }
    }

    pub async fn authenticate(&self, request: LoginRequest) -> Result<User, AuthServiceError> {
        // Find user by email
        let user = self
            .user_repository
            .find_by_email(&request.email)
            .await?
            .ok_or(AuthServiceError::InvalidCredentials)?;

        // Verify password
        if !self.verify_password(&request.password, &user.password_hash) {
            return Err(AuthServiceError::InvalidCredentials);
        }

        // Check if email is verified (optional, depending on requirements)
        // if !user.email_verified {
        //     return Err(AuthServiceError::EmailNotVerified);
        // }

        Ok(user)
    }

    pub async fn get_user_by_id(&self, user_id: i64) -> Result<User, AuthServiceError> {
        self.user_repository
            .find_by_id(user_id)
            .await?
            .ok_or(AuthServiceError::UserNotFound)
    }

    fn verify_password(&self, password: &str, password_hash: &str) -> bool {
        if let Ok(parsed_hash) = PasswordHash::new(password_hash) {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok()
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::user_repository::MockUserRepository;
    use mockall::predicate::*;

    #[tokio::test]
    async fn test_authenticate_invalid_email() {
        let mut mock_repo = MockUserRepository::new();

        mock_repo
            .expect_find_by_email()
            .with(eq("test@example.com"))
            .times(1)
            .returning(|_| Box::pin(async move { Ok(None) }));

        let service = AuthService::new(Arc::new(mock_repo));

        let request = LoginRequest {
            email: "test@example.com".to_string(),
            password: "password123".to_string(),
        };

        let result = service.authenticate(request).await;
        assert!(matches!(result, Err(AuthServiceError::InvalidCredentials)));
    }

    #[tokio::test]
    async fn test_get_user_by_id_not_found() {
        let mut mock_repo = MockUserRepository::new();

        mock_repo
            .expect_find_by_id()
            .with(eq(1))
            .times(1)
            .returning(|_| Box::pin(async move { Ok(None) }));

        let service = AuthService::new(Arc::new(mock_repo));

        let result = service.get_user_by_id(1).await;
        assert!(matches!(result, Err(AuthServiceError::UserNotFound)));
    }
}
