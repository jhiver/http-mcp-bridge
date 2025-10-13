use crate::models::{
    ConfigureInstanceForm, ExtractedParameter, InstanceDetail, InstanceParam, Tool, ToolInstance,
};
use crate::services::{ParameterResolver, SecretsManager};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct ParameterUsageCount {
    pub param_name: String,
    pub param_type: String,
    pub usage_count: usize,
    pub used_in_instances: Vec<String>,
}

#[derive(Clone)]
pub struct InstanceService {
    pool: SqlitePool,
    resolver: ParameterResolver,
}

impl InstanceService {
    pub fn new(pool: SqlitePool, secrets: SecretsManager) -> Self {
        Self {
            pool: pool.clone(),
            resolver: ParameterResolver::new(secrets),
        }
    }

    // Instance CRUD operations
    pub async fn create_instance_with_config(
        &self,
        server_id: i64,
        form: ConfigureInstanceForm,
    ) -> Result<i64> {
        ToolInstance::create_with_config(&self.pool, server_id, form).await
    }

    pub async fn get_instance(&self, instance_id: i64) -> Result<Option<ToolInstance>> {
        ToolInstance::get_by_id(&self.pool, instance_id).await
    }

    pub async fn get_instance_detail(&self, instance_id: i64) -> Result<Option<InstanceDetail>> {
        ToolInstance::get_detail(&self.pool, instance_id).await
    }

    pub async fn list_instances_by_server(&self, server_id: i64) -> Result<Vec<InstanceDetail>> {
        ToolInstance::list_details_by_server(&self.pool, server_id).await
    }

    pub async fn update_instance(
        &self,
        instance_id: i64,
        name: &str,
        description: Option<&str>,
    ) -> Result<()> {
        ToolInstance::update(&self.pool, instance_id, name, description).await
    }

    pub async fn delete_instance(&self, instance_id: i64) -> Result<()> {
        ToolInstance::delete(&self.pool, instance_id).await
    }

