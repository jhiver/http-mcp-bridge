use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ToolInstance {
    pub id: Option<i64>,
    pub server_id: i64,
    pub tool_id: i64,
    pub instance_name: String,
    pub description: Option<String>,
    pub created_at: Option<time::OffsetDateTime>,
    pub updated_at: Option<time::OffsetDateTime>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct InstanceParam {
    pub id: Option<i64>,
    pub instance_id: i64,
    pub param_name: String,
    pub source: String, // 'exposed', 'server', 'instance'
    pub value: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ConfigureInstanceForm {
    pub instance_name: String,
    pub description: Option<String>,
    pub tool_id: i64,
    #[serde(default)]
    pub param_configs: Vec<ParamConfig>,
    pub csrf_token: String,
}

#[derive(Debug, Deserialize)]
pub struct ParamConfig {
    pub name: String,
    pub source: String,
    pub value: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InstanceDetail {
    pub id: i64,
    pub server_id: i64,
    pub tool_id: i64,
    pub instance_name: String,
    pub description: Option<String>,
    pub tool_name: String,
    pub tool_description: Option<String>,
    pub toolkit_name: String,
    pub params: Vec<InstanceParam>,
    pub created_at: chrono::NaiveDateTime,
}

impl ToolInstance {
    pub async fn create(
        pool: &SqlitePool,
        server_id: i64,
        tool_id: i64,
        instance_name: &str,
        description: Option<&str>,
    ) -> Result<i64> {
        let result = sqlx::query!(
            r#"
            INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
            VALUES (?, ?, ?, ?)
            "#,
            server_id,
            tool_id,
            instance_name,
            description
        )
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    pub async fn create_with_config(
        pool: &SqlitePool,
        server_id: i64,
        form: ConfigureInstanceForm,
    ) -> Result<i64> {
        let mut tx = pool.begin().await?;

        // Inherit tool description if form description is empty or None
        let description = if form
            .description
            .as_ref()
            .is_none_or(|d| d.trim().is_empty())
        {
            let tool = sqlx::query!("SELECT description FROM tools WHERE id = ?", form.tool_id)
                .fetch_optional(&mut *tx)
                .await?;
            tool.and_then(|t| t.description)
        } else {
            form.description.clone()
        };

        // Create instance
        let instance_id = sqlx::query!(
            r#"
            INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
            VALUES (?, ?, ?, ?)
            "#,
            server_id,
            form.tool_id,
            form.instance_name,
            description
        )
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();

        // Configure parameters
        for param_config in form.param_configs {
            let value = if param_config.source == "instance" {
                param_config.value
            } else {
                None
            };

            sqlx::query!(
                r#"
                INSERT INTO instance_params (instance_id, param_name, source, value)
                VALUES (?, ?, ?, ?)
                "#,
                instance_id,
                param_config.name,
                param_config.source,
                value
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(instance_id)
    }

    pub async fn get_by_id(pool: &SqlitePool, id: i64) -> Result<Option<Self>> {
        let instance = sqlx::query_as!(
            ToolInstance,
            "SELECT * FROM tool_instances WHERE id = ?",
            id
        )
        .fetch_optional(pool)
        .await?;

        Ok(instance)
    }

    pub async fn get_detail(pool: &SqlitePool, id: i64) -> Result<Option<InstanceDetail>> {
        // Get instance with tool and toolkit info
        let instance_data = sqlx::query!(
            r#"
            SELECT
                ti.id,
                ti.server_id,
                ti.tool_id,
                ti.instance_name,
                ti.description,
                ti.created_at,
                t.name as tool_name,
                t.description as tool_description,
                tk.title as toolkit_name
            FROM tool_instances ti
            JOIN tools t ON ti.tool_id = t.id
            JOIN toolkits tk ON t.toolkit_id = tk.id
            WHERE ti.id = ?
            "#,
            id
        )
        .fetch_optional(pool)
        .await?;

        if let Some(data) = instance_data {
            // Get parameters
            let params = InstanceParam::list_by_instance(pool, id).await?;

            Ok(Some(InstanceDetail {
                id: data.id,
                server_id: data.server_id,
                tool_id: data.tool_id,
                instance_name: data.instance_name.clone(),
                description: data.description.clone(),
                tool_name: data.tool_name.clone(),
                tool_description: data.tool_description.clone(),
                toolkit_name: data.toolkit_name.clone(),
                params,
                created_at: chrono::NaiveDateTime::default(),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn list_by_server(pool: &SqlitePool, server_id: i64) -> Result<Vec<Self>> {
        let instances = sqlx::query_as!(
            ToolInstance,
            "SELECT * FROM tool_instances WHERE server_id = ? ORDER BY instance_name",
            server_id
        )
        .fetch_all(pool)
        .await?;

        Ok(instances)
    }

    pub async fn find_by_server_and_name(
        pool: &SqlitePool,
        server_id: i64,
        instance_name: &str,
    ) -> Result<Option<Self>> {
        let instance = sqlx::query_as!(
            ToolInstance,
            "SELECT * FROM tool_instances WHERE server_id = ? AND instance_name = ?",
            server_id,
            instance_name
        )
        .fetch_optional(pool)
        .await?;

        Ok(instance)
    }

    pub async fn list_details_by_server(
        pool: &SqlitePool,
        server_id: i64,
    ) -> Result<Vec<InstanceDetail>> {
        let instances_data = sqlx::query!(
            r#"
            SELECT
                ti.id,
                ti.server_id,
                ti.tool_id,
                ti.instance_name,
                ti.description,
                ti.created_at,
                t.name as tool_name,
                t.description as tool_description,
                tk.title as toolkit_name
            FROM tool_instances ti
            JOIN tools t ON ti.tool_id = t.id
            JOIN toolkits tk ON t.toolkit_id = tk.id
            WHERE ti.server_id = ?
            ORDER BY ti.instance_name
            "#,
            server_id
        )
        .fetch_all(pool)
        .await?;

        let mut details = Vec::new();
        for data in instances_data {
            let params = InstanceParam::list_by_instance(pool, data.id.unwrap_or(0)).await?;
            details.push(InstanceDetail {
                id: data.id.unwrap_or(0),
                server_id: data.server_id,
                tool_id: data.tool_id,
                instance_name: data.instance_name.clone(),
                description: data.description.clone(),
                tool_name: data.tool_name.clone(),
                tool_description: data.tool_description.clone(),
                toolkit_name: data.toolkit_name.clone(),
                params,
                created_at: chrono::NaiveDateTime::default(),
            });
        }

        Ok(details)
    }

    pub async fn update(
        pool: &SqlitePool,
        id: i64,
        instance_name: &str,
        description: Option<&str>,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE tool_instances
            SET instance_name = ?, description = ?, updated_at = unixepoch()
            WHERE id = ?
            "#,
            instance_name,
            description,
            id
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn delete(pool: &SqlitePool, id: i64) -> Result<()> {
        sqlx::query!("DELETE FROM tool_instances WHERE id = ?", id)
            .execute(pool)
            .await?;

        Ok(())
    }

    pub fn get_signature(
        &self,
        tool: &crate::models::tool::Tool,
        params: &[InstanceParam],
    ) -> String {
        use std::collections::HashSet;

        // Extract all parameters from the tool template
        let tool_params = tool.extract_parameters();

        // Build a set of param names that are NOT exposed (bound to instance or server)
        let non_exposed: HashSet<String> = params
            .iter()
            .filter(|p| p.source != "exposed")
            .map(|p| p.param_name.clone())
            .collect();

        // Collect exposed parameters:
        // - Parameters from tool template that are NOT in the non_exposed set
        let exposed: Vec<String> = tool_params
            .iter()
            .filter(|tp| !non_exposed.contains(&tp.name))
            .map(|tp| tp.name.clone())
            .collect();

        if exposed.is_empty() {
            format!("{}()", self.instance_name)
        } else {
            format!("{}({})", self.instance_name, exposed.join(", "))
        }
    }
}

impl InstanceParam {
    pub async fn create(
        pool: &SqlitePool,
        instance_id: i64,
        param_name: &str,
        source: &str,
        value: Option<&str>,
    ) -> Result<i64> {
        let result = sqlx::query!(
            r#"
            INSERT INTO instance_params (instance_id, param_name, source, value)
            VALUES (?, ?, ?, ?)
            "#,
            instance_id,
            param_name,
            source,
            value
        )
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    pub async fn list_by_instance(pool: &SqlitePool, instance_id: i64) -> Result<Vec<Self>> {
        let params = sqlx::query_as!(
            InstanceParam,
            "SELECT * FROM instance_params WHERE instance_id = ? ORDER BY param_name",
            instance_id
        )
        .fetch_all(pool)
        .await?;

        Ok(params)
    }

    pub async fn update_or_create(
        pool: &SqlitePool,
        instance_id: i64,
        param_name: &str,
        source: &str,
        value: Option<&str>,
    ) -> Result<()> {
        // Check if exists
        let exists = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT 1 FROM instance_params WHERE instance_id = ? AND param_name = ?",
        )
        .bind(instance_id)
        .bind(param_name)
        .fetch_optional(pool)
        .await?
        .is_some();

        if exists {
            // Update existing
            sqlx::query!(
                r#"
                UPDATE instance_params
                SET source = ?, value = ?
                WHERE instance_id = ? AND param_name = ?
                "#,
                source,
                value,
                instance_id,
                param_name
            )
            .execute(pool)
            .await?;
        } else {
            // Create new
            Self::create(pool, instance_id, param_name, source, value).await?;
        }

        Ok(())
    }

    pub async fn delete_all_by_instance(pool: &SqlitePool, instance_id: i64) -> Result<()> {
        sqlx::query!(
            "DELETE FROM instance_params WHERE instance_id = ?",
            instance_id
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}
