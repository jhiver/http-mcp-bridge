//! Tests for InstanceExecutor - HTTP execution of tool instances
//!
//! These tests verify that InstanceExecutor correctly:
//! - Executes HTTP requests for tool instances
//! - Resolves parameters from all three sources (instance, server, exposed)
//! - Formats MCP results correctly (success and error)
//! - Handles HTTP errors and timeouts
//! - Decrypts secrets during execution

use saramcp::models::tool::Tool;
use saramcp::services::{InstanceExecutor, SecretsManager};
use saramcp::test_utils::test_helpers;
use serde_json::json;
use wiremock::matchers::{body_string, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ============================================================================
// Basic Execution Tests
// ============================================================================

#[tokio::test]
async fn test_execute_simple_get_request() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start mock server
    let mock_server = MockServer::start().await;
    let mock_url = format!("{}/api/users", mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/api/users"))
        .respond_with(ResponseTemplate::new(200).set_body_string("success"))
        .mount(&mock_server)
        .await;

    // Create tool
    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "get_users",
        "GET",
        Some(&mock_url),
        None,
        None,
        5000,
    )
    .await
    .unwrap();

    // Load tool
    let tool = Tool::get_by_id(&pool, tool_id).await.unwrap().unwrap();

    let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
        .await
        .unwrap();

    // Create instance
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

    // Execute
    let secrets = SecretsManager::new().unwrap();
    let executor = InstanceExecutor::new(pool.clone(), server_id, instance_id, tool, secrets);
    let result = executor.execute(None).await;

    assert!(result.is_ok());
    let call_result = result.unwrap();
    assert!(call_result.is_error.is_none() || !call_result.is_error.unwrap());
}

#[tokio::test]
async fn test_execute_with_instance_param() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start mock server
    let mock_server = MockServer::start().await;
    let base_url = mock_server.uri();

    Mock::given(method("GET"))
        .and(path("/users/42"))
        .respond_with(ResponseTemplate::new(200).set_body_string("user 42 data"))
        .mount(&mock_server)
        .await;

    // Create tool with parameter
    let tool_url = format!("{}/users/{{{{integer:user_id}}}}", base_url);
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

    // Add instance param (fixed value)
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'user_id', 'instance', '42')",
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

    // Verify response contains expected data
    let content = &call_result.content[0];
    if let rmcp::model::RawContent::Text(text) = &**content {
        assert!(text.text.contains("user 42 data"));
    } else {
        panic!("Expected text content");
    }
}

#[tokio::test]
async fn test_execute_with_server_param() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start mock server
    let mock_server = MockServer::start().await;
    let base_url = mock_server.uri();

    Mock::given(method("GET"))
        .and(path("/api/data"))
        .and(header("Authorization", "Bearer secret-token"))
        .respond_with(ResponseTemplate::new(200).set_body_string("authenticated data"))
        .mount(&mock_server)
        .await;

    // Create tool with parameter in headers
    let tool_url = format!("{}/api/data", base_url);
    let headers = r#"{"Authorization":"Bearer {{api_token}}"}"#;
    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "get_data",
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

    // Add server global
    sqlx::query!(
        "INSERT INTO server_globals (server_id, key, value, is_secret)
         VALUES (?, 'api_token', 'secret-token', false)",
        server_id
    )
    .execute(&pool)
    .await
    .unwrap();

    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'get_data', 'Get data')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // Add instance param (source=server)
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'api_token', 'server', NULL)",
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

    let content = &call_result.content[0];
    if let rmcp::model::RawContent::Text(text) = &**content {
        assert!(text.text.contains("authenticated data"));
    } else {
        panic!("Expected text content");
    }
}

