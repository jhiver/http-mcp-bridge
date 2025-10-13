use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct ExecutionHistory {
    pub id: Option<i64>,
    pub server_id: i64,
    pub instance_id: i64,
    pub tool_id: i64,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub status: String, // success, error, timeout
    pub http_status_code: Option<i64>,
    pub error_message: Option<String>,
    pub input_params: Option<String>,     // JSON
    pub response_body: Option<String>,    // HTTP response body
    pub response_headers: Option<String>, // JSON
    pub request_url: Option<String>,
    pub request_method: Option<String>,
    pub response_size_bytes: Option<i64>,
    pub transport: Option<String>, // http, sse
    pub created_at: Option<String>,
}

impl ExecutionHistory {
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        pool: &SqlitePool,
        server_id: i64,
        instance_id: i64,
        tool_id: i64,
        started_at: &str,
        completed_at: Option<&str>,
        duration_ms: Option<i64>,
        status: &str,
        http_status_code: Option<i64>,
        error_message: Option<&str>,
        input_params: Option<&str>,
        response_body: Option<&str>,
        response_headers: Option<&str>,
        request_url: Option<&str>,
        request_method: Option<&str>,
        response_size_bytes: Option<i64>,
        transport: Option<&str>,
    ) -> Result<i64> {
        let result = sqlx::query!(
            r#"
            INSERT INTO execution_history (
                server_id, instance_id, tool_id, started_at, completed_at, duration_ms,
                status, http_status_code, error_message, input_params, response_body,
                response_headers, request_url, request_method, response_size_bytes, transport
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            server_id,
            instance_id,
            tool_id,
            started_at,
            completed_at,
            duration_ms,
            status,
            http_status_code,
            error_message,
            input_params,
            response_body,
            response_headers,
            request_url,
            request_method,
            response_size_bytes,
            transport
        )
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    pub async fn get_by_id(pool: &SqlitePool, id: i64) -> Result<Option<Self>> {
        let record = sqlx::query_as!(
            ExecutionHistory,
            "SELECT * FROM execution_history WHERE id = ?",
            id
        )
        .fetch_optional(pool)
        .await?;

        Ok(record)
    }

    pub async fn list_by_server(
        pool: &SqlitePool,
        server_id: i64,
        limit: i64,
    ) -> Result<Vec<Self>> {
        let records = sqlx::query_as!(
            ExecutionHistory,
            r#"
            SELECT * FROM execution_history
            WHERE server_id = ?
            ORDER BY started_at DESC
            LIMIT ?
            "#,
            server_id,
            limit
        )
        .fetch_all(pool)
        .await?;

        Ok(records)
    }

    pub async fn list_by_instance(
        pool: &SqlitePool,
        instance_id: i64,
        limit: i64,
    ) -> Result<Vec<Self>> {
        let records = sqlx::query_as!(
            ExecutionHistory,
            r#"
            SELECT * FROM execution_history
            WHERE instance_id = ?
            ORDER BY started_at DESC
            LIMIT ?
            "#,
            instance_id,
            limit
        )
        .fetch_all(pool)
        .await?;

        Ok(records)
    }

    pub async fn list_recent(pool: &SqlitePool, user_id: i64, limit: i64) -> Result<Vec<Self>> {
        let records = sqlx::query_as!(
            ExecutionHistory,
            r#"
            SELECT eh.* FROM execution_history eh
            INNER JOIN servers s ON eh.server_id = s.id
            WHERE s.user_id = ?
            ORDER BY eh.started_at DESC
            LIMIT ?
            "#,
            user_id,
            limit
        )
        .fetch_all(pool)
        .await?;

        Ok(records)
    }

    pub async fn count_by_server(
        pool: &SqlitePool,
        server_id: i64,
        hours: Option<i64>,
    ) -> Result<i64> {
        let count = if let Some(hours) = hours {
            sqlx::query_scalar::<_, i64>(
                r#"
                SELECT COUNT(*) FROM execution_history
                WHERE server_id = ?
                AND started_at >= datetime('now', '-' || ? || ' hours')
                "#,
            )
            .bind(server_id)
            .bind(hours)
            .fetch_one(pool)
            .await?
        } else {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM execution_history WHERE server_id = ?",
            )
            .bind(server_id)
            .fetch_one(pool)
            .await?
        };

        Ok(count)
    }

    pub async fn count_by_instance(
        pool: &SqlitePool,
        instance_id: i64,
        hours: Option<i64>,
    ) -> Result<i64> {
        let count = if let Some(hours) = hours {
            sqlx::query_scalar::<_, i64>(
                r#"
                SELECT COUNT(*) FROM execution_history
                WHERE instance_id = ?
                AND started_at >= datetime('now', '-' || ? || ' hours')
                "#,
            )
            .bind(instance_id)
            .bind(hours)
            .fetch_one(pool)
            .await?
        } else {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM execution_history WHERE instance_id = ?",
            )
            .bind(instance_id)
            .fetch_one(pool)
            .await?
        };

        Ok(count)
    }

    pub async fn count_by_status(pool: &SqlitePool, server_id: i64, status: &str) -> Result<i64> {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*) FROM execution_history
            WHERE server_id = ? AND status = ?
            "#,
        )
        .bind(server_id)
        .bind(status)
        .fetch_one(pool)
        .await?;

        Ok(count)
    }

    pub async fn get_average_duration(pool: &SqlitePool, instance_id: i64) -> Result<Option<f64>> {
        let avg = sqlx::query_scalar::<_, Option<f64>>(
            r#"
            SELECT AVG(duration_ms) FROM execution_history
            WHERE instance_id = ? AND duration_ms IS NOT NULL
            "#,
        )
        .bind(instance_id)
        .fetch_one(pool)
        .await?;

        Ok(avg)
    }

    pub async fn delete_by_server(pool: &SqlitePool, server_id: i64) -> Result<()> {
        sqlx::query!(
            "DELETE FROM execution_history WHERE server_id = ?",
            server_id
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn delete_older_than_days(pool: &SqlitePool, days: i64) -> Result<u64> {
        let result = sqlx::query!(
            r#"
            DELETE FROM execution_history
            WHERE started_at < datetime('now', '-' || ? || ' days')
            "#,
            days
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }
}

// For dashboard statistics
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolUsageStats {
    pub instance_id: i64,
    pub instance_name: String,
    pub tool_name: String,
    pub execution_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub avg_duration_ms: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DailyExecutionStats {
    pub date: String,
    pub execution_count: i64,
    pub success_count: i64,
    pub error_count: i64,
}
