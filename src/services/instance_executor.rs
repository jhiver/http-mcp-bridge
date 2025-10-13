//! HTTP execution handler for MCP tool instances
//!
//! This module provides the runtime execution engine for MCP tool instances.
//! It bridges the MCP protocol's tool calling mechanism with SaraMCP's HTTP
//! execution and parameter resolution systems.
//!
//! # Architecture
//!
//! The InstanceExecutor orchestrates tool execution through three main steps:
//!
//! ```text
//! ┌─────────────────────┐
//! │  LLM provides       │
//! │  exposed params     │
//! └──────────┬──────────┘
//!            │
//!            ▼
//! ┌─────────────────────────────────┐
//! │  ParameterResolver              │
//! │  - Merge instance/server params │
//! │  - Substitute variables         │
//! │  - Decrypt secrets              │
//! │  - Cast types                   │
//! └──────────┬──────────────────────┘
//!            │
//!            ▼
//! ┌─────────────────────────────────┐
//! │  HttpExecutor                   │
//! │  - Render templates             │
//! │  - Execute HTTP request         │
//! │  - Capture response             │
//! └──────────┬──────────────────────┘
//!            │
//!            ▼
//! ┌─────────────────────────────────┐
//! │  CallToolResult                 │
//! │  - Success: response body       │
//! │  - Error: HTTP status + body    │
//! └─────────────────────────────────┘
//! ```
//!
//! # Parameter Resolution
//!
//! Parameters are resolved using a 3-tier priority system:
//! 1. **instance** - Fixed values configured when creating the instance
//! 2. **server** - Server-wide defaults from server_globals table
//! 3. **exposed** - Runtime values provided by the LLM
//!
//! # Integration
//!
//! Used by SaraMcpService to create dynamic tool handlers. Each InstanceExecutor
//! is cloned into a closure that handles MCP tool calls for a specific instance.
//!
//! # Usage
//!
//! ```rust,no_run
//! use saramcp::services::instance_executor::InstanceExecutor;
//! use saramcp::services::secrets_manager::SecretsManager;
//! use saramcp::models::tool::Tool;
//! use sqlx::SqlitePool;
//!
//! async fn execute_tool(pool: SqlitePool, server_id: i64, instance_id: i64) {
//!     let tool = Tool::get_by_id(&pool, 123).await.unwrap().unwrap();
//!     let secrets = SecretsManager::new().unwrap();
//!
//!     let executor = InstanceExecutor::new(pool, server_id, instance_id, tool, secrets);
//!
//!     // LLM-provided parameters
//!     let mut params = serde_json::Map::new();
//!     params.insert("api_key".to_string(), "secret123".into());
//!
//!     let result = executor.execute(Some(params)).await.unwrap();
//!     println!("Result: {:?}", result);
//! }
//! ```

use crate::models::tool::Tool;
use crate::services::execution_tracker::{ExecutionStatus, ExecutionTracker};
use crate::services::http_executor::HttpExecutor;
use crate::services::parameter_resolver::ParameterResolver;
use crate::services::secrets_manager::SecretsManager;
use rmcp::model::{CallToolResult, Content};
use sqlx::SqlitePool;
use std::collections::HashMap;
use time::OffsetDateTime;

#[derive(Clone)]
pub struct InstanceExecutor {
    pool: SqlitePool,
    server_id: i64,
    instance_id: i64,
    tool: Tool,
    http_executor: HttpExecutor,
    resolver: ParameterResolver,
    tracker: ExecutionTracker,
}

