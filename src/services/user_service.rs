use crate::models::user::User;
use crate::repositories::user_repository::{RepositoryError, UserRepository};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, SaltString},
    Argon2, PasswordVerifier,
};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum UserServiceError {
    #[error("Invalid email address")]
    InvalidEmail,
    #[error("Password too weak (minimum 8 characters)")]
    WeakPassword,
    #[error("Passwords do not match")]
    PasswordMismatch,
    #[error("User not found")]
    UserNotFound,
    #[error("Email already registered")]
    EmailTaken,
    #[error("Password hashing failed: {0}")]
    HashingError(String),
    #[error("Repository error: {0}")]
    RepositoryError(#[from] RepositoryError),
}

pub struct CreateUserRequest {
    pub email: String,
    pub password: String,
    pub password_confirm: Option<String>,
    pub email_verified: bool,
}

pub struct UpdatePasswordRequest {
    pub user_id: i64,
    pub new_password: String,
    pub new_password_confirm: Option<String>,
}

pub struct UpdateEmailRequest {
    pub user_id: i64,
    pub new_email: String,
}

pub struct UserService {
    repository: Arc<dyn UserRepository>,
}

impl UserService {
    pub fn new(repository: Arc<dyn UserRepository>) -> Self {
        Self { repository }
    }

    pub async fn create_user(&self, request: CreateUserRequest) -> Result<User, UserServiceError> {
        // Validate email
        self.validate_email(&request.email)?;

        // Validate password confirmation if provided
        if let Some(ref confirm) = request.password_confirm {
            if request.password != *confirm {
                return Err(UserServiceError::PasswordMismatch);
            }
        }

        // Validate password strength
        self.validate_password(&request.password)?;

        // Hash password
        let password_hash = self.hash_password(&request.password)?;

        // Create user in repository
        match self
            .repository
            .create_user(&request.email, &password_hash, request.email_verified)
            .await
        {
            Ok(user) => Ok(user),
            Err(RepositoryError::AlreadyExists) => Err(UserServiceError::EmailTaken),
            Err(e) => Err(UserServiceError::RepositoryError(e)),
        }
    }

    pub async fn find_user_by_email(&self, email: &str) -> Result<Option<User>, UserServiceError> {
        Ok(self.repository.find_by_email(email).await?)
    }

    pub async fn find_user_by_id(&self, id: i64) -> Result<Option<User>, UserServiceError> {
        Ok(self.repository.find_by_id(id).await?)
    }

    pub async fn list_users(
        &self,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<User>, UserServiceError> {
        Ok(self.repository.list_users(limit, offset).await?)
    }

    pub async fn delete_user(&self, id: i64) -> Result<(), UserServiceError> {
        match self.repository.delete_user(id).await {
            Ok(()) => Ok(()),
            Err(RepositoryError::NotFound) => Err(UserServiceError::UserNotFound),
            Err(e) => Err(UserServiceError::RepositoryError(e)),
        }
    }

    pub async fn verify_user_email(&self, id: i64) -> Result<(), UserServiceError> {
        match self.repository.verify_email(id).await {
            Ok(()) => Ok(()),
            Err(RepositoryError::NotFound) => Err(UserServiceError::UserNotFound),
            Err(e) => Err(UserServiceError::RepositoryError(e)),
        }
    }

    pub async fn update_password(
        &self,
        request: UpdatePasswordRequest,
    ) -> Result<(), UserServiceError> {
        // Validate password confirmation if provided
        if let Some(ref confirm) = request.new_password_confirm {
            if request.new_password != *confirm {
                return Err(UserServiceError::PasswordMismatch);
            }
        }

        // Validate password strength
        self.validate_password(&request.new_password)?;

        // Hash new password
        let password_hash = self.hash_password(&request.new_password)?;

        // Update password in repository
        match self
            .repository
            .update_password(request.user_id, &password_hash)
            .await
        {
            Ok(()) => Ok(()),
            Err(RepositoryError::NotFound) => Err(UserServiceError::UserNotFound),
            Err(e) => Err(UserServiceError::RepositoryError(e)),
        }
    }

    fn validate_email(&self, email: &str) -> Result<(), UserServiceError> {
        if !email.contains('@') || email.len() > 255 || email.is_empty() {
            return Err(UserServiceError::InvalidEmail);
        }
        Ok(())
    }

    fn validate_password(&self, password: &str) -> Result<(), UserServiceError> {
        if password.len() < 8 {
            return Err(UserServiceError::WeakPassword);
        }
        Ok(())
    }

    fn hash_password(&self, password: &str) -> Result<String, UserServiceError> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        argon2
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|e| UserServiceError::HashingError(e.to_string()))
    }

    pub fn verify_password(&self, password: &str, password_hash: &str) -> bool {
        if let Ok(parsed_hash) = PasswordHash::new(password_hash) {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok()
        } else {
            false
        }
    }

    pub async fn update_email(&self, request: UpdateEmailRequest) -> Result<(), UserServiceError> {
        // Validate email format
        self.validate_email(&request.new_email)?;

        // Check if email is already taken by another user
        if let Some(existing_user) = self.repository.find_by_email(&request.new_email).await? {
            if existing_user.id != request.user_id {
                return Err(UserServiceError::EmailTaken);
            }
        }

        // Update email in repository
        match self
            .repository
            .update_email(request.user_id, &request.new_email)
            .await
        {
            Ok(()) => Ok(()),
            Err(RepositoryError::NotFound) => Err(UserServiceError::UserNotFound),
            Err(e) => Err(UserServiceError::RepositoryError(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::user_repository::MockUserRepository;
    use mockall::predicate::*;

    #[tokio::test]
    async fn test_create_user_success() {
        let mut mock_repo = MockUserRepository::new();

        let user = User {
            id: 1,
            email: "test@example.com".to_string(),
            password_hash: "hash".to_string(),
            created_at: None,
            email_verified: false,
        };

        let user_clone = user.clone();
        mock_repo
            .expect_create_user()
            .with(eq("test@example.com"), always(), eq(false))
            .times(1)
            .returning(move |_, _, _| {
                let user = user_clone.clone();
                Box::pin(async move { Ok(user) })
            });

        let service = UserService::new(Arc::new(mock_repo));

        let request = CreateUserRequest {
            email: "test@example.com".to_string(),
            password: "password123".to_string(),
            password_confirm: None,
            email_verified: false,
        };

        let result = service.create_user(request).await;
        assert!(result.is_ok());
        let user = result.expect("Expected Ok result");
        assert_eq!(user.email, "test@example.com");
    }

    #[tokio::test]
    async fn test_create_user_weak_password() {
        let mock_repo = MockUserRepository::new();
        let service = UserService::new(Arc::new(mock_repo));

        let request = CreateUserRequest {
            email: "test@example.com".to_string(),
            password: "short".to_string(),
            password_confirm: None,
            email_verified: false,
        };

        let result = service.create_user(request).await;
        assert!(matches!(result, Err(UserServiceError::WeakPassword)));
    }

    #[tokio::test]
    async fn test_create_user_invalid_email() {
        let mock_repo = MockUserRepository::new();
        let service = UserService::new(Arc::new(mock_repo));

        let request = CreateUserRequest {
            email: "invalid-email".to_string(),
            password: "password123".to_string(),
            password_confirm: None,
            email_verified: false,
        };

        let result = service.create_user(request).await;
        assert!(matches!(result, Err(UserServiceError::InvalidEmail)));
    }
}
