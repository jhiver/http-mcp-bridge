use saramcp::models::tool::Tool;
use saramcp::services::{HttpExecutor, HttpExecutorError};
use saramcp::test_utils::test_helpers;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::HashMap;
use wiremock::matchers::{body_string, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// Helper struct for creating test tools
struct TestToolBuilder<'a> {
    pool: &'a SqlitePool,
    toolkit_id: i64,
    name: &'a str,
    method: &'a str,
    url: Option<&'a str>,
    headers: Option<&'a str>,
    body: Option<&'a str>,
    timeout_ms: i32,
}

impl<'a> TestToolBuilder<'a> {
    fn new(pool: &'a SqlitePool, toolkit_id: i64, name: &'a str, method: &'a str) -> Self {
        Self {
            pool,
            toolkit_id,
            name,
            method,
            url: None,
            headers: None,
            body: None,
            timeout_ms: 5000,
        }
    }

    fn url(mut self, url: &'a str) -> Self {
        self.url = Some(url);
        self
    }

    fn headers(mut self, headers: &'a str) -> Self {
        self.headers = Some(headers);
        self
    }

    fn body(mut self, body: &'a str) -> Self {
        self.body = Some(body);
        self
    }

    fn timeout_ms(mut self, timeout_ms: i32) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    async fn build(self) -> Tool {
        use sqlx::Row;

        let row = sqlx::query(
            r#"
            INSERT INTO tools (toolkit_id, name, description, method, url, headers, body, timeout_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            RETURNING id, toolkit_id, name, description, method, url, headers, body, timeout_ms, created_at, updated_at
            "#,
        )
        .bind(self.toolkit_id)
        .bind(self.name)
        .bind("Test tool")
        .bind(self.method)
        .bind(self.url)
        .bind(self.headers)
        .bind(self.body)
        .bind(self.timeout_ms)
        .fetch_one(self.pool)
        .await
        .expect("Failed to create test tool");

        Tool {
            id: row.get("id"),
            toolkit_id: row.get("toolkit_id"),
            name: row.get("name"),
            description: row.get("description"),
            method: row.get("method"),
            url: row.get("url"),
            headers: row.get("headers"),
            body: row.get("body"),
            timeout_ms: row.get("timeout_ms"),
            created_at: chrono::DateTime::from_timestamp(row.get::<i64, _>("created_at"), 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
            updated_at: chrono::DateTime::from_timestamp(row.get::<i64, _>("updated_at"), 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
        }
    }
}

// Helper to create params HashMap
fn create_params(pairs: Vec<(&str, Value)>) -> HashMap<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

#[tokio::test]
async fn test_execute_get_request_success() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Mock GET endpoint with 200 response
    Mock::given(method("GET"))
        .and(path("/api/users"))
        .respond_with(ResponseTemplate::new(200).set_body_string("success"))
        .mount(&mock_server)
        .await;

    // Create tool with mock server URL
    let tool_url = format!("{}/api/users", mock_server.uri());
    let tool = TestToolBuilder::new(&pool, toolkit_id, "get_users", "GET")
        .url(&tool_url)
        .build()
        .await;

    // Execute tool
    let executor = HttpExecutor::new();
    let params = HashMap::new();
    let result = executor.execute_tool(&tool, &params).await;

    // Assert success
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    assert_eq!(execution_result.status, 200);
    assert_eq!(execution_result.body, "success");
    assert!(execution_result.is_success);
}

#[tokio::test]
async fn test_execute_post_with_json_body() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Mock expects specific JSON body
    Mock::given(method("POST"))
        .and(path("/api/users"))
        .and(body_string(r#"{"username":"alice","age":25}"#))
        .respond_with(ResponseTemplate::new(201).set_body_string("user created"))
        .mount(&mock_server)
        .await;

    // Create tool with POST method and body template
    let tool_url = format!("{}/api/users", mock_server.uri());
    let body = r#"{"username":"{{username}}","age":{{age}}}"#;
    let tool = TestToolBuilder::new(&pool, toolkit_id, "create_user", "POST")
        .url(&tool_url)
        .body(body)
        .build()
        .await;

    // Execute with parameters
    let executor = HttpExecutor::new();
    let params = create_params(vec![("username", json!("alice")), ("age", json!(25))]);
    let result = executor.execute_tool(&tool, &params).await;

    // Assert success
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    assert_eq!(execution_result.status, 201);
    assert_eq!(execution_result.body, "user created");
    assert!(execution_result.is_success);
}

#[tokio::test]
async fn test_parameter_substitution_in_url() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Mock expects URL with substituted parameters
    Mock::given(method("GET"))
        .and(path("/users/123"))
        .respond_with(ResponseTemplate::new(200).set_body_string("user data"))
        .mount(&mock_server)
        .await;

    // Create tool with URL template
    let tool_url = format!("{}/users/{{{{user_id}}}}", mock_server.uri());
    let tool = TestToolBuilder::new(&pool, toolkit_id, "get_user", "GET")
        .url(&tool_url)
        .build()
        .await;

    // Execute with parameters
    let executor = HttpExecutor::new();
    let params = create_params(vec![("user_id", json!(123))]);
    let result = executor.execute_tool(&tool, &params).await;

    // Assert success - correct URL was requested
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    assert_eq!(execution_result.status, 200);
    assert_eq!(execution_result.body, "user data");
}

#[tokio::test]
async fn test_parameter_substitution_in_headers() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Mock expects specific Authorization header
    Mock::given(method("GET"))
        .and(path("/api/protected"))
        .and(header("Authorization", "Bearer abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_string("authenticated"))
        .mount(&mock_server)
        .await;

    // Create tool with headers template
    let tool_url = format!("{}/api/protected", mock_server.uri());
    let headers = r#"{"Authorization":"Bearer {{token}}"}"#;
    let tool = TestToolBuilder::new(&pool, toolkit_id, "protected_endpoint", "GET")
        .url(&tool_url)
        .headers(headers)
        .build()
        .await;

    // Execute with parameters
    let executor = HttpExecutor::new();
    let params = create_params(vec![("token", json!("abc123"))]);
    let result = executor.execute_tool(&tool, &params).await;

    // Assert success - header was sent correctly
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    assert_eq!(execution_result.status, 200);
    assert_eq!(execution_result.body, "authenticated");
}

#[tokio::test]
async fn test_parameter_substitution_in_body() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Mock expects specific body content
    Mock::given(method("POST"))
        .and(path("/api/register"))
        .and(body_string(
            r#"{"username":"alice","age":25,"active":true}"#,
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string("registered"))
        .mount(&mock_server)
        .await;

    // Create tool with body template
    let tool_url = format!("{}/api/register", mock_server.uri());
    let body = r#"{"username":"{{username}}","age":{{age}},"active":{{active}}}"#;
    let tool = TestToolBuilder::new(&pool, toolkit_id, "register_user", "POST")
        .url(&tool_url)
        .body(body)
        .build()
        .await;

    // Execute with parameters
    let executor = HttpExecutor::new();
    let params = create_params(vec![
        ("username", json!("alice")),
        ("age", json!(25)),
        ("active", json!(true)),
    ]);
    let result = executor.execute_tool(&tool, &params).await;

    // Assert success - body was rendered correctly
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    assert_eq!(execution_result.status, 200);
    assert_eq!(execution_result.body, "registered");
}

#[tokio::test]
async fn test_all_http_methods() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;
    let executor = HttpExecutor::new();
    let params = HashMap::new();

    // Test GET
    Mock::given(method("GET"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(200).set_body_string("GET success"))
        .mount(&mock_server)
        .await;

    let test_url = format!("{}/test", mock_server.uri());
    let get_tool = TestToolBuilder::new(&pool, toolkit_id, "get_test", "GET")
        .url(&test_url)
        .build()
        .await;
    let result = executor.execute_tool(&get_tool, &params).await.unwrap();
    assert_eq!(result.status, 200);
    assert_eq!(result.body, "GET success");

    // Test POST
    Mock::given(method("POST"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(201).set_body_string("POST success"))
        .mount(&mock_server)
        .await;

    let post_tool = TestToolBuilder::new(&pool, toolkit_id, "post_test", "POST")
        .url(&test_url)
        .build()
        .await;
    let result = executor.execute_tool(&post_tool, &params).await.unwrap();
    assert_eq!(result.status, 201);
    assert_eq!(result.body, "POST success");

    // Test PUT
    Mock::given(method("PUT"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(200).set_body_string("PUT success"))
        .mount(&mock_server)
        .await;

    let put_tool = TestToolBuilder::new(&pool, toolkit_id, "put_test", "PUT")
        .url(&test_url)
        .build()
        .await;
    let result = executor.execute_tool(&put_tool, &params).await.unwrap();
    assert_eq!(result.status, 200);
    assert_eq!(result.body, "PUT success");

    // Test DELETE
    Mock::given(method("DELETE"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(204).set_body_string(""))
        .mount(&mock_server)
        .await;

    let delete_tool = TestToolBuilder::new(&pool, toolkit_id, "delete_test", "DELETE")
        .url(&test_url)
        .build()
        .await;
    let result = executor.execute_tool(&delete_tool, &params).await.unwrap();
    assert_eq!(result.status, 204);

    // Test PATCH
    Mock::given(method("PATCH"))
        .and(path("/test"))
        .respond_with(ResponseTemplate::new(200).set_body_string("PATCH success"))
        .mount(&mock_server)
        .await;

    let patch_tool = TestToolBuilder::new(&pool, toolkit_id, "patch_test", "PATCH")
        .url(&test_url)
        .build()
        .await;
    let result = executor.execute_tool(&patch_tool, &params).await.unwrap();
    assert_eq!(result.status, 200);
    assert_eq!(result.body, "PATCH success");
}

#[tokio::test]
async fn test_execute_timeout() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Mock server with delay > timeout
    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("delayed")
                .set_delay(std::time::Duration::from_millis(200)),
        )
        .mount(&mock_server)
        .await;

    // Create tool with very short timeout
    let tool_url = format!("{}/slow", mock_server.uri());
    let tool = TestToolBuilder::new(&pool, toolkit_id, "slow_endpoint", "GET")
        .url(&tool_url)
        .timeout_ms(100) // 100ms timeout
        .build()
        .await;

    // Execute tool
    let executor = HttpExecutor::new();
    let params = HashMap::new();
    let result = executor.execute_tool(&tool, &params).await;

    // Assert timeout error
    assert!(result.is_err());
    match result.unwrap_err() {
        HttpExecutorError::Timeout(ms) => assert_eq!(ms, 100),
        _ => panic!("Expected Timeout error"),
    }
}

#[tokio::test]
async fn test_invalid_url_error() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Create tool with None URL
    let tool = TestToolBuilder::new(&pool, toolkit_id, "no_url_tool", "GET")
        .build()
        .await;

    // Execute tool
    let executor = HttpExecutor::new();
    let params = HashMap::new();
    let result = executor.execute_tool(&tool, &params).await;

    // Assert InvalidUrl error
    assert!(result.is_err());
    match result.unwrap_err() {
        HttpExecutorError::InvalidUrl(_) => (),
        e => panic!("Expected InvalidUrl error, got: {:?}", e),
    }
}

#[tokio::test]
async fn test_invalid_http_method() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Create a valid tool first (database has CHECK constraint on method)
    let tool_url = format!("{}/test", mock_server.uri());
    let mut tool = TestToolBuilder::new(&pool, toolkit_id, "invalid_method", "GET")
        .url(&tool_url)
        .build()
        .await;

    // Manually set method with invalid characters (spaces are not allowed in HTTP methods)
    // reqwest::Method::from_bytes will reject methods with invalid characters
    tool.method = "INVALID METHOD".to_string();

    // Execute tool
    let executor = HttpExecutor::new();
    let params = HashMap::new();
    let result = executor.execute_tool(&tool, &params).await;

    // Assert InvalidMethod error
    assert!(result.is_err());
    match result.unwrap_err() {
        HttpExecutorError::InvalidMethod(method) => assert_eq!(method, "INVALID METHOD"),
        e => panic!("Expected InvalidMethod error, got: {:?}", e),
    }
}

#[tokio::test]
async fn test_response_headers_captured() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Mock returns custom headers
    Mock::given(method("GET"))
        .and(path("/headers"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("response")
                .insert_header("x-custom-header", "custom-value")
                .insert_header("content-type", "text/plain"),
        )
        .mount(&mock_server)
        .await;

    // Create tool
    let tool_url = format!("{}/headers", mock_server.uri());
    let tool = TestToolBuilder::new(&pool, toolkit_id, "headers_test", "GET")
        .url(&tool_url)
        .build()
        .await;

    // Execute tool
    let executor = HttpExecutor::new();
    let params = HashMap::new();
    let result = executor.execute_tool(&tool, &params).await;

    // Assert headers are captured
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    assert!(execution_result.headers.contains_key("x-custom-header"));
    assert_eq!(
        execution_result.headers.get("x-custom-header").unwrap(),
        "custom-value"
    );
    assert!(execution_result.headers.contains_key("content-type"));
}

#[tokio::test]
async fn test_non_success_status_code() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Mock returns 404
    Mock::given(method("GET"))
        .and(path("/notfound"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&mock_server)
        .await;

    // Create tool
    let tool_url = format!("{}/notfound", mock_server.uri());
    let tool = TestToolBuilder::new(&pool, toolkit_id, "notfound_test", "GET")
        .url(&tool_url)
        .build()
        .await;

    // Execute tool
    let executor = HttpExecutor::new();
    let params = HashMap::new();
    let result = executor.execute_tool(&tool, &params).await;

    // Assert status=404, is_success=false
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    assert_eq!(execution_result.status, 404);
    assert!(!execution_result.is_success);
    assert_eq!(execution_result.body, "not found");
}

#[tokio::test]
async fn test_request_without_body() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Mock accepts request without body
    Mock::given(method("GET"))
        .and(path("/no-body"))
        .respond_with(ResponseTemplate::new(200).set_body_string("success"))
        .mount(&mock_server)
        .await;

    // Create tool with body=None
    let tool_url = format!("{}/no-body", mock_server.uri());
    let tool = TestToolBuilder::new(&pool, toolkit_id, "no_body_test", "GET")
        .url(&tool_url)
        .build()
        .await;

    // Execute tool
    let executor = HttpExecutor::new();
    let params = HashMap::new();
    let result = executor.execute_tool(&tool, &params).await;

    // Assert request succeeds without body
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    assert_eq!(execution_result.status, 200);
    assert_eq!(execution_result.body, "success");
}

#[tokio::test]
async fn test_complex_url_with_multiple_parameters() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Mock expects URL with multiple substituted parameters
    Mock::given(method("GET"))
        .and(path("/api/v1/users/42/posts/100"))
        .respond_with(ResponseTemplate::new(200).set_body_string("complex url success"))
        .mount(&mock_server)
        .await;

