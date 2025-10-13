use crate::error::Result;
use crate::models::{
    CreateToolkitRequest, Toolkit, ToolkitSummary, ToolkitWithStats, UpdateToolkitRequest,
};
use async_trait::async_trait;
use sqlx::SqlitePool;

#[async_trait]
pub trait ToolkitRepository: Send + Sync {
    async fn create(&self, user_id: i64, request: CreateToolkitRequest) -> Result<i64>;
    async fn get_by_id(&self, id: i64, user_id: i64) -> Result<Option<Toolkit>>;
    async fn list_by_user(&self, user_id: i64) -> Result<Vec<Toolkit>>;
    async fn list_summaries_by_user(&self, user_id: i64) -> Result<Vec<ToolkitSummary>>;
    async fn update(&self, id: i64, user_id: i64, request: UpdateToolkitRequest) -> Result<bool>;
    async fn delete(&self, id: i64, user_id: i64) -> Result<bool>;
    async fn verify_ownership(&self, id: i64, user_id: i64) -> Result<bool>;

    // New methods for public toolkit browsing and cloning
    async fn list_public_toolkits(&self) -> Result<Vec<ToolkitWithStats>>;
    async fn get_public_toolkit(&self, id: i64) -> Result<Option<Toolkit>>;
    async fn clone_toolkit(
        &self,
        original_id: i64,
        new_owner_id: i64,
        new_title: String,
    ) -> Result<i64>;
    async fn increment_clone_count(&self, toolkit_id: i64) -> Result<()>;
}

pub struct SqliteToolkitRepository {
    pool: SqlitePool,
}

