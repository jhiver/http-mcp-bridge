//! Integration tests for MCP services
//!
//! These tests verify end-to-end MCP functionality:
//! - Full parameter resolution flow (instance → server → exposed)
//! - HTTP execution with all parameter sources
//! - Secret decryption during execution
//! - Variable substitution in instance values
//! - Schema generation matching actual execution

use saramcp::models::tool::Tool;
use saramcp::services::{InstanceExecutor, SchemaGenerator, SecretsManager};
use saramcp::test_utils::test_helpers;
use serde_json::json;
use wiremock::matchers::{body_string, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ============================================================================
// Full MCP Flow Tests
// ============================================================================

#[tokio::test]
async fn test_full_mcp_flow() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // 1. Create user
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    // 2. Create toolkit and tool
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "API Toolkit")
        .await
        .unwrap();

    // Start mock server
    let mock_server = MockServer::start().await;
    let base_url = mock_server.uri();

    // Mock API endpoint
    Mock::given(method("POST"))
        .and(path("/api/v2/users"))
        .and(header("Authorization", "Bearer secret-api-key"))
        .and(body_string(r#"{"name":"Alice","age":25}"#))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_string(r#"{"id": 123, "name": "Alice", "age": 25}"#),
        )
        .mount(&mock_server)
        .await;

    // Create tool with parameters in URL, headers, and body
    let tool_url = format!("{}/api/{{{{version}}}}/users", base_url);
    let headers = r#"{"Authorization":"Bearer {{api_key}}"}"#;
    let body = r#"{"name":"{{name}}","age":{{integer:age}}}"#;

    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "create_user",
        "POST",
        Some(&tool_url),
        Some(headers),
        Some(body),
        5000,
    )
    .await
    .unwrap();

    let tool = Tool::get_by_id(&pool, tool_id).await.unwrap().unwrap();

    // 3. Create server
    let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Production API", None)
        .await
        .unwrap();

    // 4. Add server global (encrypted secret)
    let secrets = SecretsManager::new().unwrap();
    let encrypted_key = secrets.encrypt("secret-api-key").unwrap();

    sqlx::query!(
        "INSERT INTO server_globals (server_id, key, value, is_secret)
         VALUES (?, 'api_key', ?, true)",
        server_id,
        encrypted_key
    )
    .execute(&pool)
    .await
    .unwrap();

    // 5. Create instance with mixed parameters
    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'create_user', 'Create a new user')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // Configure parameters:
    // - version: instance source (fixed value)
    // - api_key: server source (from encrypted global)
    // - name, age: exposed (LLM provides)
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'version', 'instance', 'v2'),
                (?, 'api_key', 'server', NULL),
                (?, 'name', 'exposed', NULL),
                (?, 'age', 'exposed', NULL)",
        instance_id,
        instance_id,
        instance_id,
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // 6. Generate schema and verify
    let schema = SchemaGenerator::generate_for_instance(&pool, instance_id)
        .await
        .unwrap();

    // Only exposed parameters should be in schema
    let properties = schema["properties"].as_object().unwrap();
    assert_eq!(properties.len(), 2);
    assert!(properties.contains_key("name"));
    assert!(properties.contains_key("age"));

    // 7. Execute with LLM-provided parameters
    let executor = InstanceExecutor::new(pool.clone(), server_id, instance_id, tool, secrets);

    let mut llm_params = serde_json::Map::new();
    llm_params.insert("name".to_string(), json!("Alice"));
    llm_params.insert("age".to_string(), json!(25));

    let result = executor.execute(Some(llm_params)).await;

    // 8. Verify successful execution
    assert!(result.is_ok());
    let call_result = result.unwrap();
    assert!(call_result.is_error.is_none() || !call_result.is_error.unwrap());

    // 9. Verify response content
    let content = &call_result.content[0];
    if let rmcp::model::RawContent::Text(text) = &**content {
        assert!(text.text.contains("Alice"));
        assert!(text.text.contains("123"));
    } else {
        panic!("Expected text content");
    }
}

