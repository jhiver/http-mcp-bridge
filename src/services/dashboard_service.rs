use crate::models::{ExecutionHistory, ToolUsageStats};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Clone, Debug)]
pub struct DashboardService {
    pool: SqlitePool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DashboardStats {
    pub total_servers: i64,
    pub total_toolkits: i64,
    pub total_tools: i64,
    pub total_instances: i64,
    pub executions_24h: i64,
    pub executions_7d: i64,
    pub executions_all_time: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerStats {
    pub server_id: i64,
    pub server_name: String,
    pub instance_count: i64,
    pub executions_24h: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub success_rate: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecentExecution {
    pub id: i64,
    pub server_name: String,
    pub instance_name: String,
    pub tool_name: String,
    pub started_at: String,
    pub duration_ms: Option<i64>,
    pub status: String,
    pub http_status_code: Option<i64>,
}

impl DashboardService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Get overview statistics for a user's dashboard
    pub async fn get_dashboard_stats(&self, user_id: i64) -> Result<DashboardStats> {
        // Count servers
        let total_servers =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM servers WHERE user_id = ?")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;

        // Count toolkits
        let total_toolkits =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM toolkits WHERE user_id = ?")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;

        // Count tools (across user's toolkits)
        let total_tools = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*) FROM tools
            WHERE toolkit_id IN (SELECT id FROM toolkits WHERE user_id = ?)
            "#,
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;

        // Count tool instances (across user's servers)
        let total_instances = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*) FROM tool_instances
            WHERE server_id IN (SELECT id FROM servers WHERE user_id = ?)
            "#,
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;

        // Count executions in last 24 hours
        let executions_24h = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*) FROM execution_history eh
            INNER JOIN servers s ON eh.server_id = s.id
            WHERE s.user_id = ?
            AND eh.started_at >= datetime('now', '-24 hours')
            "#,
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;

        // Count executions in last 7 days
        let executions_7d = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*) FROM execution_history eh
            INNER JOIN servers s ON eh.server_id = s.id
            WHERE s.user_id = ?
            AND eh.started_at >= datetime('now', '-7 days')
            "#,
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;

        // Count all-time executions
        let executions_all_time = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*) FROM execution_history eh
            INNER JOIN servers s ON eh.server_id = s.id
            WHERE s.user_id = ?
            "#,
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(DashboardStats {
            total_servers,
            total_toolkits,
            total_tools,
            total_instances,
            executions_24h,
            executions_7d,
            executions_all_time,
        })
    }

    /// Get statistics for all servers
    pub async fn get_server_stats(&self, user_id: i64) -> Result<Vec<ServerStats>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                s.id as "server_id!: i64",
                s.name as "server_name!: String",
                CAST(COUNT(DISTINCT ti.id) AS INTEGER) as "instance_count!: i64",
                CAST(COUNT(CASE WHEN eh.started_at >= datetime('now', '-24 hours')
                      THEN 1 END) AS INTEGER) as "executions_24h!: i64",
                CAST(COUNT(CASE WHEN eh.status = 'success'
                      AND eh.started_at >= datetime('now', '-24 hours')
                      THEN 1 END) AS INTEGER) as "success_count!: i64",
                CAST(COUNT(CASE WHEN eh.status = 'error'
                      AND eh.started_at >= datetime('now', '-24 hours')
                      THEN 1 END) AS INTEGER) as "error_count!: i64"
            FROM servers s
            LEFT JOIN tool_instances ti ON ti.server_id = s.id
            LEFT JOIN execution_history eh ON eh.server_id = s.id
            WHERE s.user_id = ?
            GROUP BY s.id, s.name
            ORDER BY s.name
            "#,
            user_id
        )
        .fetch_all(&self.pool)
        .await?;

        let stats = rows
            .into_iter()
            .map(|row| {
                let executions_24h = row.executions_24h;
                let success_count = row.success_count;
                let error_count = row.error_count;
                let success_rate = if executions_24h > 0 {
                    (success_count as f64 / executions_24h as f64) * 100.0
                } else {
                    0.0
                };

                ServerStats {
                    server_id: row.server_id,
                    server_name: row.server_name,
                    instance_count: row.instance_count,
                    executions_24h,
                    success_count,
                    error_count,
                    success_rate,
                }
            })
            .collect();

        Ok(stats)
    }

    /// Get most used tools (by execution count)
    pub async fn get_most_used_tools(
        &self,
        user_id: i64,
        limit: i64,
    ) -> Result<Vec<ToolUsageStats>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                ti.id as "instance_id!: i64",
                ti.instance_name as "instance_name!: String",
                t.name as "tool_name!: String",
                CAST(COUNT(*) AS INTEGER) as "execution_count!: i64",
                CAST(SUM(CASE WHEN eh.status = 'success' THEN 1 ELSE 0 END) AS INTEGER) as "success_count!: i64",
                CAST(SUM(CASE WHEN eh.status = 'error' THEN 1 ELSE 0 END) AS INTEGER) as "error_count!: i64",
                AVG(eh.duration_ms) as "avg_duration_ms?: f64"
            FROM execution_history eh
            INNER JOIN tool_instances ti ON eh.instance_id = ti.id
            INNER JOIN tools t ON eh.tool_id = t.id
            INNER JOIN servers s ON eh.server_id = s.id
            WHERE s.user_id = ?
            GROUP BY ti.id, ti.instance_name, t.name
            ORDER BY COUNT(*) DESC
            LIMIT ?
            "#,
            user_id,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        let stats = rows
            .into_iter()
            .map(|row| ToolUsageStats {
                instance_id: row.instance_id,
                instance_name: row.instance_name,
                tool_name: row.tool_name,
                execution_count: row.execution_count,
                success_count: row.success_count,
                error_count: row.error_count,
                avg_duration_ms: row.avg_duration_ms,
            })
            .collect();

        Ok(stats)
    }

    /// Get recent executions with joined data
    pub async fn get_recent_executions(
        &self,
        user_id: i64,
        limit: i64,
    ) -> Result<Vec<RecentExecution>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                eh.id as "id!: i64",
                s.name as "server_name!: String",
                ti.instance_name as "instance_name!: String",
                t.name as "tool_name!: String",
                eh.started_at as "started_at!: String",
                eh.duration_ms,
                eh.status as "status!: String",
                eh.http_status_code
            FROM execution_history eh
            INNER JOIN servers s ON eh.server_id = s.id
            INNER JOIN tool_instances ti ON eh.instance_id = ti.id
            INNER JOIN tools t ON eh.tool_id = t.id
            WHERE s.user_id = ?
            ORDER BY eh.started_at DESC
            LIMIT ?
            "#,
            user_id,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        let executions = rows
            .into_iter()
            .map(|row| RecentExecution {
                id: row.id,
                server_name: row.server_name,
                instance_name: row.instance_name,
                tool_name: row.tool_name,
                started_at: row.started_at,
                duration_ms: row.duration_ms,
                status: row.status,
                http_status_code: row.http_status_code,
            })
            .collect();

        Ok(executions)
    }

    /// Get detailed execution by ID
    pub async fn get_execution_detail(
        &self,
        execution_id: i64,
    ) -> Result<Option<ExecutionHistory>> {
        ExecutionHistory::get_by_id(&self.pool, execution_id).await
    }
}
