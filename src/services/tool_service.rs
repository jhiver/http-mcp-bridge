use crate::error::{AppError, Result};
use crate::models::{CreateToolRequest, ExtractedParameter, Tool, UpdateToolRequest};
use crate::repositories::{ToolRepository, ToolkitRepository};
use std::sync::Arc;

pub struct ToolService {
    tool_repository: Arc<dyn ToolRepository>,
    toolkit_repository: Arc<dyn ToolkitRepository>,
}

impl ToolService {
    pub fn new(
        tool_repository: Arc<dyn ToolRepository>,
        toolkit_repository: Arc<dyn ToolkitRepository>,
    ) -> Self {
        Self {
            tool_repository,
            toolkit_repository,
        }
    }

    pub async fn create_tool(
        &self,
        toolkit_id: i64,
        user_id: i64,
        request: CreateToolRequest,
    ) -> Result<i64> {
        // Verify toolkit ownership
        let owns_toolkit = self
            .toolkit_repository
            .verify_ownership(toolkit_id, user_id)
            .await?;

        if !owns_toolkit {
            return Err(AppError::UserNotFound);
        }

        // Validate input
        if request.name.trim().is_empty() {
            return Err(AppError::Validation("Tool name is required".to_string()));
        }

        if request.name.len() > 100 {
            return Err(AppError::Validation(
                "Tool name must be 100 characters or less".to_string(),
            ));
        }

        // Validate HTTP method
        if !["GET", "POST", "PUT", "DELETE", "PATCH"].contains(&request.method.as_str()) {
            return Err(AppError::Validation("Invalid HTTP method".to_string()));
        }

        // Validate JSON format for headers
        if let Some(ref headers) = request.headers {
            if !headers.trim().is_empty()
                && serde_json::from_str::<serde_json::Value>(headers).is_err()
            {
                return Err(AppError::Validation(
                    "Headers must be valid JSON".to_string(),
                ));
            }
        }

        // Validate JSON format for body
        if let Some(ref body) = request.body {
            if !body.trim().is_empty() && serde_json::from_str::<serde_json::Value>(body).is_err() {
                return Err(AppError::Validation("Body must be valid JSON".to_string()));
            }
        }

        // Create tool with parameters
        self.tool_repository.create(toolkit_id, request).await
    }

    pub async fn get_tool(&self, id: i64, user_id: i64) -> Result<(Tool, Vec<ExtractedParameter>)> {
        // Get tool
        let tool = self
            .tool_repository
            .get_by_id(id)
            .await?
            .ok_or(AppError::UserNotFound)?;

        // Verify ownership
        let owns_toolkit = self
            .toolkit_repository
            .verify_ownership(tool.toolkit_id, user_id)
            .await?;

        if !owns_toolkit {
            return Err(AppError::UserNotFound);
        }

        // Extract parameters dynamically from tool templates
        let parameters = tool.extract_parameters();

        Ok((tool, parameters))
    }

    pub async fn list_tools(&self, toolkit_id: i64, user_id: i64) -> Result<Vec<Tool>> {
        // Verify ownership
        let owns_toolkit = self
            .toolkit_repository
            .verify_ownership(toolkit_id, user_id)
            .await?;

        if !owns_toolkit {
            return Err(AppError::UserNotFound);
        }

        self.tool_repository.list_by_toolkit(toolkit_id).await
    }

    pub async fn update_tool(
        &self,
        id: i64,
        user_id: i64,
        request: UpdateToolRequest,
    ) -> Result<()> {
        // Get tool to check ownership
        let tool = self
            .tool_repository
            .get_by_id(id)
            .await?
            .ok_or(AppError::UserNotFound)?;

        // Verify ownership
        let owns_toolkit = self
            .toolkit_repository
            .verify_ownership(tool.toolkit_id, user_id)
            .await?;

        if !owns_toolkit {
            return Err(AppError::UserNotFound);
        }

        // Validate input
        if request.name.trim().is_empty() {
            return Err(AppError::Validation("Tool name is required".to_string()));
        }

        // Validate HTTP method
        if !["GET", "POST", "PUT", "DELETE", "PATCH"].contains(&request.method.as_str()) {
            return Err(AppError::Validation("Invalid HTTP method".to_string()));
        }

        // Validate JSON format for headers
        if let Some(ref headers) = request.headers {
            if !headers.trim().is_empty()
                && serde_json::from_str::<serde_json::Value>(headers).is_err()
            {
                return Err(AppError::Validation(
                    "Headers must be valid JSON".to_string(),
                ));
            }
        }

        // Validate JSON format for body
        if let Some(ref body) = request.body {
            if !body.trim().is_empty() && serde_json::from_str::<serde_json::Value>(body).is_err() {
                return Err(AppError::Validation("Body must be valid JSON".to_string()));
            }
        }

        // Update tool
        let updated = self.tool_repository.update(id, request).await?;

        if updated {
            Ok(())
        } else {
            Err(AppError::UserNotFound)
        }
    }

    pub async fn delete_tool(&self, id: i64, user_id: i64) -> Result<()> {
        // Get tool to check ownership
        let tool = self
            .tool_repository
            .get_by_id(id)
            .await?
            .ok_or(AppError::UserNotFound)?;

        // Verify ownership
        let owns_toolkit = self
            .toolkit_repository
            .verify_ownership(tool.toolkit_id, user_id)
            .await?;

        if !owns_toolkit {
            return Err(AppError::UserNotFound);
        }

        // Delete tool (parameters will cascade)
        let deleted = self.tool_repository.delete(id).await?;

        if deleted {
            Ok(())
        } else {
            Err(AppError::UserNotFound)
        }
    }
}