#[tokio::test]
async fn test_parameter_resolution_all_sources() {
    let pool = test_helpers::create_test_db().await.unwrap();

    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start mock server
    let mock_server = MockServer::start().await;
    let base_url = mock_server.uri();

    // Mock expects all parameters resolved correctly
    Mock::given(method("GET"))
        .and(path("/api/production/users/42"))
        .and(header("X-Environment", "production"))
        .and(header("Authorization", "Bearer decrypted-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_string("success"))
        .mount(&mock_server)
        .await;

    // Create tool with parameters from all three sources
    let tool_url = format!("{}/api/{{{{env}}}}/users/{{{{integer:user_id}}}}", base_url);
    let headers = r#"{"X-Environment":"{{env}}","Authorization":"Bearer {{api_token}}"}"#;

    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "get_user",
        "GET",
        Some(&tool_url),
        Some(headers),
        None,
        5000,
    )
    .await
    .unwrap();

    let tool = Tool::get_by_id(&pool, tool_id).await.unwrap().unwrap();

    let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    // Add server global (encrypted)
    let secrets = SecretsManager::new().unwrap();
    let encrypted = secrets.encrypt("decrypted-secret").unwrap();

    sqlx::query!(
        "INSERT INTO server_globals (server_id, key, value, is_secret)
         VALUES (?, 'api_token', ?, true)",
        server_id,
        encrypted
    )
    .execute(&pool)
    .await
    .unwrap();

    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'get_user', 'Get user')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // Configure parameters:
    // - env: instance (fixed to "production")
    // - api_token: server (encrypted secret)
    // - user_id: exposed (LLM provides)
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'env', 'instance', 'production'),
                (?, 'api_token', 'server', NULL),
                (?, 'user_id', 'exposed', NULL)",
        instance_id,
        instance_id,
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Execute
    let executor = InstanceExecutor::new(pool.clone(), server_id, instance_id, tool, secrets);

    let mut llm_params = serde_json::Map::new();
    llm_params.insert("user_id".to_string(), json!(42));

    let result = executor.execute(Some(llm_params)).await;

    assert!(result.is_ok());
    let call_result = result.unwrap();
    assert!(call_result.is_error.is_none() || !call_result.is_error.unwrap());
}

#[tokio::test]
async fn test_variable_substitution_in_instance_values() {
    let pool = test_helpers::create_test_db().await.unwrap();

    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start mock server
    let mock_server = MockServer::start().await;
    let base_url = mock_server.uri();

    // Expect URL built from variables
    Mock::given(method("GET"))
        .and(path("/api/v2/production/users"))
        .respond_with(ResponseTemplate::new(200).set_body_string("success"))
        .mount(&mock_server)
        .await;

    // Create tool with base_url parameter
    let tool_url = format!("{}/{{{{base_url}}}}/users", base_url);

    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "get_users",
        "GET",
        Some(&tool_url),
        None,
        None,
        5000,
    )
    .await
    .unwrap();

    let tool = Tool::get_by_id(&pool, tool_id).await.unwrap().unwrap();

    let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    // Add server globals for variable substitution
    sqlx::query!(
        "INSERT INTO server_globals (server_id, key, value, is_secret)
         VALUES (?, 'api_version', 'v2', false),
                (?, 'environment', 'production', false)",
        server_id,
        server_id
    )
    .execute(&pool)
    .await
    .unwrap();

    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'get_users', 'Get users')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // Instance param uses variable substitution
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'base_url', 'instance', 'api/{{api_version}}/{{environment}}')",
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Execute
    let secrets = SecretsManager::new().unwrap();
    let executor = InstanceExecutor::new(pool.clone(), server_id, instance_id, tool, secrets);

    let result = executor.execute(None).await;

    assert!(result.is_ok());
    let call_result = result.unwrap();
    assert!(call_result.is_error.is_none() || !call_result.is_error.unwrap());
}

#[tokio::test]
async fn test_multiple_instances_isolated() {
    let pool = test_helpers::create_test_db().await.unwrap();

    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start mock server
    let mock_server = MockServer::start().await;
    let base_url = mock_server.uri();

    // Mock two different endpoints
    Mock::given(method("GET"))
        .and(path("/staging/users/1"))
        .respond_with(ResponseTemplate::new(200).set_body_string("staging user 1"))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/production/users/2"))
        .respond_with(ResponseTemplate::new(200).set_body_string("production user 2"))
        .mount(&mock_server)
        .await;

    // Create tool
    let tool_url = format!("{}/{{{{env}}}}/users/{{{{integer:user_id}}}}", base_url);

    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "get_user",
        "GET",
        Some(&tool_url),
        None,
        None,
        5000,
    )
    .await
    .unwrap();

    let tool = Tool::get_by_id(&pool, tool_id).await.unwrap().unwrap();

    let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    // Create two instances with different configs
    let staging_instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'get_user_staging', 'Get user from staging')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    let prod_instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'get_user_prod', 'Get user from production')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // Configure staging instance
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'env', 'instance', 'staging'),
                (?, 'user_id', 'exposed', NULL)",
        staging_instance_id,
        staging_instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Configure production instance
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'env', 'instance', 'production'),
                (?, 'user_id', 'exposed', NULL)",
        prod_instance_id,
        prod_instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Execute both instances
    let secrets = SecretsManager::new().unwrap();

    let staging_executor = InstanceExecutor::new(
        pool.clone(),
        server_id,
        staging_instance_id,
        tool.clone(),
        secrets.clone(),
    );
    let prod_executor =
        InstanceExecutor::new(pool.clone(), server_id, prod_instance_id, tool, secrets);

    let mut staging_params = serde_json::Map::new();
    staging_params.insert("user_id".to_string(), json!(1));

    let mut prod_params = serde_json::Map::new();
    prod_params.insert("user_id".to_string(), json!(2));

    let staging_result = staging_executor
        .execute(Some(staging_params))
        .await
        .unwrap();
    let prod_result = prod_executor.execute(Some(prod_params)).await.unwrap();

    // Verify both executed independently with correct configs
    let staging_content = &staging_result.content[0];
    if let rmcp::model::RawContent::Text(text) = &**staging_content {
        assert!(text.text.contains("staging"));
    } else {
        panic!("Expected text content");
    }

    let prod_content = &prod_result.content[0];
    if let rmcp::model::RawContent::Text(text) = &**prod_content {
        assert!(text.text.contains("production"));
    } else {
        panic!("Expected text content");
    }
}