impl InstanceExecutor {
    /// Create new executor for a tool instance
    ///
    /// # Arguments
    ///
    /// * `pool` - Database connection pool for parameter resolution
    /// * `server_id` - ID of the server this instance belongs to
    /// * `instance_id` - ID of the tool instance to execute
    /// * `tool` - Tool definition containing HTTP template
    /// * `secrets` - Secrets manager for decrypting sensitive values
    ///
    /// # Returns
    ///
    /// A new InstanceExecutor ready to execute the tool
    pub fn new(
        pool: SqlitePool,
        server_id: i64,
        instance_id: i64,
        tool: Tool,
        secrets: SecretsManager,
    ) -> Self {
        let tracker = ExecutionTracker::new(pool.clone());
        Self {
            pool,
            server_id,
            instance_id,
            tool,
            http_executor: HttpExecutor::new(),
            resolver: ParameterResolver::new(secrets),
            tracker,
        }
    }

    /// Execute the tool with the given parameters
    ///
    /// Performs complete tool execution: parameter resolution, HTTP request,
    /// and response handling. This is the main entry point for MCP tool calls.
    ///
    /// # Arguments
    ///
    /// * `llm_params` - Optional map of parameter values provided by the LLM
    ///
    /// # Returns
    ///
    /// A `CallToolResult` containing either:
    /// - Success: HTTP response body as text content
    /// - Error: HTTP status and error message as text content
    ///
    /// # Errors
    ///
    /// Returns `rmcp::ErrorData` if:
    /// - Parameter resolution fails (INVALID_PARAMS)
    /// - HTTP execution fails (INTERNAL_ERROR)
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use saramcp::services::instance_executor::InstanceExecutor;
    /// # use serde_json::json;
    /// # async fn example(executor: &InstanceExecutor) {
    /// // Execute with LLM-provided parameters
    /// let mut params = serde_json::Map::new();
    /// params.insert("user_id".to_string(), json!(42));
    ///
    /// let result = executor.execute(Some(params)).await.unwrap();
    /// println!("Tool executed: {:?}", result);
    /// # }
    /// ```
    pub async fn execute(
        &self,
        llm_params: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let started_at = OffsetDateTime::now_utc();

        // Convert LLM params for tracking
        let input_params_for_tracking = llm_params.as_ref().map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<HashMap<String, serde_json::Value>>()
        });

        let llm_map = llm_params.map(|m| {
            m.into_iter()
                .collect::<HashMap<String, serde_json::Value>>()
        });

        // Resolve parameters
        let resolved = self
            .resolver
            .resolve_parameters(&self.pool, self.instance_id, llm_map)
            .await
            .map_err(|e| rmcp::ErrorData {
                code: rmcp::model::ErrorCode::INVALID_PARAMS,
                message: format!("Parameter resolution failed: {}", e).into(),
                data: None,
            })?;

        // Execute HTTP request
        let response = self
            .http_executor
            .execute_tool(&self.tool, &resolved)
            .await
            .map_err(|e| rmcp::ErrorData {
                code: rmcp::model::ErrorCode::INTERNAL_ERROR,
                message: format!("HTTP execution failed: {}", e).into(),
                data: None,
            })?;

        let completed_at = OffsetDateTime::now_utc();

        // Determine execution status
        let status = ExecutionStatus::from_result(response.is_success);
        let error_message = if response.is_success {
            None
        } else {
            Some(format!("HTTP {} - {}", response.status, response.body))
        };

        // Track execution (log errors but don't fail the execution)
        if let Err(e) = self
            .tracker
            .record_execution(
                self.server_id,
                self.instance_id,
                self.tool.id,
                started_at,
                completed_at,
                status,
                Some(response.status),
                error_message.clone(),
                input_params_for_tracking,
                Some(response.body.clone()),
                Some(response.headers.clone()),
                self.tool.url.clone(),
                Some(self.tool.method.clone()),
                Some(response.body.len()),
                Some("http".to_string()),
            )
            .await
        {
            tracing::error!("Failed to track execution: {}", e);
        }

        // Return result to MCP
        if response.is_success {
            Ok(CallToolResult::success(vec![Content::text(response.body)]))
        } else {
            let error_msg = format!("HTTP {} - {}", response.status, response.body);
            Ok(CallToolResult::error(vec![Content::text(error_msg)]))
        }
    }
}