#[tokio::test]
async fn test_execute_with_exposed_param() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start mock server
    let mock_server = MockServer::start().await;
    let base_url = mock_server.uri();

    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_string("user 123 data"))
        .mount(&mock_server)
        .await;

    // Create tool
    let tool_url = format!("{}/users/{{{{integer:user_id}}}}", base_url);
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

    // Add instance param (source=exposed)
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'user_id', 'exposed', NULL)",
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Execute with LLM-provided parameter
    let secrets = SecretsManager::new().unwrap();
    let executor = InstanceExecutor::new(pool.clone(), server_id, instance_id, tool, secrets);

    let mut llm_params = serde_json::Map::new();
    llm_params.insert("user_id".to_string(), json!(123));

    let result = executor.execute(Some(llm_params)).await;

    assert!(result.is_ok());
    let call_result = result.unwrap();
    assert!(call_result.is_error.is_none() || !call_result.is_error.unwrap());

    let content = &call_result.content[0];
    if let rmcp::model::RawContent::Text(text) = &**content {
        assert!(text.text.contains("user 123 data"));
    } else {
        panic!("Expected text content");
    }
}

// ============================================================================
// Parameter Resolution Tests
// ============================================================================

#[tokio::test]
async fn test_execute_with_mixed_params() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start mock server
    let mock_server = MockServer::start().await;
    let base_url = mock_server.uri();

    Mock::given(method("GET"))
        .and(path("/api/v2/users/123"))
        .and(header("Authorization", "Bearer server-token"))
        .respond_with(ResponseTemplate::new(200).set_body_string("mixed params success"))
        .mount(&mock_server)
        .await;

    // Create tool with multiple parameters
    let tool_url = format!(
        "{}/api/{{{{version}}}}/users/{{{{integer:user_id}}}}",
        base_url
    );
    let headers = r#"{"Authorization":"Bearer {{api_token}}"}"#;
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

    // Add server global
    sqlx::query!(
        "INSERT INTO server_globals (server_id, key, value, is_secret)
         VALUES (?, 'api_token', 'server-token', false)",
        server_id
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

    // Add instance params with mixed sources
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'version', 'instance', 'v2'),
                (?, 'api_token', 'server', NULL),
                (?, 'user_id', 'exposed', NULL)",
        instance_id,
        instance_id,
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Execute with LLM-provided parameter
    let secrets = SecretsManager::new().unwrap();
    let executor = InstanceExecutor::new(pool.clone(), server_id, instance_id, tool, secrets);

    let mut llm_params = serde_json::Map::new();
    llm_params.insert("user_id".to_string(), json!(123));

    let result = executor.execute(Some(llm_params)).await;

    assert!(result.is_ok());
    let call_result = result.unwrap();
    assert!(call_result.is_error.is_none() || !call_result.is_error.unwrap());

    let content = &call_result.content[0];
    if let rmcp::model::RawContent::Text(text) = &**content {
        assert!(text.text.contains("mixed params success"));
    } else {
        panic!("Expected text content");
    }
}

#[tokio::test]
async fn test_execute_with_secret_param() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start mock server
    let mock_server = MockServer::start().await;
    let base_url = mock_server.uri();

    Mock::given(method("GET"))
        .and(path("/api/data"))
        .and(header("X-API-Key", "my-secret-key"))
        .respond_with(ResponseTemplate::new(200).set_body_string("secret data"))
        .mount(&mock_server)
        .await;

    // Create tool
    let tool_url = format!("{}/api/data", base_url);
    let headers = r#"{"X-API-Key":"{{api_key}}"}"#;
    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "get_data",
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

    // Add encrypted server global
    let secrets = SecretsManager::new().unwrap();
    let encrypted = secrets.encrypt("my-secret-key").unwrap();

    sqlx::query!(
        "INSERT INTO server_globals (server_id, key, value, is_secret)
         VALUES (?, 'api_key', ?, true)",
        server_id,
        encrypted
    )
    .execute(&pool)
    .await
    .unwrap();

    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'get_data', 'Get data')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // Add instance param (source=server for encrypted secret)
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'api_key', 'server', NULL)",
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Execute
    let executor = InstanceExecutor::new(pool.clone(), server_id, instance_id, tool, secrets);
    let result = executor.execute(None).await;

    assert!(result.is_ok());
    let call_result = result.unwrap();
    assert!(call_result.is_error.is_none() || !call_result.is_error.unwrap());

    let content = &call_result.content[0];
    if let rmcp::model::RawContent::Text(text) = &**content {
        assert!(text.text.contains("secret data"));
    } else {
        panic!("Expected text content");
    }
}