#[tokio::test]
async fn test_schema_matches_execution() {
    let pool = test_helpers::create_test_db().await.unwrap();

    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start mock server
    let mock_server = MockServer::start().await;
    let base_url = mock_server.uri();

    Mock::given(method("POST"))
        .and(path("/users"))
        .and(body_string(r#"{"name":"Test","age":30,"active":true}"#))
        .respond_with(ResponseTemplate::new(201).set_body_string("created"))
        .mount(&mock_server)
        .await;

    // Create tool with typed parameters
    let tool_url = format!("{}/users", base_url);
    let body = r#"{"name":"{{name}}","age":{{integer:age}},"active":{{boolean:active}}}"#;

    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "create_user",
        "POST",
        Some(&tool_url),
        None,
        Some(body),
        5000,
    )
    .await
    .unwrap();

    let tool = Tool::get_by_id(&pool, tool_id).await.unwrap().unwrap();

    let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'create_user', 'Create user')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // All params are exposed
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'name', 'exposed', NULL),
                (?, 'age', 'exposed', NULL),
                (?, 'active', 'exposed', NULL)",
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

    // Verify schema structure
    let properties = schema["properties"].as_object().unwrap();
    assert_eq!(properties.len(), 3);
    assert_eq!(properties["name"]["type"], "string");
    assert_eq!(properties["age"]["type"], "integer");
    assert_eq!(properties["active"]["type"], "boolean");

    let required = schema["required"].as_array().unwrap();
    assert_eq!(required.len(), 3);

    // Execute with parameters matching schema
    let secrets = SecretsManager::new().unwrap();
    let executor = InstanceExecutor::new(pool.clone(), server_id, instance_id, tool, secrets);

    let mut llm_params = serde_json::Map::new();
    llm_params.insert("name".to_string(), json!("Test"));
    llm_params.insert("age".to_string(), json!(30));
    llm_params.insert("active".to_string(), json!(true));

    let result = executor.execute(Some(llm_params)).await;

    assert!(result.is_ok());
    let call_result = result.unwrap();
    assert!(call_result.is_error.is_none() || !call_result.is_error.unwrap());
}

#[tokio::test]
async fn test_type_casting_in_execution() {
    let pool = test_helpers::create_test_db().await.unwrap();

    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start mock server
    let mock_server = MockServer::start().await;
    let base_url = mock_server.uri();

    // Expect properly typed values in body
    Mock::given(method("POST"))
        .and(path("/data"))
        .and(body_string(r#"{"count":42,"price":19.99,"enabled":true}"#))
        .respond_with(ResponseTemplate::new(200).set_body_string("success"))
        .mount(&mock_server)
        .await;

    // Create tool with typed parameters
    let tool_url = format!("{}/data", base_url);
    let body =
        r#"{"count":{{integer:count}},"price":{{number:price}},"enabled":{{boolean:enabled}}}"#;

    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "send_data",
        "POST",
        Some(&tool_url),
        None,
        Some(body),
        5000,
    )
    .await
    .unwrap();

    let tool = Tool::get_by_id(&pool, tool_id).await.unwrap().unwrap();

    let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'send_data', 'Send data')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // Exposed parameters
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'count', 'exposed', NULL),
                (?, 'price', 'exposed', NULL),
                (?, 'enabled', 'exposed', NULL)",
        instance_id,
        instance_id,
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Execute with properly typed JSON values
    let secrets = SecretsManager::new().unwrap();
    let executor = InstanceExecutor::new(pool.clone(), server_id, instance_id, tool, secrets);

    let mut llm_params = serde_json::Map::new();
    llm_params.insert("count".to_string(), json!(42));
    llm_params.insert("price".to_string(), json!(19.99));
    llm_params.insert("enabled".to_string(), json!(true));

    let result = executor.execute(Some(llm_params)).await;

    assert!(result.is_ok());
    let call_result = result.unwrap();
    assert!(call_result.is_error.is_none() || !call_result.is_error.unwrap());
}
