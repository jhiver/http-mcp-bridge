use crate::models::{InstanceParam, ServerGlobal, Tool};
use crate::services::{
    variable_engine::{TypedVariableEngine, VariableType},
    SecretsManager,
};
use anyhow::Result;
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;

#[derive(Clone)]
pub struct ParameterResolver {
    engine: TypedVariableEngine,
    secrets: SecretsManager,
}

impl ParameterResolver {
    pub fn new(secrets: SecretsManager) -> Self {
        Self {
            engine: TypedVariableEngine::new(),
            secrets,
        }
    }

    /// Resolve all parameters for a tool instance
    /// Resolution order:
    /// 1. Instance-level fixed values (with variable substitution)
    /// 2. Server-level defaults
    /// 3. Exposed parameters (provided by LLM at execution time)
    pub async fn resolve_parameters(
        &self,
        pool: &SqlitePool,
        instance_id: i64,
        llm_provided: Option<HashMap<String, Value>>,
    ) -> Result<HashMap<String, Value>> {
        let mut resolved = HashMap::new();

        // Get instance configuration
        let instance_params = InstanceParam::list_by_instance(pool, instance_id).await?;

        // Get server ID from instance
        let server_id = sqlx::query!(
            "SELECT server_id FROM tool_instances WHERE id = ?",
            instance_id
        )
        .fetch_one(pool)
        .await?
        .server_id;

        // Load server globals (including decrypted secrets)
        let globals = self.load_globals(pool, server_id).await?;

        // Get tool from instance to extract parameter types
        let tool_id = sqlx::query!(
            "SELECT tool_id FROM tool_instances WHERE id = ?",
            instance_id
        )
        .fetch_one(pool)
        .await?
        .tool_id;

        let tool = Tool::get_by_id(pool, tool_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Tool not found"))?;

        // Extract parameters dynamically from tool templates
        let extracted_params = tool.extract_parameters();

        // Build a map of param_name -> param_type for quick lookup
        let param_types: HashMap<String, String> = extracted_params
            .into_iter()
            .map(|p| (p.name, p.param_type))
            .collect();

        // Resolve each configured parameter
        for config in instance_params {
            let param_type = param_types
                .get(&config.param_name)
                .map(|s| s.as_str())
                .unwrap_or("string");

            let value = match config.source.as_str() {
                "instance" => {
                    // Use instance-level value with variable substitution
                    if let Some(val) = &config.value {
                        Some(self.substitute_and_cast(val, param_type, &globals)?)
                    } else {
                        None
                    }
                }
                "server" => {
                    // Use server default
                    if let Some(val) = globals.get(&config.param_name) {
                        Some(self.cast_value(val, param_type)?)
                    } else {
                        None
                    }
                }
                "exposed" => {
                    // Will be provided at execution time
                    if let Some(llm_values) = &llm_provided {
                        llm_values.get(&config.param_name).cloned()
                    } else {
                        None
                    }
                }
                _ => None,
            };

            // Only add to resolved if we have a value
            if let Some(val) = value {
                resolved.insert(config.param_name, val);
            }
        }

        Ok(resolved)
    }

    /// Get exposed parameters for an instance
    pub async fn get_exposed_params(pool: &SqlitePool, instance_id: i64) -> Result<Vec<String>> {
        let params = InstanceParam::list_by_instance(pool, instance_id).await?;

        Ok(params
            .into_iter()
            .filter(|p| p.source == "exposed")
            .map(|p| p.param_name)
            .collect())
    }

    /// Substitute variables and cast to the correct type
    fn substitute_and_cast(
        &self,
        value: &str,
        param_type: &str,
        globals: &HashMap<String, String>,
    ) -> Result<Value> {
        // First perform variable substitution
        let substituted = self.engine.substitute(value, globals)?;

        // Then cast to appropriate type
        self.cast_value(&substituted, param_type)
    }

    /// Cast a string value to the appropriate JSON type
    fn cast_value(&self, value: &str, param_type: &str) -> Result<Value> {
        let var_type = match param_type.to_lowercase().as_str() {
            "number" => VariableType::Number,
            "integer" => VariableType::Integer,
            "boolean" | "bool" => VariableType::Boolean,
            "json" | "object" | "array" => VariableType::Json,
            "url" => VariableType::Url,
            _ => VariableType::String,
        };

        var_type.cast(value)
    }

    /// Load server globals including decrypted secrets
    async fn load_globals(
        &self,
        pool: &SqlitePool,
        server_id: i64,
    ) -> Result<HashMap<String, String>> {
        let mut globals = HashMap::new();

        let records = ServerGlobal::list_by_server(pool, server_id).await?;

        for record in records {
            let value = if record.is_secret.unwrap_or(false) {
                self.secrets.decrypt(&record.value)?
            } else {
                record.value
            };
            globals.insert(record.key, value);
        }

        Ok(globals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils;

    #[sqlx::test]
    async fn test_parameter_resolution(pool: SqlitePool) {
        // Setup test data
        let secrets = SecretsManager::new().unwrap();
        let resolver = ParameterResolver::new(secrets.clone());

        // Create test user, toolkit
        let user_id = test_utils::create_test_user(&pool, "test@example.com", "password")
            .await
            .unwrap();
        let toolkit_id = test_utils::create_test_toolkit(&pool, user_id, "Test Toolkit")
            .await
            .unwrap();

        // Create tool with parameters in templates
        // Note: Must explicitly set INTEGER timestamps because tools table still uses TEXT TIMESTAMP
        let tool_id = sqlx::query!(
            "INSERT INTO tools (toolkit_id, name, description, method, url, headers, body, timeout_ms, created_at, updated_at)
             VALUES (?, 'TestTool', 'Test tool', 'GET',
                     'https://api.example.com/{{url}}?timeout={{integer:timeout}}&debug={{boolean:debug}}',
                     '{}', '{}', 5000, unixepoch(), unixepoch())",
            toolkit_id
        )
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_rowid();

        // Create server
        let server_id = sqlx::query!(
            "INSERT INTO servers (user_id, name) VALUES (?, 'Test Server')",
            user_id
        )
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_rowid();

        // Add server global
        sqlx::query!(
            "INSERT INTO server_globals (server_id, key, value, is_secret)
             VALUES (?, 'timeout', '3000', false)",
            server_id
        )
        .execute(&pool)
        .await
        .unwrap();

        // Create instance
        let instance_id = sqlx::query!(
            "INSERT INTO tool_instances (server_id, tool_id, instance_name)
             VALUES (?, ?, 'TestInstance')",
            server_id,
            tool_id
        )
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_rowid();

        // Configure parameters
        sqlx::query!(
            "INSERT INTO instance_params (instance_id, param_name, source, value)
             VALUES (?, 'url', 'instance', 'endpoint'),
                    (?, 'timeout', 'server', NULL),
                    (?, 'debug', 'exposed', NULL)",
            instance_id,
            instance_id,
            instance_id
        )
        .execute(&pool)
        .await
        .unwrap();

        // Resolve parameters without LLM input
        let resolved = resolver
            .resolve_parameters(&pool, instance_id, None)
            .await
            .unwrap();

        assert_eq!(resolved.get("url").unwrap().as_str().unwrap(), "endpoint");
        assert_eq!(
            resolved.get("timeout").unwrap().as_i64().unwrap(),
            3000 // From server global
        );
        // debug is exposed, so not in resolved without LLM input

        // Resolve with LLM input
        let mut llm_provided = HashMap::new();
        llm_provided.insert("debug".to_string(), Value::Bool(true));

        let resolved = resolver
            .resolve_parameters(&pool, instance_id, Some(llm_provided))
            .await
            .unwrap();

        assert!(resolved.get("debug").unwrap().as_bool().unwrap());
    }
}
