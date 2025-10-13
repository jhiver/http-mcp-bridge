use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde_json::json;
use thiserror::Error;

// Type alias for Result with our AppError
pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("User not found")]
    UserNotFound,

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Internal server error")]
    InternalError,

    #[error("Validation error: {0}")]
    Validation(String),
}

/// Error types for MCP service operations
///
/// These errors cover all failure modes in the MCP protocol implementation,
/// from database access to HTTP execution to parameter resolution.
///
/// # Error Conversion
///
/// All McpServiceError variants are automatically converted to rmcp::ErrorData
/// with appropriate MCP error codes via the `From<McpServiceError>` implementation.
///
/// # Usage
///
/// ```rust,no_run
/// use saramcp::error::McpServiceError;
///
/// fn validate_instance(id: i64) -> Result<(), McpServiceError> {
///     if id <= 0 {
///         return Err(McpServiceError::InstanceNotFound(
///             format!("Invalid instance ID: {}", id)
///         ));
///     }
///     Ok(())
/// }
/// ```
#[derive(Debug, Error)]
pub enum McpServiceError {
    /// Database operation failed
    ///
    /// Covers all SQLx errors including connection failures, query errors,
    /// and constraint violations. Maps to MCP error code INTERNAL_ERROR.
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    /// HTTP request execution failed
    ///
    /// Includes network errors, timeouts, and invalid responses from
    /// the HTTP executor. Maps to MCP error code INTERNAL_ERROR.
    #[error("HTTP execution failed: {0}")]
    HttpExecution(#[from] crate::services::HttpExecutorError),

    /// Parameter resolution failed
    ///
    /// Occurs when parameters cannot be resolved due to missing values,
    /// type mismatches, or invalid variable substitutions. Maps to MCP
    /// error code INVALID_PARAMS.
    #[error("Parameter resolution failed: {0}")]
    ParameterResolution(String),

    /// JSON Schema generation failed
    ///
    /// Happens when the generated schema is not a valid JSON object
    /// or contains invalid type definitions. Maps to MCP error code
    /// INTERNAL_ERROR.
    #[error("Schema generation failed: {0}")]
    SchemaGeneration(String),

    /// Template rendering failed
    ///
    /// Occurs when variable substitution or template rendering encounters
    /// invalid syntax or missing variables. Maps to MCP error code
    /// INVALID_PARAMS.
    #[error("Template rendering failed: {0}")]
    TemplateRendering(String),

    /// Tool definition not found in database
    ///
    /// The referenced tool ID does not exist. Maps to MCP error code
    /// METHOD_NOT_FOUND.
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    /// Tool instance not found in database
    ///
    /// The referenced instance ID does not exist. Maps to MCP error code
    /// METHOD_NOT_FOUND.
    #[error("Instance not found: {0}")]
    InstanceNotFound(String),

    /// Internal service error
    ///
    /// Catch-all for unexpected errors like secrets manager failures
    /// or logic errors. Maps to MCP error code INTERNAL_ERROR.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<anyhow::Error> for McpServiceError {
    fn from(err: anyhow::Error) -> Self {
        McpServiceError::Internal(err.to_string())
    }
}

/// Convert McpServiceError to rmcp::ErrorData for MCP protocol responses
///
/// Maps each error variant to an appropriate MCP error code:
///
/// | McpServiceError Variant | MCP Error Code      | Reason                           |
/// |------------------------|---------------------|----------------------------------|
/// | Database               | INTERNAL_ERROR      | Server-side failure              |
/// | HttpExecution          | INTERNAL_ERROR      | Server-side failure              |
/// | ParameterResolution    | INVALID_PARAMS      | Client provided invalid params   |
/// | SchemaGeneration       | INTERNAL_ERROR      | Server configuration issue       |
/// | TemplateRendering      | INVALID_PARAMS      | Invalid parameter values         |
/// | ToolNotFound           | METHOD_NOT_FOUND    | Requested tool doesn't exist     |
/// | InstanceNotFound       | METHOD_NOT_FOUND    | Requested instance doesn't exist |
/// | Internal               | INTERNAL_ERROR      | Unexpected server error          |
impl From<McpServiceError> for rmcp::ErrorData {
    fn from(err: McpServiceError) -> Self {
        use rmcp::model::{ErrorCode, ErrorData};

        let (code, message) = match err {
            McpServiceError::Database(e) => (ErrorCode::INTERNAL_ERROR, e.to_string()),
            McpServiceError::HttpExecution(e) => (ErrorCode::INTERNAL_ERROR, e.to_string()),
            McpServiceError::ParameterResolution(msg) => (ErrorCode::INVALID_PARAMS, msg),
            McpServiceError::SchemaGeneration(msg) => (ErrorCode::INTERNAL_ERROR, msg),
            McpServiceError::TemplateRendering(msg) => (ErrorCode::INVALID_PARAMS, msg),
            McpServiceError::ToolNotFound(msg) => (ErrorCode::METHOD_NOT_FOUND, msg),
            McpServiceError::InstanceNotFound(msg) => (ErrorCode::METHOD_NOT_FOUND, msg),
            McpServiceError::Internal(msg) => (ErrorCode::INTERNAL_ERROR, msg),
        };

        ErrorData {
            code,
            message: message.into(),
            data: None,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AppError::AuthenticationFailed | AppError::InvalidCredentials => (
                StatusCode::UNAUTHORIZED,
                "Authentication failed".to_string(),
            ),
            AppError::UserNotFound => (StatusCode::NOT_FOUND, "User not found".to_string()),
            AppError::Validation(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Database(_) | AppError::InternalError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error".to_string(),
            ),
        };

        (status, error_message).into_response()
    }
}

/// MCP authentication errors
#[derive(Debug)]
pub enum McpAuthError {
    MissingAuthorizationHeader,
    InvalidAuthorizationFormat,
    InvalidToken,
    ExpiredToken,
    Forbidden,
    ServerNotFound,
    DatabaseError(sqlx::Error),
    ServiceError(anyhow::Error),
}

impl std::fmt::Display for McpAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpAuthError::MissingAuthorizationHeader => {
                write!(f, "Authorization header is required")
            }
            McpAuthError::InvalidAuthorizationFormat => {
                write!(f, "Authorization header must be 'Bearer <token>'")
            }
            McpAuthError::InvalidToken => write!(f, "Invalid access token"),
            McpAuthError::ExpiredToken => write!(f, "Access token has expired"),
            McpAuthError::Forbidden => {
                write!(f, "You don't have permission to access this server")
            }
            McpAuthError::ServerNotFound => write!(f, "Server not found"),
            McpAuthError::DatabaseError(e) => write!(f, "Database error: {}", e),
            McpAuthError::ServiceError(e) => write!(f, "Service error: {}", e),
        }
    }
}

