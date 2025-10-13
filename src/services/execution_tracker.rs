use crate::models::ExecutionHistory;
use anyhow::Result;
use sqlx::SqlitePool;
use std::collections::HashMap;
use time::OffsetDateTime;

#[derive(Clone, Debug)]
pub struct ExecutionTracker {
    pool: SqlitePool,
}

impl ExecutionTracker {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Record a completed execution
    #[allow(clippy::too_many_arguments)]
    pub async fn record_execution(
        &self,
        server_id: i64,
        instance_id: i64,
        tool_id: i64,
        started_at: OffsetDateTime,
        completed_at: OffsetDateTime,
        status: ExecutionStatus,
        http_status_code: Option<u16>,
        error_message: Option<String>,
        input_params: Option<HashMap<String, serde_json::Value>>,
        response_body: Option<String>,
        response_headers: Option<HashMap<String, String>>,
        request_url: Option<String>,
        request_method: Option<String>,
        response_size_bytes: Option<usize>,
        transport: Option<String>,
    ) -> Result<i64> {
        let duration_ms = (completed_at - started_at).whole_milliseconds() as i64;

        let input_params_json = input_params.and_then(|p| serde_json::to_string(&p).ok());

        let response_headers_json = response_headers.and_then(|h| serde_json::to_string(&h).ok());

        let execution_id = ExecutionHistory::create(
            &self.pool,
            server_id,
            instance_id,
            tool_id,
            &started_at.to_string(),
            Some(&completed_at.to_string()),
            Some(duration_ms),
            status.as_str(),
            http_status_code.map(|c| c as i64),
            error_message.as_deref(),
            input_params_json.as_deref(),
            response_body.as_deref(),
            response_headers_json.as_deref(),
            request_url.as_deref(),
            request_method.as_deref(),
            response_size_bytes.map(|s| s as i64),
            transport.as_deref(),
        )
        .await?;

        Ok(execution_id)
    }

    /// Get recent executions for a user (across all their servers)
    pub async fn get_recent_for_user(
        &self,
        user_id: i64,
        limit: i64,
    ) -> Result<Vec<ExecutionHistory>> {
        ExecutionHistory::list_recent(&self.pool, user_id, limit).await
    }

    /// Get executions for a specific server
    pub async fn get_for_server(
        &self,
        server_id: i64,
        limit: i64,
    ) -> Result<Vec<ExecutionHistory>> {
        ExecutionHistory::list_by_server(&self.pool, server_id, limit).await
    }

    /// Get executions for a specific instance
    pub async fn get_for_instance(
        &self,
        instance_id: i64,
        limit: i64,
    ) -> Result<Vec<ExecutionHistory>> {
        ExecutionHistory::list_by_instance(&self.pool, instance_id, limit).await
    }

    /// Delete old executions (for cleanup/maintenance)
    pub async fn cleanup_old_executions(&self, days: i64) -> Result<u64> {
        ExecutionHistory::delete_older_than_days(&self.pool, days).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStatus {
    Success,
    Error,
    Timeout,
}

impl ExecutionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecutionStatus::Success => "success",
            ExecutionStatus::Error => "error",
            ExecutionStatus::Timeout => "timeout",
        }
    }

    pub fn from_result(is_success: bool) -> Self {
        if is_success {
            ExecutionStatus::Success
        } else {
            ExecutionStatus::Error
        }
    }
}