// ============================================================================
// HTTP Method Tests
// ============================================================================

#[tokio::test]
async fn test_execute_post_with_body() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
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
        .and(path("/api/users"))
        .and(body_string(r#"{"name":"Alice","age":25}"#))
        .respond_with(ResponseTemplate::new(201).set_body_string("user created"))
        .mount(&mock_server)
        .await;

    // Create tool
    let tool_url = format!("{}/api/users", base_url);
    let body = r#"{"name":"{{name}}","age":{{integer:age}}}"#;
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

    // Add instance params
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'name', 'exposed', NULL),
                (?, 'age', 'exposed', NULL)",
        instance_id,
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Execute
    let secrets = SecretsManager::new().unwrap();
    let executor = InstanceExecutor::new(pool.clone(), server_id, instance_id, tool, secrets);

    let mut llm_params = serde_json::Map::new();
    llm_params.insert("name".to_string(), json!("Alice"));
    llm_params.insert("age".to_string(), json!(25));

    let result = executor.execute(Some(llm_params)).await;

    assert!(result.is_ok());
    let call_result = result.unwrap();
    assert!(call_result.is_error.is_none() || !call_result.is_error.unwrap());

    let content = &call_result.content[0];
    if let rmcp::model::RawContent::Text(text) = &**content {
        assert!(text.text.contains("user created"));
    } else {
        panic!("Expected text content");
    }
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_execute_http_error_status() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start mock server
    let mock_server = MockServer::start().await;
    let mock_url = format!("{}/api/notfound", mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/api/notfound"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&mock_server)
        .await;

    // Create tool
    let tool_id = test_helpers::create_test_tool(
        &pool,
        toolkit_id,
        "get_notfound",
        "GET",
        Some(&mock_url),
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

    let instance_id = sqlx::query!(
        "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
         VALUES (?, ?, 'get_notfound', 'Get notfound')",
        server_id,
        tool_id
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_rowid();

    // Execute
    let secrets = SecretsManager::new().unwrap();
    let executor = InstanceExecutor::new(pool.clone(), server_id, instance_id, tool, secrets);
    let result = executor.execute(None).await;

    assert!(result.is_ok());
    let call_result = result.unwrap();

    // HTTP errors should be returned as error results, not Err
    assert!(call_result.is_error.unwrap_or(false));

    let content = &call_result.content[0];
    if let rmcp::model::RawContent::Text(text) = &**content {
        assert!(text.text.contains("404") || text.text.contains("not found"));
    } else {
        panic!("Expected text content");
    }
}

#[tokio::test]
async fn test_execute_missing_exposed_param() {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create test data
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();

    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start mock server (won't be reached)
    let mock_server = MockServer::start().await;
    let tool_url = format!("{}/users/{{{{integer:user_id}}}}", mock_server.uri());

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

    // Add instance param (source=exposed)
    sqlx::query!(
        "INSERT INTO instance_params (instance_id, param_name, source, value)
         VALUES (?, 'user_id', 'exposed', NULL)",
        instance_id
    )
    .execute(&pool)
    .await
    .unwrap();

    // Execute WITHOUT providing the required exposed parameter
    let secrets = SecretsManager::new().unwrap();
    let executor = InstanceExecutor::new(pool.clone(), server_id, instance_id, tool, secrets);

    let result = executor.execute(None).await;

    // Should fail with parameter resolution error OR HTTP execution error
    // (if parameter defaults to empty and template fails)
    assert!(result.is_err());
    let err = result.unwrap_err();
    // Accept various error messages related to missing parameter
    let err_msg = err.message.to_lowercase();
    assert!(
        err_msg.contains("parameter")
            || err_msg.contains("resolution")
            || err_msg.contains("template")
            || err_msg.contains("invalid"),
        "Unexpected error message: {}",
        err.message
    );
}
