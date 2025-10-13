use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ServerGlobal {
    pub id: Option<i64>,
    pub server_id: i64,
    pub key: String,
    pub value: String,
    pub is_secret: Option<bool>,
    pub created_at: Option<time::OffsetDateTime>,
    pub updated_at: Option<time::OffsetDateTime>,
}

#[derive(Debug, Deserialize)]
pub struct GlobalsForm {
    pub var_keys: Vec<String>,
    pub var_values: Vec<String>,
    pub secret_keys: Vec<String>,
    pub secret_values: Vec<String>,
    pub csrf_token: String,
}

impl ServerGlobal {
    pub async fn create(
        pool: &SqlitePool,
        server_id: i64,
        key: &str,
        value: &str,
        is_secret: bool,
    ) -> Result<i64> {
        let result = sqlx::query!(
            r#"
            INSERT INTO server_globals (server_id, key, value, is_secret)
            VALUES (?, ?, ?, ?)
            "#,
            server_id,
            key,
            value,
            is_secret
        )
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    pub async fn list_by_server(pool: &SqlitePool, server_id: i64) -> Result<Vec<Self>> {
        let globals = sqlx::query_as!(
            ServerGlobal,
            "SELECT * FROM server_globals WHERE server_id = ? ORDER BY key",
            server_id
        )
        .fetch_all(pool)
        .await?;

        Ok(globals)
    }

    pub async fn get_by_key(pool: &SqlitePool, server_id: i64, key: &str) -> Result<Option<Self>> {
        let global = sqlx::query_as!(
            ServerGlobal,
            "SELECT * FROM server_globals WHERE server_id = ? AND key = ?",
            server_id,
            key
        )
        .fetch_optional(pool)
        .await?;

        Ok(global)
    }

    pub async fn update_or_create(
        pool: &SqlitePool,
        server_id: i64,
        key: &str,
        value: &str,
        is_secret: bool,
    ) -> Result<()> {
        // Check if exists
        let exists = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT 1 FROM server_globals WHERE server_id = ? AND key = ?",
        )
        .bind(server_id)
        .bind(key)
        .fetch_optional(pool)
        .await?
        .is_some();

        if exists {
            // Update existing
            sqlx::query!(
                r#"
                UPDATE server_globals
                SET value = ?, is_secret = ?, updated_at = unixepoch()
                WHERE server_id = ? AND key = ?
                "#,
                value,
                is_secret,
                server_id,
                key
            )
            .execute(pool)
            .await?;
        } else {
            // Create new
            sqlx::query!(
                r#"
                INSERT INTO server_globals (server_id, key, value, is_secret)
                VALUES (?, ?, ?, ?)
                "#,
                server_id,
                key,
                value,
                is_secret
            )
            .execute(pool)
            .await?;
        }

        Ok(())
    }

    pub async fn delete(pool: &SqlitePool, server_id: i64, key: &str) -> Result<()> {
        sqlx::query!(
            "DELETE FROM server_globals WHERE server_id = ? AND key = ?",
            server_id,
            key
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn delete_all_by_server(pool: &SqlitePool, server_id: i64) -> Result<()> {
        sqlx::query!("DELETE FROM server_globals WHERE server_id = ?", server_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    // Get all non-secret globals as a hashmap
    pub async fn get_public_as_map(
        pool: &SqlitePool,
        server_id: i64,
    ) -> Result<std::collections::HashMap<String, String>> {
        let globals = sqlx::query!(
            "SELECT key, value FROM server_globals WHERE server_id = ? AND is_secret = false",
            server_id
        )
        .fetch_all(pool)
        .await?;

        let mut map = std::collections::HashMap::new();
        for global in globals {
            map.insert(global.key, global.value);
        }

        Ok(map)
    }
}