impl SqliteToolkitRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ToolkitRepository for SqliteToolkitRepository {
    async fn create(&self, user_id: i64, request: CreateToolkitRequest) -> Result<i64> {
        let result = sqlx::query!(
            r#"
            INSERT INTO toolkits (user_id, title, description, visibility)
            VALUES (?, ?, ?, ?)
            "#,
            user_id,
            request.title,
            request.description,
            request.visibility
        )
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    async fn get_by_id(&self, id: i64, user_id: i64) -> Result<Option<Toolkit>> {
        let row = sqlx::query!(
            r#"
            SELECT id, user_id, title, description, visibility, parent_toolkit_id, clone_count, created_at, updated_at
            FROM toolkits
            WHERE id = ? AND user_id = ?
            "#,
            id,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Toolkit {
            id: r.id,
            user_id: r.user_id,
            title: r.title,
            description: r.description,
            visibility: r.visibility.unwrap_or_else(|| "private".to_string()),
            parent_toolkit_id: r.parent_toolkit_id,
            clone_count: r.clone_count as i32,
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

    async fn list_by_user(&self, user_id: i64) -> Result<Vec<Toolkit>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, user_id, title, description, visibility, parent_toolkit_id, clone_count, created_at, updated_at
            FROM toolkits
            WHERE user_id = ?
            ORDER BY created_at DESC
            "#,
            user_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Toolkit {
                id: r.id.unwrap_or(0),
                user_id: r.user_id,
                title: r.title,
                description: r.description,
                visibility: r.visibility.unwrap_or_else(|| "private".to_string()),
                parent_toolkit_id: r.parent_toolkit_id,
                clone_count: r.clone_count as i32,
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

    async fn list_summaries_by_user(&self, user_id: i64) -> Result<Vec<ToolkitSummary>> {
        let summaries = sqlx::query!(
            r#"
            SELECT
                t.id,
                t.title,
                COALESCE(t.description, '') as "description!: String",
                CAST(COUNT(tl.id) AS INTEGER) as "tools_count!: i64"
            FROM toolkits t
            LEFT JOIN tools tl ON t.id = tl.toolkit_id
            WHERE t.user_id = ?
            GROUP BY t.id, t.title, t.description
            ORDER BY t.created_at DESC
            "#,
            user_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(summaries
            .into_iter()
            .map(|row| ToolkitSummary {
                id: row.id.unwrap_or(0),
                title: row.title,
                description: row.description,
                tools_count: row.tools_count as i32,
            })
            .collect())
    }

    async fn update(&self, id: i64, user_id: i64, request: UpdateToolkitRequest) -> Result<bool> {
        let result = sqlx::query!(
            r#"
            UPDATE toolkits
            SET title = ?, description = ?, visibility = ?, updated_at = unixepoch()
            WHERE id = ? AND user_id = ?
            "#,
            request.title,
            request.description,
            request.visibility,
            id,
            user_id
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn delete(&self, id: i64, user_id: i64) -> Result<bool> {
        let result = sqlx::query!(
            "DELETE FROM toolkits WHERE id = ? AND user_id = ?",
            id,
            user_id
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn verify_ownership(&self, id: i64, user_id: i64) -> Result<bool> {
        let result = sqlx::query!(
            "SELECT COUNT(*) as count FROM toolkits WHERE id = ? AND user_id = ?",
            id,
            user_id
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(result.count > 0)
    }

    async fn list_public_toolkits(&self) -> Result<Vec<ToolkitWithStats>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                t.id, t.user_id, t.title, t.description, t.visibility,
                t.parent_toolkit_id, t.clone_count, t.created_at,
                u.email as owner_email,
                CAST(COUNT(DISTINCT tl.id) AS INTEGER) as "tools_count!: i64"
            FROM toolkits t
            LEFT JOIN tools tl ON t.id = tl.toolkit_id
            LEFT JOIN users u ON t.user_id = u.id
            WHERE t.visibility = 'public'
            GROUP BY t.id, t.user_id, t.title, t.description, t.visibility,
                     t.parent_toolkit_id, t.clone_count, t.created_at, u.email
            ORDER BY t.clone_count DESC, t.created_at DESC
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ToolkitWithStats {
                id: r.id.unwrap_or(0),
                user_id: r.user_id,
                title: r.title,
                description: r.description,
                visibility: r.visibility,
                parent_toolkit_id: r.parent_toolkit_id,
                clone_count: r.clone_count as i32,
                tools_count: r.tools_count as i32,
                owner_email: r.owner_email.unwrap_or_else(|| "Unknown".to_string()),
                created_at: r
                    .created_at
                    .map(|dt| {
                        chrono::DateTime::from_timestamp(dt, 0)
                            .map(|ts| ts.naive_utc())
                            .unwrap_or_default()
                    })
                    .unwrap_or_default(),
            })
            .collect())
    }

    async fn get_public_toolkit(&self, id: i64) -> Result<Option<Toolkit>> {
        let row = sqlx::query!(
            r#"
            SELECT id, user_id, title, description, visibility, parent_toolkit_id, clone_count, created_at, updated_at
            FROM toolkits
            WHERE id = ? AND visibility = 'public'
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Toolkit {
            id: r.id,
            user_id: r.user_id,
            title: r.title,
            description: r.description,
            visibility: r.visibility.unwrap_or_else(|| "private".to_string()),
            parent_toolkit_id: r.parent_toolkit_id,
            clone_count: r.clone_count as i32,
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

    async fn clone_toolkit(
        &self,
        original_id: i64,
        new_owner_id: i64,
        new_title: String,
    ) -> Result<i64> {
        // Start a transaction to ensure atomicity
        let mut tx = self.pool.begin().await?;

        // Create the new toolkit
        let new_toolkit_id = sqlx::query!(
            r#"
            INSERT INTO toolkits (user_id, title, description, visibility, parent_toolkit_id, clone_count)
            SELECT ?, ?, description, visibility, ?, 0
            FROM toolkits
            WHERE id = ?
            "#,
            new_owner_id,
            new_title,
            original_id,
            original_id
        )
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();

        // Copy all tools from the original toolkit
        sqlx::query!(
            r#"
            INSERT INTO tools (toolkit_id, name, description, method, url, headers, body, timeout_ms)
            SELECT ?, name, description, method, url, headers, body, timeout_ms
            FROM tools
            WHERE toolkit_id = ?
            "#,
            new_toolkit_id,
            original_id
        )
        .execute(&mut *tx)
        .await?;

        // Commit the transaction
        tx.commit().await?;

        Ok(new_toolkit_id)
    }

    async fn increment_clone_count(&self, toolkit_id: i64) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE toolkits
            SET clone_count = COALESCE(clone_count, 0) + 1
            WHERE id = ?
            "#,
            toolkit_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{create_test_pool, create_test_user};

    #[sqlx::test]
    async fn test_toolkit_crud() {
        let pool = create_test_pool().await;
        let repo = SqliteToolkitRepository::new(pool.clone());
        let user_id = create_test_user(&pool, "test@example.com", "password")
            .await
            .unwrap();

        // Create
        let request = CreateToolkitRequest {
            title: "Test Toolkit".to_string(),
            description: Some("Test description".to_string()),
            visibility: "private".to_string(),
        };
        let toolkit_id = repo.create(user_id, request).await.unwrap();
        assert!(toolkit_id > 0);

        // Read
        let toolkit = repo.get_by_id(toolkit_id, user_id).await.unwrap().unwrap();
        assert_eq!(toolkit.title, "Test Toolkit");

        // Update
        let update_request = UpdateToolkitRequest {
            title: "Updated Toolkit".to_string(),
            description: Some("Updated description".to_string()),
            visibility: "public".to_string(),
        };
        let updated = repo
            .update(toolkit_id, user_id, update_request)
            .await
            .unwrap();
        assert!(updated);

        // List
        let toolkits = repo.list_by_user(user_id).await.unwrap();
        assert_eq!(toolkits.len(), 1);
        assert_eq!(toolkits[0].title, "Updated Toolkit");

        // Delete
        let deleted = repo.delete(toolkit_id, user_id).await.unwrap();
        assert!(deleted);

        // Verify deleted
        let toolkit = repo.get_by_id(toolkit_id, user_id).await.unwrap();
        assert!(toolkit.is_none());
    }
}
