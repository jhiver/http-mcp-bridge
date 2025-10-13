//! JSON Schema generation for MCP tool definitions
//!
//! This module generates JSON Schema definitions for MCP tool parameters by analyzing
//! tool instances and their parameter configurations. It bridges SaraMCP's typed parameter
//! system with the MCP protocol's JSON Schema requirements.
//!
//! # Architecture
//!
//! The SchemaGenerator service:
//! 1. Loads tool instances from the database
//! 2. Extracts parameters with source="exposed" (LLM-provided)
//! 3. Maps SaraMCP variable types to JSON Schema types
//! 4. Generates JSON Schema with properties and required fields
//!
//! # Type Mapping
//!
//! SaraMCP types are mapped to JSON Schema as follows:
//!
//! | SaraMCP Type | JSON Schema Type | Additional Properties |
//! |--------------|------------------|----------------------|
//! | string       | "string"         | -                    |
//! | integer      | "integer"        | -                    |
//! | number       | "number"         | -                    |
//! | boolean/bool | "boolean"        | -                    |
//! | json/object  | "object"         | -                    |
//! | array        | "array"          | -                    |
//! | url          | "string"         | format: "uri"        |
//!
//! # Integration
//!
//! Used by SaraMcpService when building ToolRoute definitions for the MCP protocol.
//! Each tool instance generates one JSON Schema that defines the parameters the LLM
//! must provide when calling the tool.
//!
//! # Usage
//!
//! ```rust,no_run
//! use saramcp::services::schema_generator::SchemaGenerator;
//! use sqlx::SqlitePool;
//!
//! async fn generate_schema(pool: &SqlitePool, instance_id: i64) {
//!     let schema = SchemaGenerator::generate_for_instance(pool, instance_id)
//!         .await
//!         .expect("Failed to generate schema");
//!
//!     // schema is a JSON object with "type", "properties", and "required" fields
//!     println!("{}", serde_json::to_string_pretty(&schema).unwrap());
//! }
//! ```

use crate::error::McpServiceError;
use crate::models::instance::InstanceParam;
use crate::models::tool::Tool;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::HashMap;

pub struct SchemaGenerator;

impl SchemaGenerator {
    /// Map SaraMCP variable type to JSON Schema type
    ///
    /// Converts a SaraMCP type string (e.g., "string", "integer", "url") into
    /// a JSON Schema type definition.
    ///
    /// # Arguments
    ///
    /// * `var_type` - The SaraMCP variable type string (case-insensitive)
    ///
    /// # Returns
    ///
    /// A JSON Schema type definition as a serde_json::Value
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let schema = SchemaGenerator::map_variable_type_to_json_schema("integer");
    /// // Returns: {"type": "integer"}
    ///
    /// let schema = SchemaGenerator::map_variable_type_to_json_schema("url");
    /// // Returns: {"type": "string", "format": "uri"}
    /// ```
    fn map_variable_type_to_json_schema(var_type: &str) -> Value {
        match var_type.to_lowercase().as_str() {
            "string" => json!({"type": "string"}),
            "integer" => json!({"type": "integer"}),
            "number" => json!({"type": "number"}),
            "boolean" | "bool" => json!({"type": "boolean"}),
            "json" | "object" => json!({"type": "object"}),
            "array" => json!({"type": "array"}),
            "url" => json!({"type": "string", "format": "uri"}),
            _ => json!({"type": "string"}),
        }
    }

    /// Build schema for a single property
    ///
    /// Creates a complete JSON Schema property definition for a parameter,
    /// including type information and a description field.
    ///
    /// # Arguments
    ///
    /// * `param_name` - The name of the parameter
    /// * `param_type` - The SaraMCP type of the parameter
    ///
    /// # Returns
    ///
    /// A JSON Schema property definition with type and description
    fn build_property_schema(param_name: &str, param_type: &str) -> Value {
        let mut schema = Self::map_variable_type_to_json_schema(param_type);

        if let Some(obj) = schema.as_object_mut() {
            obj.insert(
                "description".to_string(),
                json!(format!("Parameter: {}", param_name)),
            );
        }

        schema
    }

    /// Generate JSON Schema for an instance
    ///
    /// Loads instance parameters with source="exposed" and creates a complete
    /// JSON Schema object suitable for MCP tool definitions. Only exposed parameters
    /// (those provided by the LLM at runtime) are included in the schema.
    ///
    /// # Arguments
    ///
    /// * `pool` - Database connection pool
    /// * `instance_id` - ID of the tool instance
    ///
    /// # Returns
    ///
    /// A JSON Schema object with:
    /// - `type`: "object"
    /// - `properties`: Map of parameter names to their schemas
    /// - `required`: Array of all exposed parameter names
    ///
    /// # Errors
    ///
    /// * `McpServiceError::InstanceNotFound` - Instance does not exist
    /// * `McpServiceError::ToolNotFound` - Referenced tool does not exist
    /// * `McpServiceError::Database` - Database query failure
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use saramcp::services::schema_generator::SchemaGenerator;
    /// # use sqlx::SqlitePool;
    /// # async fn example(pool: &SqlitePool) {
    /// let schema = SchemaGenerator::generate_for_instance(pool, 42).await.unwrap();
    /// // schema = {
    /// //   "type": "object",
    /// //   "properties": {
    /// //     "api_key": {"type": "string", "description": "Parameter: api_key"},
    /// //     "timeout": {"type": "integer", "description": "Parameter: timeout"}
    /// //   },
    /// //   "required": ["api_key", "timeout"]
    /// // }
    /// # }
    /// ```
    pub async fn generate_for_instance(
        pool: &SqlitePool,
        instance_id: i64,
    ) -> Result<Value, McpServiceError> {
        let tool_id =
            sqlx::query_scalar::<_, i64>("SELECT tool_id FROM tool_instances WHERE id = ?")
                .bind(instance_id)
                .fetch_optional(pool)
                .await?
                .ok_or_else(|| {
                    McpServiceError::InstanceNotFound(format!("Instance {} not found", instance_id))
                })?;

        let tool = Tool::get_by_id(pool, tool_id)
            .await?
            .ok_or_else(|| McpServiceError::ToolNotFound(format!("Tool {} not found", tool_id)))?;

        let instance_params = InstanceParam::list_by_instance(pool, instance_id).await?;

        let extracted = tool.extract_parameters();
        let param_types: HashMap<String, String> = extracted
            .into_iter()
            .map(|p| (p.name, p.param_type))
            .collect();

        let exposed: Vec<&InstanceParam> = instance_params
            .iter()
            .filter(|p| p.source == "exposed")
            .collect();

        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in exposed {
            let param_type = param_types
                .get(&param.param_name)
                .map(|s| s.as_str())
                .unwrap_or("string");

            properties.insert(
                param.param_name.clone(),
                Self::build_property_schema(&param.param_name, param_type),
            );

            required.push(param.param_name.clone());
        }

        Ok(json!({
            "type": "object",
            "properties": properties,
            "required": required
        }))
    }
}