    // Create tool with complex URL template
    let tool_url = format!(
        "{}/api/{{{{version}}}}/users/{{{{user_id}}}}/posts/{{{{post_id}}}}",
        mock_server.uri()
    );
    let tool = TestToolBuilder::new(&pool, toolkit_id, "complex_url", "GET")
        .url(&tool_url)
        .build()
        .await;

    // Execute with multiple parameters
    let executor = HttpExecutor::new();
    let params = create_params(vec![
        ("version", json!("v1")),
        ("user_id", json!(42)),
        ("post_id", json!(100)),
    ]);
    let result = executor.execute_tool(&tool, &params).await;

    // Assert success
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    assert_eq!(execution_result.status, 200);
    assert_eq!(execution_result.body, "complex url success");
}

#[tokio::test]
async fn test_invalid_headers_json() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Create tool with invalid headers JSON
    let tool_url = format!("{}/test", mock_server.uri());
    let invalid_headers = "not valid json";
    let tool = TestToolBuilder::new(&pool, toolkit_id, "invalid_headers", "GET")
        .url(&tool_url)
        .headers(invalid_headers)
        .build()
        .await;

    // Execute tool
    let executor = HttpExecutor::new();
    let params = HashMap::new();
    let result = executor.execute_tool(&tool, &params).await;

    // Assert InvalidHeaders error
    assert!(result.is_err());
    match result.unwrap_err() {
        HttpExecutorError::InvalidHeaders(_) => (),
        e => panic!("Expected InvalidHeaders error, got: {:?}", e),
    }
}

