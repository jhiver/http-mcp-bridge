//! Tool testing service for manual parameter input
//!
//! This module provides functionality for testing HTTP tools with manually
//! provided parameter values. This is separate from the MCP execution flow
//! and serves as a simple testing utility for tool developers.
//!
//! # Architecture
//!
//! The testing flow consists of two main steps:
//!
//! ```text
//! ┌─────────────────────────────────┐
//! │  User provides string params    │
//! │  via test form                  │
//! └──────────┬──────────────────────┘
//!            │
//!            ▼
//! ┌─────────────────────────────────┐
//! │  prepare_test_parameters        │
//! │  - Convert strings to typed     │
//! │    JSON values using param type │
//! │  - Validate required params     │
//! └──────────┬──────────────────────┘
//!            │
//!            ▼
//! ┌─────────────────────────────────┐
//! │  HttpExecutor                   │
//! │  - Render templates             │
//! │  - Execute HTTP request         │
//! │  - Return response              │
//! └─────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! use saramcp::services::tool_test_service;
//! use std::collections::HashMap;
//!
//! async fn test_example(pool: &sqlx::SqlitePool) {
//!     let mut params = HashMap::new();
//!     params.insert("user_id".to_string(), "123".to_string());
//!     params.insert("api_key".to_string(), "secret".to_string());
//!
//!     let tool_id = 1;
//!     let user_id = 1;
//!     let result = tool_test_service::test_tool(
//!         pool,
//!         tool_id,
//!         user_id,
//!         params
//!     ).await.unwrap();
//!
//!     println!("Status: {}", result.status);
//!     println!("Body: {}", result.body);
//! }
//! ```

use crate::error::AppError;
use crate::models::tool::{ExtractedParameter, Tool};
use crate::services::http_executor::{ExecutionResult, HttpExecutor, HttpExecutorError};
use crate::services::variable_engine::VariableType;
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;

/// Converts string parameter inputs to serde_json::Value using type information
///
/// Takes a HashMap of string values from the test form and converts them
/// to properly typed JSON values based on the extracted parameter types.
///
/// # Arguments
/// * `params` - Raw string values from test form
/// * `extracted_params` - Parameter metadata from tool.extract_parameters()
///
/// # Returns
/// HashMap<String, Value> suitable for HttpExecutor, or AppError
///
/// # Errors
/// Returns AppError::Validation if:
/// - Required parameter is missing
/// - Type casting fails (e.g., "abc" as integer)
fn prepare_test_parameters(
    params: HashMap<String, String>,
    extracted_params: &[ExtractedParameter],
) -> Result<HashMap<String, Value>, AppError> {
    let mut typed_params = HashMap::new();

    for extracted in extracted_params {
        let param_name = &extracted.name;
        let param_type_str = &extracted.param_type;

        // Get the string value from the params HashMap
        let string_value = params.get(param_name).ok_or_else(|| {
            AppError::Validation(format!("Required parameter '{}' is missing", param_name))
        })?;

        // Convert type string to VariableType
        let var_type = match param_type_str.to_lowercase().as_str() {
            "number" => VariableType::Number,
            "integer" => VariableType::Integer,
            "boolean" | "bool" => VariableType::Boolean,
            "json" => VariableType::Json,
            "url" => VariableType::Url,
            _ => VariableType::String,
        };

        // Cast the string value to the appropriate JSON type
        let typed_value = var_type.cast(string_value).map_err(|e| {
            AppError::Validation(format!(
                "Failed to cast parameter '{}' to type '{}': {}",
                param_name, param_type_str, e
            ))
        })?;

        typed_params.insert(param_name.clone(), typed_value);
    }

    Ok(typed_params)
}

/// Tests a tool with user-provided parameter values
///
/// Verifies ownership, loads the tool, prepares parameters, and executes
/// the HTTP request using HttpExecutor.
///
/// # Arguments
/// * `pool` - Database connection pool
/// * `tool_id` - ID of the tool to test
/// * `user_id` - ID of the user testing the tool (for ownership verification)
/// * `test_params` - String parameter values from test form
///
/// # Returns
/// ExecutionResult with HTTP response, or AppError
///
/// # Errors
/// - NotFound if tool doesn't exist
/// - Unauthorized if user doesn't own the toolkit containing the tool
/// - Validation if parameters are invalid
/// - InternalError if request fails
pub async fn test_tool(
    pool: &SqlitePool,
    tool_id: i64,
    user_id: i64,
    test_params: HashMap<String, String>,
) -> Result<ExecutionResult, AppError> {
    // Load the tool from database
    let tool = Tool::get_by_id(pool, tool_id)
        .await?
        .ok_or_else(|| AppError::Validation(format!("Tool {} not found", tool_id)))?;

    // Load the toolkit and verify ownership
    let toolkit_row = sqlx::query!(
        r#"
        SELECT id, user_id, title, description, visibility, created_at, updated_at
        FROM toolkits
        WHERE id = ?
        "#,
        tool.toolkit_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Validation(format!("Toolkit {} not found", tool.toolkit_id)))?;

    if toolkit_row.user_id != user_id {
        return Err(AppError::Validation(
            "You don't have permission to test this tool".to_string(),
        ));
    }

    // Extract parameters from the tool template
    let extracted_params = tool.extract_parameters();

    // Convert string parameters to typed JSON values
    let typed_params = prepare_test_parameters(test_params, &extracted_params)?;

    // Create HTTP executor and execute the request
    let executor = HttpExecutor::new();
    let result = executor
        .execute_tool(&tool, &typed_params)
        .await
        .map_err(|e| match e {
            HttpExecutorError::InvalidUrl(msg) => {
                AppError::Validation(format!("Invalid URL: {}", msg))
            }
            HttpExecutorError::InvalidMethod(msg) => {
                AppError::Validation(format!("Invalid HTTP method: {}", msg))
            }
            HttpExecutorError::TemplateError(msg) => {
                AppError::Validation(format!("Template error: {}", msg))
            }
            HttpExecutorError::InvalidHeaders(msg) => {
                AppError::Validation(format!("Invalid headers: {}", msg))
            }
            _ => AppError::InternalError,
        })?;

    Ok(result)
}