impl IntoResponse for McpAuthError {
    fn into_response(self) -> Response {
        let (status, error_code, description) = match self {
            McpAuthError::MissingAuthorizationHeader => (
                StatusCode::UNAUTHORIZED,
                "missing_token",
                "Authorization header is required",
            ),
            McpAuthError::InvalidAuthorizationFormat => (
                StatusCode::UNAUTHORIZED,
                "invalid_request",
                "Authorization header must be 'Bearer <token>'",
            ),
            McpAuthError::InvalidToken => (
                StatusCode::UNAUTHORIZED,
                "invalid_token",
                "The access token is invalid",
            ),
            McpAuthError::ExpiredToken => (
                StatusCode::UNAUTHORIZED,
                "invalid_token",
                "The access token has expired",
            ),
            McpAuthError::Forbidden => (
                StatusCode::FORBIDDEN,
                "insufficient_scope",
                "You don't have permission to access this server",
            ),
            McpAuthError::ServerNotFound => (
                StatusCode::NOT_FOUND,
                "server_not_found",
                "The requested server does not exist",
            ),
            McpAuthError::DatabaseError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "An internal error occurred",
            ),
            McpAuthError::ServiceError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "An internal error occurred",
            ),
        };

        let body = json!({
            "error": error_code,
            "error_description": description,
        });

        (status, Json(body)).into_response()
    }
}

// Conversion traits
impl From<sqlx::Error> for McpAuthError {
    fn from(err: sqlx::Error) -> Self {
        McpAuthError::DatabaseError(err)
    }
}

impl From<anyhow::Error> for McpAuthError {
    fn from(err: anyhow::Error) -> Self {
        let msg = err.to_string();
        if msg.contains("Server not found") {
            McpAuthError::ServerNotFound
        } else if msg.contains("Invalid access token") {
            McpAuthError::InvalidToken
        } else if msg.contains("expired") {
            McpAuthError::ExpiredToken
        } else {
            McpAuthError::ServiceError(err)
        }
    }
}
