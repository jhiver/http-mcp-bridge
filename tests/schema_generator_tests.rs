//! Tests for JSON Schema generation from MCP tool instances
//!
//! These tests verify that SchemaGenerator correctly:
//! - Generates schemas from instance parameters with correct type mappings
//! - Filters exposed parameters correctly
//! - Handles edge cases (empty params, mixed sources, etc.)

use saramcp::services::SchemaGenerator;
use saramcp::test_utils::test_helpers;
use serde_json::json;

// ============================================================================
// Schema Generation Tests
// ============================================================================

#[tokio::test]
async fn test_generate_schema_with_exposed_params() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Create tool with typed parameters
    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "test_tool",
        "GET",
        Some("https://api.example.com/users/{{integer:user_id}}?debug={{boolean:debug}}"),
        None,
        None,
        5000,
    )
    .await
    .unwrap();

    let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    // Create instance
    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'test_instance', 'Test instance')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // Add exposed parameters
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'user_id', 'exposed', NULL),
                (?, 'debug', 'exposed', NULL)",
        instance_id,
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Generate schema
    let schema = SchemaGenerator::generate_for_instance(&pool, instance_id)
        .await
        .unwrap();

    // Verify schema structure
    assert_eq!(schema["type"], "object");

    let properties = schema["properties"].as_object().unwrap();
    assert_eq!(properties.len(), 2);

    // Verify user_id property
    let user_id_prop = &properties["user_id"];
    assert_eq!(user_id_prop["type"], "integer");
    assert!(user_id_prop["description"]
        .as_str()
        .unwrap()
        .contains("user_id"));

    // Verify debug property
    let debug_prop = &properties["debug"];
    assert_eq!(debug_prop["type"], "boolean");
    assert!(debug_prop["description"]
        .as_str()
        .unwrap()
        .contains("debug"));

    // Verify required fields
    let required = schema["required"].as_array().unwrap();
    assert_eq!(required.len(), 2);
    assert!(required.contains(&json!("user_id")));
    assert!(required.contains(&json!("debug")));
}

#[tokio::test]
async fn test_generate_schema_no_exposed_params() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "test_tool",
        "GET",
        Some("https://api.example.com/endpoint"),
        None,
        None,
        5000,
    )
    .await
    .unwrap();

    let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'test_instance', 'Test instance')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // No instance_params created

    // Generate schema
    let schema = SchemaGenerator::generate_for_instance(&pool, instance_id)
        .await
        .unwrap();

    // Verify empty schema
    assert_eq!(schema["type"], "object");

    let properties = schema["properties"].as_object().unwrap();
    assert_eq!(properties.len(), 0);

    let required = schema["required"].as_array().unwrap();
    assert_eq!(required.len(), 0);
}

#[tokio::test]
async fn test_generate_schema_mixed_sources() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "test_tool",
        "GET",
        Some("https://{{url:base_url}}/users/{{integer:user_id}}?token={{api_token}}"),
        None,
        None,
        5000,
    )
    .await
    .unwrap();

    let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'test_instance', 'Test instance')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // Add parameters with different sources
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'base_url', 'instance', 'api.example.com'),
                (?, 'user_id', 'exposed', NULL),
                (?, 'api_token', 'server', NULL)",
        instance_id,
        instance_id,
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Generate schema
    let schema = SchemaGenerator::generate_for_instance(&pool, instance_id)
        .await
        .unwrap();

    // Only exposed parameters should appear in schema
    let properties = schema["properties"].as_object().unwrap();
    assert_eq!(properties.len(), 1);
    assert!(properties.contains_key("user_id"));
    assert!(!properties.contains_key("base_url"));
    assert!(!properties.contains_key("api_token"));

    let required = schema["required"].as_array().unwrap();
    assert_eq!(required.len(), 1);
    assert!(required.contains(&json!("user_id")));
}

#[tokio::test]
async fn test_generate_schema_all_types() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "test_tool",
        "POST",
        Some("https://api.example.com/endpoint"),
        None,
        Some(r#"{"name":"{{name}}","age":{{integer:age}},"salary":{{number:salary}},"active":{{boolean:active}},"config":{{json:config}},"website":"{{url:website}}"}"#),
        5000,
    )
    .await
    .unwrap();

    let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'test_instance', 'Test instance')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // Add parameters with all supported types
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'name', 'exposed', NULL),
                (?, 'age', 'exposed', NULL),
                (?, 'salary', 'exposed', NULL),
                (?, 'active', 'exposed', NULL),
                (?, 'config', 'exposed', NULL),
                (?, 'website', 'exposed', NULL)",
        instance_id,
        instance_id,
        instance_id,
        instance_id,
        instance_id,
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Generate schema
    let schema = SchemaGenerator::generate_for_instance(&pool, instance_id)
        .await
        .unwrap();

    let properties = schema["properties"].as_object().unwrap();
    assert_eq!(properties.len(), 6);

    // Verify each type mapping
    assert_eq!(properties["name"]["type"], "string");
    assert_eq!(properties["age"]["type"], "integer");
    assert_eq!(properties["salary"]["type"], "number");
    assert_eq!(properties["active"]["type"], "boolean");
    assert_eq!(properties["config"]["type"], "object");
    assert_eq!(properties["website"]["type"], "string");
    assert_eq!(properties["website"]["format"], "uri");
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_generate_schema_instance_not_found() {
    let pool = test_helpers::create_test_db().await.unwrap();

    let result = SchemaGenerator::generate_for_instance(&pool, 999999).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Instance") || err.to_string().contains("not found"));
}

#[tokio::test]
async fn test_generate_schema_tool_not_found() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "test_tool",
        "GET",
        Some("https://api.example.com"),
        None,
        None,
        5000,
    )
    .await
    .unwrap();

    let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    // Create instance with valid tool_id
    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'test_instance', 'Test instance')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // Now delete the tool to simulate tool not found scenario
    sqlx::query!("DELETE FROM tools WHERE id = ?", tool_id)
        .execute(&pool)
        .await
        .unwrap();

    let result = SchemaGenerator::generate_for_instance(&pool, instance_id).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Tool") || err.to_string().contains("not found"));
}

#[tokio::test]
async fn test_generate_schema_empty_tool_url() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Create tool with no URL (None)
    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "test_tool",
        "GET",
        None,
        None,
        None,
        5000,
    )
    .await
    .unwrap();

    let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'test_instance', 'Test instance')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // Should still generate schema (empty if no params)
    let schema = SchemaGenerator::generate_for_instance(&pool, instance_id)
        .await
        .unwrap();

    assert_eq!(schema["type"], "object");
    let properties = schema["properties"].as_object().unwrap();
    assert_eq!(properties.len(), 0);
}
