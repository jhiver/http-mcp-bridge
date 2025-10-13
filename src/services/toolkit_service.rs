use crate::error::{AppError, Result};
use crate::models::{
    CreateToolkitRequest, PublicToolkitDetails, Toolkit, ToolkitSummary, ToolkitWithStats,
    UpdateToolkitRequest,
};
use crate::repositories::{ToolRepository, ToolkitRepository};
use std::sync::Arc;

pub struct ToolkitService {
    repository: Arc<dyn ToolkitRepository>,
    tool_repository: Arc<dyn ToolRepository>,
}

impl ToolkitService {
    pub fn new(
        repository: Arc<dyn ToolkitRepository>,
        tool_repository: Arc<dyn ToolRepository>,
    ) -> Self {
        Self {
            repository,
            tool_repository,
        }
    }

    pub async fn create_toolkit(&self, user_id: i64, request: CreateToolkitRequest) -> Result<i64> {
        // Validate input
        if request.title.trim().is_empty() {
            return Err(AppError::Validation("Title is required".to_string()));
        }

        if request.title.len() > 100 {
            return Err(AppError::Validation(
                "Title must be 100 characters or less".to_string(),
            ));
        }

        if !["private", "public"].contains(&request.visibility.as_str()) {
            return Err(AppError::Validation(
                "Visibility must be 'private' or 'public'".to_string(),
            ));
        }

        // Create toolkit
        self.repository.create(user_id, request).await
    }

    pub async fn get_toolkit(&self, id: i64, user_id: i64) -> Result<Toolkit> {
        self.repository
            .get_by_id(id, user_id)
            .await?
            .ok_or(AppError::UserNotFound)
    }

    pub async fn list_toolkits(&self, user_id: i64) -> Result<Vec<Toolkit>> {
        self.repository.list_by_user(user_id).await
    }

    pub async fn list_toolkit_summaries(&self, user_id: i64) -> Result<Vec<ToolkitSummary>> {
        self.repository.list_summaries_by_user(user_id).await
    }

    pub async fn update_toolkit(
        &self,
        id: i64,
        user_id: i64,
        request: UpdateToolkitRequest,
    ) -> Result<()> {
        // Validate input
        if request.title.trim().is_empty() {
            return Err(AppError::Validation("Title is required".to_string()));
        }

        if request.title.len() > 100 {
            return Err(AppError::Validation(
                "Title must be 100 characters or less".to_string(),
            ));
        }

        if !["private", "public"].contains(&request.visibility.as_str()) {
            return Err(AppError::Validation(
                "Visibility must be 'private' or 'public'".to_string(),
            ));
        }

        // Update toolkit
        let updated = self.repository.update(id, user_id, request).await?;

        if updated {
            Ok(())
        } else {
            Err(AppError::UserNotFound)
        }
    }

    pub async fn delete_toolkit(&self, id: i64, user_id: i64) -> Result<()> {
        let deleted = self.repository.delete(id, user_id).await?;

        if deleted {
            Ok(())
        } else {
            Err(AppError::UserNotFound)
        }
    }

    pub async fn verify_ownership(&self, id: i64, user_id: i64) -> Result<bool> {
        self.repository.verify_ownership(id, user_id).await
    }

    // New methods for public toolkit browsing and cloning

    pub async fn list_public_toolkits(&self) -> Result<Vec<ToolkitWithStats>> {
        self.repository.list_public_toolkits().await
    }