#[tokio::test]
async fn test_empty_headers_and_body() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/empty"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&mock_server)
        .await;

    // Create tool with empty/None headers and body
    let tool_url = format!("{}/empty", mock_server.uri());
    let tool = TestToolBuilder::new(&pool, toolkit_id, "empty_test", "GET")
        .url(&tool_url)
        .build()
        .await;

    // Execute tool
    let executor = HttpExecutor::new();
    let params = HashMap::new();
    let result = executor.execute_tool(&tool, &params).await;

    // Assert success
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    assert_eq!(execution_result.status, 200);
    assert_eq!(execution_result.body, "ok");
}

#[tokio::test]
async fn test_parameter_with_special_characters() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/special"))
        .and(body_string(r#"{"message":"Hello, World! @#$%"}"#))
        .respond_with(ResponseTemplate::new(200).set_body_string("received"))
        .mount(&mock_server)
        .await;

    // Create tool with body template
    let tool_url = format!("{}/special", mock_server.uri());
    let body = r#"{"message":"{{message}}"}"#;
    let tool = TestToolBuilder::new(&pool, toolkit_id, "special_chars", "POST")
        .url(&tool_url)
        .body(body)
        .build()
        .await;

    // Execute with special characters in parameter
    let executor = HttpExecutor::new();
    let params = create_params(vec![("message", json!("Hello, World! @#$%"))]);
    let result = executor.execute_tool(&tool, &params).await;

    // Assert success
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    assert_eq!(execution_result.status, 200);
}

#[tokio::test]
async fn test_empty_response_body() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Mock returns empty body
    Mock::given(method("DELETE"))
        .and(path("/delete"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&mock_server)
        .await;

    // Create tool
    let tool_url = format!("{}/delete", mock_server.uri());
    let tool = TestToolBuilder::new(&pool, toolkit_id, "delete_test", "DELETE")
        .url(&tool_url)
        .build()
        .await;

    // Execute tool
    let executor = HttpExecutor::new();
    let params = HashMap::new();
    let result = executor.execute_tool(&tool, &params).await;

    // Assert success with empty body
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    assert_eq!(execution_result.status, 204);
    assert_eq!(execution_result.body, "");
    assert!(execution_result.is_success);
}

#[tokio::test]
async fn test_multiple_headers_substitution() {
    // Setup database
    let pool = test_helpers::create_test_db().await.unwrap();
    let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
        .await
        .unwrap();
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
        .await
        .unwrap();

    // Start wiremock server
    let mock_server = MockServer::start().await;

    // Mock expects multiple headers
    Mock::given(method("GET"))
        .and(path("/multi-headers"))
        .and(header("Authorization", "Bearer token123"))
        .and(header("X-Api-Key", "api-key-456"))
        .respond_with(ResponseTemplate::new(200).set_body_string("authorized"))
        .mount(&mock_server)
        .await;

    // Create tool with multiple headers template
    let tool_url = format!("{}/multi-headers", mock_server.uri());
    let headers = r#"{"Authorization":"Bearer {{token}}","X-Api-Key":"{{api_key}}"}"#;
    let tool = TestToolBuilder::new(&pool, toolkit_id, "multi_headers", "GET")
        .url(&tool_url)
        .headers(headers)
        .build()
        .await;

    // Execute with parameters
    let executor = HttpExecutor::new();
    let params = create_params(vec![
        ("token", json!("token123")),
        ("api_key", json!("api-key-456")),
    ]);
    let result = executor.execute_tool(&tool, &params).await;

    // Assert success
    assert!(result.is_ok());
    let execution_result = result.unwrap();
    assert_eq!(execution_result.status, 200);
    assert_eq!(execution_result.body, "authorized");
}
