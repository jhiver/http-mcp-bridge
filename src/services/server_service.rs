use crate::models::{
    CreateServerForm, GlobalsForm, Server, ServerGlobal, ServerSummary, ServerToolkit,
    UpdateServerForm,
};
use crate::services::SecretsManager;
use anyhow::Result;
use sqlx::SqlitePool;

#[derive(Clone)]
pub struct ServerService {
    pool: SqlitePool,
    secrets: SecretsManager,
}

impl ServerService {
    pub fn new(pool: SqlitePool, secrets: SecretsManager) -> Self {
        Self { pool, secrets }
    }

    // Server CRUD operations
    pub async fn create_server(&self, user_id: i64, form: CreateServerForm) -> Result<i64> {
        Server::create_with_toolkits(&self.pool, user_id, form).await
    }

    pub async fn get_server(&self, server_id: i64, user_id: i64) -> Result<Option<Server>> {
        Server::get_by_id_and_user(&self.pool, server_id, user_id).await
    }

    pub async fn list_servers(&self, user_id: i64) -> Result<Vec<ServerSummary>> {
        ServerSummary::list_by_user(&self.pool, user_id).await
    }

    pub async fn update_server(
        &self,
        server_id: i64,
        user_id: i64,
        form: UpdateServerForm,
    ) -> Result<()> {
        Server::update(&self.pool, server_id, user_id, form).await
    }

    pub async fn delete_server(&self, server_id: i64, user_id: i64) -> Result<()> {
        Server::delete(&self.pool, server_id, user_id).await
    }

    pub async fn update_server_access(
        &self,
        server_id: i64,
        user_id: i64,
        access_level: &str,
    ) -> Result<()> {
        // Validate access level
        if !["private", "organization", "public"].contains(&access_level) {
            anyhow::bail!("Invalid access level: {}", access_level);
        }

        // Verify ownership
        Server::get_by_id_and_user(&self.pool, server_id, user_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Server not found or unauthorized"))?;

        sqlx::query!(
            r#"
            UPDATE servers
            SET access_level = ?, updated_at = unixepoch()
            WHERE id = ? AND user_id = ?
            "#,
            access_level,
            server_id,
            user_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // Server toolkits
    pub async fn get_server_toolkits(&self, server_id: i64) -> Result<Vec<ServerToolkit>> {
        ServerToolkit::list_by_server(&self.pool, server_id).await
    }

    pub async fn add_toolkit_to_server(
        &self,
        server_id: i64,
        toolkit_id: i64,
        user_id: i64,
    ) -> Result<()> {
        // Verify ownership
        let owns_server = sqlx::query!(
            "SELECT 1 as result FROM servers WHERE id = ? AND user_id = ?",
            server_id,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?
        .is_some();

        let owns_toolkit = sqlx::query!(
            "SELECT 1 as result FROM toolkits WHERE id = ? AND user_id = ?",
            toolkit_id,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?
        .is_some();

        if !owns_server || !owns_toolkit {
            anyhow::bail!("Unauthorized");
        }

        sqlx::query!(
            "INSERT OR IGNORE INTO server_toolkits (server_id, toolkit_id) VALUES (?, ?)",
            server_id,
            toolkit_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn remove_toolkit_from_server(
        &self,
        server_id: i64,
        toolkit_id: i64,
        user_id: i64,
    ) -> Result<()> {
        // Verify ownership
        let owns_server = sqlx::query!(
            "SELECT 1 as result FROM servers WHERE id = ? AND user_id = ?",
            server_id,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?
        .is_some();

        if !owns_server {
            anyhow::bail!("Unauthorized");
        }

        sqlx::query!(
            "DELETE FROM server_toolkits WHERE server_id = ? AND toolkit_id = ?",
            server_id,
            toolkit_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // Server globals management
    pub async fn get_server_globals(&self, server_id: i64) -> Result<Vec<ServerGlobal>> {
        ServerGlobal::list_by_server(&self.pool, server_id).await
    }

    pub async fn get_server_globals_decrypted(&self, server_id: i64) -> Result<Vec<ServerGlobal>> {
        let mut globals = ServerGlobal::list_by_server(&self.pool, server_id).await?;

        for global in &mut globals {
            if global.is_secret.unwrap_or(false) {
                global.value = self.secrets.decrypt(&global.value)?;
            }
        }

        Ok(globals)
    }

    pub async fn save_server_globals(
        &self,
        server_id: i64,
        user_id: i64,
        form: GlobalsForm,
    ) -> Result<()> {
        // Verify ownership
        let owns = sqlx::query!(
            "SELECT 1 as result FROM servers WHERE id = ? AND user_id = ?",
            server_id,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?
        .is_some();

        if !owns {
            anyhow::bail!("Unauthorized");
        }

        let mut tx = self.pool.begin().await?;

        // Clear existing globals
        sqlx::query!("DELETE FROM server_globals WHERE server_id = ?", server_id)
            .execute(&mut *tx)
            .await?;

        // Save variables
        for (key, value) in form.var_keys.iter().zip(&form.var_values) {
            if !key.is_empty() && !value.is_empty() {
                sqlx::query!(
                    r#"
                    INSERT INTO server_globals (server_id, key, value, is_secret)
                    VALUES (?, ?, ?, false)
                    "#,
                    server_id,
                    key,
                    value
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        // Save secrets (encrypted)
        for (key, value) in form.secret_keys.iter().zip(&form.secret_values) {
            if !key.is_empty() && !value.is_empty() {
                let encrypted = self.secrets.encrypt(value)?;

                sqlx::query!(
                    r#"
                    INSERT INTO server_globals (server_id, key, value, is_secret)
                    VALUES (?, ?, ?, true)
                    "#,
                    server_id,
                    key,
                    encrypted
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(())
    }

    // Check if user owns a server
    pub async fn user_owns_server(&self, server_id: i64, user_id: i64) -> Result<bool> {
        let owns = sqlx::query!(
            "SELECT 1 as result FROM servers WHERE id = ? AND user_id = ?",
            server_id,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?
        .is_some();

        Ok(owns)
    }
}
