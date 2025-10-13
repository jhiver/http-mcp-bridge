use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Server {
    pub id: Option<i64>,
    pub uuid: String,
    pub user_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub access_level: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateServerForm {
    pub name: String,
    pub description: String,
    pub csrf_token: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateServerForm {
    pub name: String,
    pub description: String,
    pub csrf_token: String,
}

impl Server {
    pub async fn create(
        pool: &SqlitePool,
        user_id: i64,
        name: &str,
        description: Option<&str>,
    ) -> Result<i64> {
        let uuid = Uuid::new_v4().to_string();
        let result = sqlx::query!(
            r#"
            INSERT INTO servers (uuid, user_id, name, description)
            VALUES (?, ?, ?, ?)
            "#,
            uuid,
            user_id,
            name,
            description
        )
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    pub async fn create_with_toolkits(
        pool: &SqlitePool,
        user_id: i64,
        form: CreateServerForm,
    ) -> Result<i64> {
        // Simply create the server without any toolkit imports
        let uuid = Uuid::new_v4().to_string();
        let result = sqlx::query!(
            r#"
            INSERT INTO servers (uuid, user_id, name, description)
            VALUES (?, ?, ?, ?)
            "#,
            uuid,
            user_id,
            form.name,
            form.description
        )
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    pub async fn get_by_id(pool: &SqlitePool, id: i64) -> Result<Option<Self>> {
        let server = sqlx::query_as!(
            Server,
            r#"SELECT id, uuid as "uuid!", user_id, name, description, access_level, created_at, updated_at FROM servers WHERE id = ?"#,
            id
        )
        .fetch_optional(pool)
        .await?;

        Ok(server)
    }

    pub async fn get_by_uuid(pool: &SqlitePool, uuid: &str) -> Result<Option<Self>> {
        let server = sqlx::query_as!(
            Server,
            r#"SELECT id, uuid as "uuid!", user_id, name, description, access_level, created_at, updated_at FROM servers WHERE uuid = ?"#,
            uuid
        )
        .fetch_optional(pool)
        .await?;

        Ok(server)
    }

    pub async fn get_by_id_and_user(
        pool: &SqlitePool,
        id: i64,
        user_id: i64,
    ) -> Result<Option<Self>> {
        let server = sqlx::query_as!(
            Server,
            r#"SELECT id, uuid as "uuid!", user_id, name, description, access_level, created_at, updated_at FROM servers WHERE id = ? AND user_id = ?"#,
            id,
            user_id
        )
        .fetch_optional(pool)
        .await?;

        Ok(server)
    }

    pub async fn list_by_user(pool: &SqlitePool, user_id: i64) -> Result<Vec<Self>> {
        let servers = sqlx::query_as!(
            Server,
            r#"SELECT id, uuid as "uuid!", user_id, name, description, access_level, created_at, updated_at FROM servers WHERE user_id = ? ORDER BY created_at DESC"#,
            user_id
        )
        .fetch_all(pool)
        .await?;

        Ok(servers)
    }

    pub async fn update(
        pool: &SqlitePool,
        id: i64,
        user_id: i64,
        form: UpdateServerForm,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE servers
            SET name = ?, description = ?, updated_at = unixepoch()
            WHERE id = ? AND user_id = ?
            "#,
            form.name,
            form.description,
            id,
            user_id
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn delete(pool: &SqlitePool, id: i64, user_id: i64) -> Result<()> {
        sqlx::query!(
            "DELETE FROM servers WHERE id = ? AND user_id = ?",
            id,
            user_id
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}

// Server summary with toolkit count
#[derive(Debug, Serialize)]
pub struct ServerSummary {
    pub id: Option<i64>,
    pub uuid: String,
    pub name: String,
    pub description: Option<String>,
    pub toolkit_count: i64,
    pub instance_count: i64,
    pub created_at: Option<i64>,
}

impl ServerSummary {
    pub async fn list_by_user(pool: &SqlitePool, user_id: i64) -> Result<Vec<Self>> {
        let summaries = sqlx::query_as!(
            ServerSummary,
            r#"
            SELECT
                s.id,
                s.uuid as "uuid!",
                s.name,
                s.description,
                CAST(COUNT(DISTINCT st.toolkit_id) AS INTEGER) as "toolkit_count!: i64",
                CAST(COUNT(DISTINCT ti.id) AS INTEGER) as "instance_count!: i64",
                s.created_at
            FROM servers s
            LEFT JOIN server_toolkits st ON s.id = st.server_id
            LEFT JOIN tool_instances ti ON s.id = ti.server_id
            WHERE s.user_id = ?
            GROUP BY s.id
            ORDER BY s.created_at DESC
            "#,
            user_id
        )
        .fetch_all(pool)
        .await?;

        Ok(summaries)
    }
}

// Server toolkit info
#[derive(Debug, Serialize)]
pub struct ServerToolkit {
    pub toolkit_id: Option<i64>,
    pub title: String,
    pub description: Option<String>,
    pub tools_count: i64,
}

impl ServerToolkit {
    pub async fn list_by_server(pool: &SqlitePool, server_id: i64) -> Result<Vec<Self>> {
        let toolkits = sqlx::query_as!(
            ServerToolkit,
            r#"
            SELECT
                t.id as toolkit_id,
                t.title,
                t.description,
                CAST(COUNT(tool.id) AS INTEGER) as "tools_count!: i64"
            FROM server_toolkits st
            JOIN toolkits t ON st.toolkit_id = t.id
            LEFT JOIN tools tool ON t.id = tool.toolkit_id
            WHERE st.server_id = ?
            GROUP BY t.id
            ORDER BY t.title
            "#,
            server_id
        )
        .fetch_all(pool)
        .await?;

        Ok(toolkits)
    }
}