    // Parameter management
    pub async fn update_instance_params(
        &self,
        instance_id: i64,
        params: Vec<InstanceParam>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // Clear existing params
        sqlx::query!(
            "DELETE FROM instance_params WHERE instance_id = ?",
            instance_id
        )
        .execute(&mut *tx)
        .await?;

        // Insert new params
        for param in params {
            sqlx::query!(
                r#"
                INSERT INTO instance_params (instance_id, param_name, source, value)
                VALUES (?, ?, ?, ?)
                "#,
                instance_id,
                param.param_name,
                param.source,
                param.value
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    // Get exposed parameters for an instance
    pub async fn get_exposed_params(&self, instance_id: i64) -> Result<Vec<String>> {
        ParameterResolver::get_exposed_params(&self.pool, instance_id).await
    }

    // Get the signature for an instance (for display)
    pub async fn get_instance_signature(&self, instance_id: i64) -> Result<String> {
        let instance = ToolInstance::get_by_id(&self.pool, instance_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Instance not found"))?;

        let tool = crate::models::tool::Tool::get_by_id(&self.pool, instance.tool_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Tool not found"))?;

        let params = InstanceParam::list_by_instance(&self.pool, instance_id).await?;

        Ok(instance.get_signature(&tool, &params))
    }

    // Execute instance with parameters (returns resolved parameters)
    pub async fn execute_instance(
        &self,
        instance_id: i64,
        llm_provided: Option<HashMap<String, Value>>,
    ) -> Result<HashMap<String, Value>> {
        self.resolver
            .resolve_parameters(&self.pool, instance_id, llm_provided)
            .await
    }

    // Get available tools for a server (from imported toolkits)
    pub async fn get_available_tools(&self, server_id: i64) -> Result<Vec<ToolWithParams>> {
        let tool_ids = sqlx::query!(
            r#"
            SELECT DISTINCT t.id as id
            FROM server_toolkits st
            JOIN toolkits tk ON st.toolkit_id = tk.id
            JOIN tools t ON tk.id = t.toolkit_id
            WHERE st.server_id = ?
            ORDER BY tk.title, t.name
            "#,
            server_id
        )
        .fetch_all(&self.pool)
        .await?;

        let mut tools = Vec::new();
        for row in tool_ids {
            let tool_id = row.id.unwrap_or(0);
            if tool_id > 0 {
                if let Some(tool) = Tool::get_by_id(&self.pool, tool_id).await? {
                    tools.push(tool);
                }
            }
        }

        let mut result = Vec::new();
        for tool in tools {
            // Get toolkit info for display
            let toolkit = sqlx::query!(
                "SELECT id, title FROM toolkits WHERE id = ?",
                tool.toolkit_id
            )
            .fetch_one(&self.pool)
            .await?;

            // Extract parameters dynamically
            let params = tool.extract_parameters();

            result.push(ToolWithParams {
                tool_id: tool.id,
                tool_name: tool.name,
                tool_description: tool.description,
                toolkit_id: toolkit.id,
                toolkit_title: toolkit.title,
                params,
            });
        }

        Ok(result)
    }

    // Check if an instance name is available in a server
    pub async fn is_instance_name_available(
        &self,
        server_id: i64,
        name: &str,
        exclude_id: Option<i64>,
    ) -> Result<bool> {
        if let Some(id) = exclude_id {
            let exists = sqlx::query_scalar::<_, Option<i64>>(
                "SELECT 1 FROM tool_instances
                 WHERE server_id = ? AND instance_name = ? AND id != ?",
            )
            .bind(server_id)
            .bind(name)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
            Ok(exists.is_none())
        } else {
            let exists = sqlx::query_scalar::<_, Option<i64>>(
                "SELECT 1 FROM tool_instances
                 WHERE server_id = ? AND instance_name = ?",
            )
            .bind(server_id)
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
            Ok(exists.is_none())
        }
    }

    // Generate a unique instance name for a tool
    pub async fn generate_instance_name(&self, server_id: i64, tool_name: &str) -> Result<String> {
        // Try the base tool name first
        if self
            .is_instance_name_available(server_id, tool_name, None)
            .await?
        {
            return Ok(tool_name.to_string());
        }

        // Try appending _2, _3, etc.
        for i in 2..=100 {
            let candidate = format!("{}_{}", tool_name, i);
            if self
                .is_instance_name_available(server_id, &candidate, None)
                .await?
            {
                return Ok(candidate);
            }
        }

        // Fallback with timestamp if we somehow can't find a name
        Ok(format!("{}_{}", tool_name, chrono::Utc::now().timestamp()))
    }

    // Discover parameters used across all instances and count usage
    pub async fn discover_parameters_with_usage(
        &self,
        server_id: i64,
    ) -> Result<Vec<ParameterUsageCount>> {
        let instances = ToolInstance::list_by_server(&self.pool, server_id).await?;

        let mut param_map: HashMap<(String, String), Vec<String>> = HashMap::new();

        for instance in instances {
            let tool = Tool::get_by_id(&self.pool, instance.tool_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Tool not found"))?;

            let params = tool.extract_parameters();

            for param in params {
                let key = (param.name.clone(), param.param_type.clone());
                param_map
                    .entry(key)
                    .or_default()
                    .push(instance.instance_name.clone());
            }
        }

        let mut result: Vec<ParameterUsageCount> = param_map
            .into_iter()
            .map(|((name, param_type), instances)| {
                let usage_count = instances.len();
                ParameterUsageCount {
                    param_name: name,
                    param_type,
                    usage_count,
                    used_in_instances: instances,
                }
            })
            .collect();

        result.sort_by(|a, b| {
            b.usage_count
                .cmp(&a.usage_count)
                .then_with(|| a.param_name.cmp(&b.param_name))
        });

        Ok(result)
    }
}

#[derive(Debug, serde::Serialize)]
pub struct ToolWithParams {
    pub tool_id: i64,
    pub tool_name: String,
    pub tool_description: Option<String>,
    pub toolkit_id: i64,
    pub toolkit_title: String,
    pub params: Vec<ExtractedParameter>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ConfigureInstanceForm;
    use crate::services::SecretsManager;
    use crate::test_utils::test_helpers;

    async fn setup_test_environment() -> Result<(SqlitePool, InstanceService, i64, i64, i64)> {
        let pool = test_helpers::create_test_db().await?;

        // Create test user
        let user_id =
            test_helpers::insert_test_user(&pool, "test@example.com", "password", true).await?;

        // Create test toolkit
        let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit").await?;

        // Create test server
        let server_id = sqlx::query!(
            "INSERT INTO servers (user_id, name, description) VALUES (?, ?, ?)",
            user_id,
            "Test Server",
            "Test Description"
        )
        .execute(&pool)
        .await?
        .last_insert_rowid();

        // Create secrets manager (will use env var or default)
        let secrets = SecretsManager::new()?;

        // Create instance service
        let service = InstanceService::new(pool.clone(), secrets);

        Ok((pool, service, user_id, toolkit_id, server_id))
    }

    async fn create_test_tool(
        pool: &SqlitePool,
        toolkit_id: i64,
        name: &str,
        url: &str,
    ) -> Result<i64> {
        let tool_id = sqlx::query!(
            r#"
            INSERT INTO tools (toolkit_id, name, description, method, url, headers, timeout_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            toolkit_id,
            name,
            "Test tool",
            "POST",
            url,
            "{}",
            30000
        )
        .execute(pool)
        .await?
        .last_insert_rowid();

        Ok(tool_id)
    }

    #[tokio::test]
    async fn test_discover_parameters_with_no_instances() -> Result<()> {
        let (_pool, service, _user_id, _toolkit_id, server_id) = setup_test_environment().await?;

        let params = service.discover_parameters_with_usage(server_id).await?;

        assert_eq!(params.len(), 0, "Should return empty vec when no instances");
        Ok(())
    }

    #[tokio::test]
    async fn test_discover_parameters_single_instance_single_parameter() -> Result<()> {
        let (pool, service, _user_id, toolkit_id, server_id) = setup_test_environment().await?;

        // Create tool with one parameter
        let tool_id = create_test_tool(
            &pool,
            toolkit_id,
            "test_tool",
            "https://api.example.com/{{string:api_key}}",
        )
        .await?;

        // Create instance
        let form = ConfigureInstanceForm {
            instance_name: "test_instance".to_string(),
            description: None,
            tool_id,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };

        service.create_instance_with_config(server_id, form).await?;

        let params = service.discover_parameters_with_usage(server_id).await?;

        assert_eq!(params.len(), 1, "Should find one parameter");
        assert_eq!(params[0].param_name, "api_key");
        assert_eq!(params[0].param_type, "string");
        assert_eq!(params[0].usage_count, 1);
        assert_eq!(params[0].used_in_instances.len(), 1);
        assert_eq!(params[0].used_in_instances[0], "test_instance");

        Ok(())
    }

    #[tokio::test]
    async fn test_discover_parameters_multiple_instances_same_parameter() -> Result<()> {
        let (pool, service, _user_id, toolkit_id, server_id) = setup_test_environment().await?;

        // Create two tools with same parameter
        let tool_id_1 = create_test_tool(
            &pool,
            toolkit_id,
            "tool_1",
            "https://api.example.com/{{string:api_key}}",
        )
        .await?;

        let tool_id_2 = create_test_tool(
            &pool,
            toolkit_id,
            "tool_2",
            "https://api.example.com/user/{{string:api_key}}",
        )
        .await?;

        // Create two instances
        let form1 = ConfigureInstanceForm {
            instance_name: "instance_1".to_string(),
            description: None,
            tool_id: tool_id_1,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };
        service
            .create_instance_with_config(server_id, form1)
            .await?;

        let form2 = ConfigureInstanceForm {
            instance_name: "instance_2".to_string(),
            description: None,
            tool_id: tool_id_2,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };
        service
            .create_instance_with_config(server_id, form2)
            .await?;

        let params = service.discover_parameters_with_usage(server_id).await?;

        assert_eq!(params.len(), 1, "Should consolidate same parameter");
        assert_eq!(params[0].param_name, "api_key");
        assert_eq!(
            params[0].usage_count, 2,
            "Should count usage in both instances"
        );
        assert_eq!(params[0].used_in_instances.len(), 2);
        assert!(params[0]
            .used_in_instances
            .contains(&"instance_1".to_string()));
        assert!(params[0]
            .used_in_instances
            .contains(&"instance_2".to_string()));

        Ok(())
    }

    #[tokio::test]
    async fn test_discover_parameters_multiple_instances_different_parameters() -> Result<()> {
        let (pool, service, _user_id, toolkit_id, server_id) = setup_test_environment().await?;

        // Create tools with different parameters
        let tool_id_1 = create_test_tool(
            &pool,
            toolkit_id,
            "tool_1",
            "https://api.example.com/{{string:api_key}}",
        )
        .await?;

        let tool_id_2 = create_test_tool(
            &pool,
            toolkit_id,
            "tool_2",
            "https://api.example.com/user/{{string:user_id}}",
        )
        .await?;

        // Create instances
        let form1 = ConfigureInstanceForm {
            instance_name: "instance_1".to_string(),
            description: None,
            tool_id: tool_id_1,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };
        service
            .create_instance_with_config(server_id, form1)
            .await?;

        let form2 = ConfigureInstanceForm {
            instance_name: "instance_2".to_string(),
            description: None,
            tool_id: tool_id_2,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };
        service
            .create_instance_with_config(server_id, form2)
            .await?;

        let params = service.discover_parameters_with_usage(server_id).await?;

        assert_eq!(params.len(), 2, "Should find both different parameters");

        let api_key_param = params.iter().find(|p| p.param_name == "api_key").unwrap();
        assert_eq!(api_key_param.usage_count, 1);

        let user_id_param = params.iter().find(|p| p.param_name == "user_id").unwrap();
        assert_eq!(user_id_param.usage_count, 1);

        Ok(())
    }

    #[tokio::test]
    async fn test_discover_parameters_same_name_different_types() -> Result<()> {
        let (pool, service, _user_id, toolkit_id, server_id) = setup_test_environment().await?;

        // Create tools with same parameter name but different types
        let tool_id_1 = create_test_tool(
            &pool,
            toolkit_id,
            "tool_1",
            "https://api.example.com/{{string:key}}",
        )
        .await?;

        let tool_id_2 = create_test_tool(
            &pool,
            toolkit_id,
            "tool_2",
            "https://api.example.com/user/{{number:key}}",
        )
        .await?;

        // Create instances
        let form1 = ConfigureInstanceForm {
            instance_name: "instance_1".to_string(),
            description: None,
            tool_id: tool_id_1,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };
        service
            .create_instance_with_config(server_id, form1)
            .await?;

        let form2 = ConfigureInstanceForm {
            instance_name: "instance_2".to_string(),
            description: None,
            tool_id: tool_id_2,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };
        service
            .create_instance_with_config(server_id, form2)
            .await?;

        let params = service.discover_parameters_with_usage(server_id).await?;

        assert_eq!(
            params.len(),
            2,
            "Should treat same name with different types as separate"
        );

        let string_key = params.iter().find(|p| p.param_type == "string").unwrap();
        assert_eq!(string_key.param_name, "key");
        assert_eq!(string_key.usage_count, 1);

        let number_key = params.iter().find(|p| p.param_type == "number").unwrap();
        assert_eq!(number_key.param_name, "key");
        assert_eq!(number_key.usage_count, 1);

        Ok(())
    }

    #[tokio::test]
    async fn test_discover_parameters_sorting_by_usage() -> Result<()> {
        let (pool, service, _user_id, toolkit_id, server_id) = setup_test_environment().await?;

        // Create tools with different parameters
        let tool_id_1 = create_test_tool(
            &pool,
            toolkit_id,
            "tool_1",
            "https://api.example.com/{{string:common_key}}",
        )
        .await?;

        let tool_id_2 = create_test_tool(
            &pool,
            toolkit_id,
            "tool_2",
            "https://api.example.com/{{string:common_key}}/{{string:rare_key}}",
        )
        .await?;

        let tool_id_3 = create_test_tool(
            &pool,
            toolkit_id,
            "tool_3",
            "https://api.example.com/{{string:common_key}}",
        )
        .await?;

        // Create instances - common_key used 3 times, rare_key used 1 time
        let form1 = ConfigureInstanceForm {
            instance_name: "instance_1".to_string(),
            description: None,
            tool_id: tool_id_1,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };
        service
            .create_instance_with_config(server_id, form1)
            .await?;

        let form2 = ConfigureInstanceForm {
            instance_name: "instance_2".to_string(),
            description: None,
            tool_id: tool_id_2,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };
        service
            .create_instance_with_config(server_id, form2)
            .await?;

        let form3 = ConfigureInstanceForm {
            instance_name: "instance_3".to_string(),
            description: None,
            tool_id: tool_id_3,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };
        service
            .create_instance_with_config(server_id, form3)
            .await?;

        let params = service.discover_parameters_with_usage(server_id).await?;

        assert_eq!(params.len(), 2);
        assert_eq!(
            params[0].param_name, "common_key",
            "Most used parameter should be first"
        );
        assert_eq!(params[0].usage_count, 3);
        assert_eq!(params[1].param_name, "rare_key");
        assert_eq!(params[1].usage_count, 1);

        Ok(())
    }

    #[tokio::test]
    async fn test_discover_parameters_sorting_alphabetically_when_same_usage() -> Result<()> {
        let (pool, service, _user_id, toolkit_id, server_id) = setup_test_environment().await?;

        // Create tools with different parameters, all used once
        let tool_id_1 = create_test_tool(
            &pool,
            toolkit_id,
            "tool_1",
            "https://api.example.com/{{string:zebra}}",
        )
        .await?;

        let tool_id_2 = create_test_tool(
            &pool,
            toolkit_id,
            "tool_2",
            "https://api.example.com/{{string:alpha}}",
        )
        .await?;

        let tool_id_3 = create_test_tool(
            &pool,
            toolkit_id,
            "tool_3",
            "https://api.example.com/{{string:beta}}",
        )
        .await?;

        // Create instances
        let form1 = ConfigureInstanceForm {
            instance_name: "instance_1".to_string(),
            description: None,
            tool_id: tool_id_1,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };
        service
            .create_instance_with_config(server_id, form1)
            .await?;

        let form2 = ConfigureInstanceForm {
            instance_name: "instance_2".to_string(),
            description: None,
            tool_id: tool_id_2,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };
        service
            .create_instance_with_config(server_id, form2)
            .await?;

        let form3 = ConfigureInstanceForm {
            instance_name: "instance_3".to_string(),
            description: None,
            tool_id: tool_id_3,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };
        service
            .create_instance_with_config(server_id, form3)
            .await?;

        let params = service.discover_parameters_with_usage(server_id).await?;

        assert_eq!(params.len(), 3);
        assert_eq!(
            params[0].param_name, "alpha",
            "Should be sorted alphabetically"
        );
        assert_eq!(params[1].param_name, "beta");
        assert_eq!(params[2].param_name, "zebra");

        Ok(())
    }

    #[tokio::test]
    async fn test_instance_inherits_tool_description_when_none() -> Result<()> {
        let (pool, service, _user_id, toolkit_id, server_id) = setup_test_environment().await?;

        // Create a tool with a description
        let tool_id = sqlx::query!(
            r#"
            INSERT INTO tools (toolkit_id, name, description, method, url, headers, timeout_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            toolkit_id,
            "test_tool",
            "This is the tool description",
            "POST",
            "https://api.example.com",
            "{}",
            30000
        )
        .execute(&pool)
        .await?
        .last_insert_rowid();

        // Create instance with no description
        let form = ConfigureInstanceForm {
            instance_name: "test_instance".to_string(),
            description: None,
            tool_id,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };

        let instance_id = service.create_instance_with_config(server_id, form).await?;

        // Verify instance inherited the tool description
        let instance = service.get_instance(instance_id).await?.unwrap();
        assert_eq!(
            instance.description,
            Some("This is the tool description".to_string()),
            "Instance should inherit tool description when form description is None"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_instance_inherits_tool_description_when_empty() -> Result<()> {
        let (pool, service, _user_id, toolkit_id, server_id) = setup_test_environment().await?;

        // Create a tool with a description
        let tool_id = sqlx::query!(
            r#"
            INSERT INTO tools (toolkit_id, name, description, method, url, headers, timeout_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            toolkit_id,
            "test_tool",
            "This is the tool description",
            "POST",
            "https://api.example.com",
            "{}",
            30000
        )
        .execute(&pool)
        .await?
        .last_insert_rowid();

        // Create instance with empty description
        let form = ConfigureInstanceForm {
            instance_name: "test_instance".to_string(),
            description: Some("   ".to_string()), // Empty/whitespace
            tool_id,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };

        let instance_id = service.create_instance_with_config(server_id, form).await?;

        // Verify instance inherited the tool description
        let instance = service.get_instance(instance_id).await?.unwrap();
        assert_eq!(
            instance.description,
            Some("This is the tool description".to_string()),
            "Instance should inherit tool description when form description is empty"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_instance_uses_custom_description_when_provided() -> Result<()> {
        let (pool, service, _user_id, toolkit_id, server_id) = setup_test_environment().await?;

        // Create a tool with a description
        let tool_id = sqlx::query!(
            r#"
            INSERT INTO tools (toolkit_id, name, description, method, url, headers, timeout_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            toolkit_id,
            "test_tool",
            "This is the tool description",
            "POST",
            "https://api.example.com",
            "{}",
            30000
        )
        .execute(&pool)
        .await?
        .last_insert_rowid();

        // Create instance with custom description
        let form = ConfigureInstanceForm {
            instance_name: "test_instance".to_string(),
            description: Some("Custom instance description".to_string()),
            tool_id,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };

        let instance_id = service.create_instance_with_config(server_id, form).await?;

        // Verify instance uses custom description
        let instance = service.get_instance(instance_id).await?.unwrap();
        assert_eq!(
            instance.description,
            Some("Custom instance description".to_string()),
            "Instance should use custom description when provided"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_instance_description_none_when_tool_has_no_description() -> Result<()> {
        let (pool, service, _user_id, toolkit_id, server_id) = setup_test_environment().await?;

        // Create a tool without a description
        let tool_id = sqlx::query!(
            r#"
            INSERT INTO tools (toolkit_id, name, description, method, url, headers, timeout_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            toolkit_id,
            "test_tool",
            None as Option<String>,
            "POST",
            "https://api.example.com",
            "{}",
            30000
        )
        .execute(&pool)
        .await?
        .last_insert_rowid();

        // Create instance with no description
        let form = ConfigureInstanceForm {
            instance_name: "test_instance".to_string(),
            description: None,
            tool_id,
            param_configs: vec![],
            csrf_token: "test".to_string(),
        };

        let instance_id = service.create_instance_with_config(server_id, form).await?;

        // Verify instance has no description
        let instance = service.get_instance(instance_id).await?.unwrap();
        assert_eq!(
            instance.description, None,
            "Instance should have None when both form and tool have no description"
        );

        Ok(())
    }
}