    pub async fn get_public_toolkit_details(&self, id: i64) -> Result<PublicToolkitDetails> {
        // Get the toolkit - it must be public
        let toolkit = self
            .repository
            .get_public_toolkit(id)
            .await?
            .ok_or(AppError::UserNotFound)?;

        // Get the tools for this toolkit
        let tools = self.tool_repository.list_by_toolkit(id).await?;

        // Get the owner email
        let owner_email = self
            .repository
            .list_public_toolkits()
            .await?
            .into_iter()
            .find(|t| t.id == id)
            .map(|t| t.owner_email)
            .unwrap_or_else(|| "Unknown".to_string());

        // Get parent toolkit info if it exists
        let parent_toolkit = if let Some(parent_id) = toolkit.parent_toolkit_id {
            if let Ok(Some(parent)) = self.repository.get_public_toolkit(parent_id).await {
                let parent_tools_count = self
                    .tool_repository
                    .list_by_toolkit(parent_id)
                    .await
                    .map(|tools| tools.len() as i32)
                    .unwrap_or(0);

                Some(ToolkitSummary {
                    id: parent.id,
                    title: parent.title,
                    description: parent.description.unwrap_or(String::new()),
                    tools_count: parent_tools_count,
                })
            } else {
                None
            }
        } else {
            None
        };

        Ok(PublicToolkitDetails {
            toolkit,
            tools,
            owner_email,
            parent_toolkit,
        })
    }

    pub async fn can_user_view_toolkit(
        &self,
        toolkit_id: i64,
        user_id: Option<i64>,
    ) -> Result<bool> {
        // First check if it's public
        if self
            .repository
            .get_public_toolkit(toolkit_id)
            .await?
            .is_some()
        {
            return Ok(true);
        }

        // If not public, check ownership if user_id is provided
        if let Some(uid) = user_id {
            return self.repository.verify_ownership(toolkit_id, uid).await;
        }

        Ok(false)
    }

    pub async fn clone_toolkit(
        &self,
        original_id: i64,
        user_id: i64,
        new_title: Option<String>,
    ) -> Result<i64> {
        // First verify the toolkit can be cloned (must be public or owned by user)
        let can_view = self
            .can_user_view_toolkit(original_id, Some(user_id))
            .await?;
        if !can_view {
            return Err(AppError::Validation(
                "You don't have permission to clone this toolkit".to_string(),
            ));
        }

        // Get the original toolkit to get its title
        let original_toolkit =
            if let Some(toolkit) = self.repository.get_public_toolkit(original_id).await? {
                toolkit
            } else {
                // Try to get it as owner
                self.repository
                    .get_by_id(original_id, user_id)
                    .await?
                    .ok_or(AppError::UserNotFound)?
            };

        // Generate the new title
        let final_title =
            new_title.unwrap_or_else(|| format!("Copy of {}", original_toolkit.title));

        // Validate the new title
        if final_title.trim().is_empty() {
            return Err(AppError::Validation("Title is required".to_string()));
        }

        if final_title.len() > 100 {
            return Err(AppError::Validation(
                "Title must be 100 characters or less".to_string(),
            ));
        }

        // Clone the toolkit (repository handles copying tools)
        let new_toolkit_id = self
            .repository
            .clone_toolkit(original_id, user_id, final_title)
            .await?;

        // Increment the clone count of the original toolkit
        self.repository.increment_clone_count(original_id).await?;

        Ok(new_toolkit_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::SqliteToolkitRepository;
    use crate::test_utils::{create_test_pool, create_test_user};

    #[tokio::test]
    async fn test_create_toolkit_validation() {
        let pool = create_test_pool().await;
        let repository = Arc::new(SqliteToolkitRepository::new(pool.clone()));
        let tool_repository =
            Arc::new(crate::repositories::SqliteToolRepository::new(pool.clone()));
        let service = ToolkitService::new(repository, tool_repository);
        let user_id = create_test_user(&pool, "test@example.com", "password")
            .await
            .unwrap();

        // Test empty title
        let request = CreateToolkitRequest {
            title: "   ".to_string(),
            description: None,
            visibility: "private".to_string(),
        };
        let result = service.create_toolkit(user_id, request).await;
        assert!(matches!(result, Err(AppError::Validation(_))));

        // Test invalid visibility
        let request = CreateToolkitRequest {
            title: "Test".to_string(),
            description: None,
            visibility: "invalid".to_string(),
        };
        let result = service.create_toolkit(user_id, request).await;
        assert!(matches!(result, Err(AppError::Validation(_))));

        // Test valid request
        let request = CreateToolkitRequest {
            title: "Valid Toolkit".to_string(),
            description: Some("Description".to_string()),
            visibility: "public".to_string(),
        };
        let result = service.create_toolkit(user_id, request).await;
        assert!(result.is_ok());
    }
}
