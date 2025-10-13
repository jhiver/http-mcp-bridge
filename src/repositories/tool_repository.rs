use crate::error::Result;
use crate::models::{CreateToolRequest, Tool, UpdateToolRequest};
use async_trait::async_trait;
use sqlx::SqlitePool;

#[async_trait]
pub trait ToolRepository: Send + Sync {
    async fn create(&self, toolkit_id: i64, request: CreateToolRequest) -> Result<i64>;
    async fn get_by_id(&self, id: i64) -> Result<Option<Tool>>;
    async fn list_by_toolkit(&self, toolkit_id: i64) -> Result<Vec<Tool>>;
    async fn update(&self, id: i64, request: UpdateToolRequest) -> Result<bool>;
    async fn delete(&self, id: i64) -> Result<bool>;
}

pub struct SqliteToolRepository {
    pool: SqlitePool,
}

impl SqliteToolRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ToolRepository for SqliteToolRepository {
    async fn create(&self, toolkit_id: i64, request: CreateToolRequest) -> Result<i64> {
        // Insert tool (no transaction needed since parameters are auto-extracted)
        let tool_id = sqlx::query!(
            r#"
            INSERT INTO tools (toolkit_id, name, description, method, url, headers, body, timeout_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            toolkit_id,
            request.name,
            request.description,
            request.method,
            request.url,
            request.headers,
            request.body,
            request.timeout_ms
        )
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        Ok(tool_id)
    }

    async fn get_by_id(&self, id: i64) -> Result<Option<Tool>> {
        let row = sqlx::query!(
            r#"
            SELECT id, toolkit_id, name, description, method, url, headers, body, timeout_ms, created_at, updated_at
            FROM tools
            WHERE id = ?
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Tool {
            id: r.id,
            toolkit_id: r.toolkit_id,
            name: r.name,
            description: r.description,
            method: r.method.unwrap_or_else(|| "GET".to_string()),
            url: r.url,
            headers: r.headers,
            body: r.body,
            timeout_ms: r.timeout_ms.unwrap_or(30000) as i32,
            created_at: r
                .created_at
                .map(|dt| {
                    chrono::DateTime::from_timestamp(dt, 0)
                        .map(|ts| ts.naive_utc())
                        .unwrap_or_default()
                })
                .unwrap_or_default(),
            updated_at: r
                .updated_at
                .map(|dt| {
                    chrono::DateTime::from_timestamp(dt, 0)
                        .map(|ts| ts.naive_utc())
                        .unwrap_or_default()
                })
                .unwrap_or_default(),
        }))
    }

    async fn list_by_toolkit(&self, toolkit_id: i64) -> Result<Vec<Tool>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, toolkit_id, name, description, method, url, headers, body, timeout_ms, created_at, updated_at
            FROM tools
            WHERE toolkit_id = ?
            ORDER BY created_at DESC
            "#,
            toolkit_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Tool {
                id: r.id.unwrap_or(0),
                toolkit_id: r.toolkit_id,
                name: r.name,
                description: r.description,
                method: r.method.unwrap_or_else(|| "GET".to_string()),
                url: r.url,
                headers: r.headers,
                body: r.body,
                timeout_ms: r.timeout_ms.unwrap_or(30000) as i32,
                created_at: r
                    .created_at
                    .map(|dt| {
                        chrono::DateTime::from_timestamp(dt, 0)
                            .map(|ts| ts.naive_utc())
                            .unwrap_or_default()
                    })
                    .unwrap_or_default(),
                updated_at: r
                    .updated_at
                    .map(|dt| {
                        chrono::DateTime::from_timestamp(dt, 0)
                            .map(|ts| ts.naive_utc())
                            .unwrap_or_default()
                    })
                    .unwrap_or_default(),
            })
            .collect())
    }

    async fn update(&self, id: i64, request: UpdateToolRequest) -> Result<bool> {
        // Update tool (no transaction needed since parameters are auto-extracted)
        let result = sqlx::query!(
            r#"
            UPDATE tools
            SET name = ?, description = ?, method = ?, url = ?, headers = ?, body = ?, timeout_ms = ?, updated_at = unixepoch()
            WHERE id = ?
            "#,
            request.name,
            request.description,
            request.method,
            request.url,
            request.headers,
            request.body,
            request.timeout_ms,
            id
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn delete(&self, id: i64) -> Result<bool> {
        // Parameters will cascade delete due to foreign key constraint
        let result = sqlx::query!("DELETE FROM tools WHERE id = ?", id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{create_test_pool, create_test_toolkit, create_test_user};

    #[sqlx::test]
    async fn test_tool_with_parameters() {
        let pool = create_test_pool().await;
        let repo = SqliteToolRepository::new(pool.clone());
        let user_id = create_test_user(&pool, "test@example.com", "password")
            .await
            .unwrap();
        let toolkit_id = create_test_toolkit(&pool, user_id, "Test Toolkit")
            .await
            .unwrap();

        // Create tool with parameters embedded in templates
        let request = CreateToolRequest {
            name: "Test Tool".to_string(),
            description: Some("Test description".to_string()),
            method: "GET".to_string(),
            url: Some(
                "https://api.example.com/{{url}}/data?timeout={{integer:timeout}}".to_string(),
            ),
            headers: Some("{}".to_string()),
            body: None,
            timeout_ms: 30000,
        };

        let tool_id = repo.create(toolkit_id, request).await.unwrap();
        assert!(tool_id > 0);

        // Get tool
        let tool = repo.get_by_id(tool_id).await.unwrap().unwrap();
        assert_eq!(tool.name, "Test Tool");

        // Tool should have URL with embedded parameters
        assert!(tool.url.as_ref().unwrap().contains("{{url}}"));
        assert!(tool.url.as_ref().unwrap().contains("{{integer:timeout}}"));

        // Update tool with different parameters in URL
        let update_request = UpdateToolRequest {
            name: "Updated Tool".to_string(),
            description: Some("Updated description".to_string()),
            method: "POST".to_string(),
            url: Some("https://api.example.com/{{string:host}}/endpoint".to_string()),
            headers: Some(r#"{"Authorization": "Bearer {{token}}"}"#.to_string()),
            body: Some(r#"{"data": "{{json:payload}}"}"#.to_string()),
            timeout_ms: 60000,
        };

        let updated = repo.update(tool_id, update_request).await.unwrap();
        assert!(updated);

        // Verify tool was updated
        let updated_tool = repo.get_by_id(tool_id).await.unwrap().unwrap();
        assert_eq!(updated_tool.name, "Updated Tool");
        assert!(updated_tool
            .url
            .as_ref()
            .unwrap()
            .contains("{{string:host}}"));
        assert!(updated_tool.headers.as_ref().unwrap().contains("{{token}}"));
        assert!(updated_tool
            .body
            .as_ref()
            .unwrap()
            .contains("{{json:payload}}"));

        // Delete tool
        let deleted = repo.delete(tool_id).await.unwrap();
        assert!(deleted);

        // Verify tool was deleted
        let deleted_tool = repo.get_by_id(tool_id).await.unwrap();
        assert!(deleted_tool.is_none());
    }
}
