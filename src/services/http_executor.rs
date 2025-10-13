//! HTTP execution service for SaraMCP
//!
//! This module provides a reusable HTTP client for executing tool requests with
//! parameter substitution. It is channel-agnostic and can be used by MCP handlers,
//! web UI, or CLI interfaces.
//!
//! # Features
//!
//! - Template-based HTTP request building with parameter substitution
//! - Support for all standard HTTP methods (GET, POST, PUT, DELETE, PATCH)
//! - Dynamic URL, header, and body rendering using the TypedVariableEngine
//! - Configurable timeouts per tool
//! - Comprehensive error handling with typed errors
//!
//! # Example
//!
//! ```rust,no_run
//! use saramcp::services::HttpExecutor;
//! use saramcp::models::tool::Tool;
//! use serde_json::json;
//! use std::collections::HashMap;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let executor = HttpExecutor::new();
//!
//! // Assuming you have a Tool with templates like:
//! // url: "https://api.example.com/users/{{user_id}}"
//! // headers: {"Authorization": "Bearer {{token}}"}
//! # let tool = todo!();
//!
//! let mut params = HashMap::new();
//! params.insert("user_id".to_string(), json!(123));
//! params.insert("token".to_string(), json!("abc123"));
//!
//! let result = executor.execute_tool(&tool, &params).await?;
//! println!("Status: {}", result.status);
//! println!("Body: {}", result.body);
//! # Ok(())
//! # }
//! ```

use crate::models::tool::Tool;
use crate::services::variable_engine::TypedVariableEngine;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

/// Errors that can occur during HTTP request execution
#[derive(Debug, thiserror::Error)]
pub enum HttpExecutorError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Invalid HTTP method: {0}")]
    InvalidMethod(String),

    #[error("Request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    #[error("Timeout after {0}ms")]
    Timeout(u64),

    #[error("Template rendering failed: {0}")]
    TemplateError(String),

    #[error("Invalid headers format: {0}")]
    InvalidHeaders(String),

    #[error("Response body read failed: {0}")]
    ResponseBodyError(String),
}

/// Result of executing an HTTP request
///
/// Contains the HTTP response status, body, headers, and success indicator.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// HTTP status code (e.g., 200, 404, 500)
    pub status: u16,
    /// Response body as a string
    pub body: String,
    /// Response headers as key-value pairs
    pub headers: HashMap<String, String>,
    /// True if status code is 2xx, false otherwise
    pub is_success: bool,
    /// Equivalent cURL command for debugging
    pub curl_command: String,
}

/// HTTP request executor with template rendering
///
/// Executes HTTP requests based on Tool templates, substituting parameters
/// into URLs, headers, and request bodies using the TypedVariableEngine.
///
/// # Thread Safety
///
/// HttpExecutor is safe to share across threads as the underlying reqwest::Client
/// uses connection pooling and is designed for concurrent use.
#[derive(Clone)]
pub struct HttpExecutor {
    client: reqwest::Client,
    engine: TypedVariableEngine,
}

impl Default for HttpExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpExecutor {
    /// Creates a new HttpExecutor with default configuration
    ///
    /// Initializes an HTTP client with a 30-second default timeout and
    /// a TypedVariableEngine for parameter substitution.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use saramcp::services::HttpExecutor;
    ///
    /// let executor = HttpExecutor::new();
    /// ```
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            client,
            engine: TypedVariableEngine::new(),
        }
    }

    fn render_url(
        &self,
        template_opt: Option<&str>,
        params: &HashMap<String, Value>,
    ) -> Result<String, HttpExecutorError> {
        let template = template_opt
            .ok_or_else(|| HttpExecutorError::InvalidUrl("URL template is required".to_string()))?;

        let context: HashMap<String, String> = params
            .iter()
            .map(|(k, v)| {
                let value_str = match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => v.to_string(),
                };
                (k.clone(), value_str)
            })
            .collect();

        self.engine
            .substitute(template, &context)
            .map_err(|e| HttpExecutorError::TemplateError(e.to_string()))
    }

    fn render_headers(
        &self,
        headers_json: Option<&str>,
        params: &HashMap<String, Value>,
    ) -> Result<HeaderMap, HttpExecutorError> {
        let mut header_map = HeaderMap::new();

        if let Some(json_str) = headers_json {
            let headers_value: Value = serde_json::from_str(json_str)
                .map_err(|e| HttpExecutorError::InvalidHeaders(e.to_string()))?;

            if let Value::Object(map) = headers_value {
                let context: HashMap<String, String> = params
                    .iter()
                    .map(|(k, v)| {
                        let value_str = match v {
                            Value::String(s) => s.clone(),
                            Value::Number(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            _ => v.to_string(),
                        };
                        (k.clone(), value_str)
                    })
                    .collect();

                for (key, value) in map {
                    if let Value::String(template) = value {
                        let rendered = self
                            .engine
                            .substitute(&template, &context)
                            .map_err(|e| HttpExecutorError::TemplateError(e.to_string()))?;

                        let header_name = HeaderName::from_bytes(key.as_bytes())
                            .map_err(|e| HttpExecutorError::InvalidHeaders(e.to_string()))?;
                        let header_value = HeaderValue::from_str(&rendered)
                            .map_err(|e| HttpExecutorError::InvalidHeaders(e.to_string()))?;

                        header_map.insert(header_name, header_value);
                    }
                }
            }
        }

        Ok(header_map)
    }

    fn render_body(
        &self,
        body_json: Option<&str>,
        params: &HashMap<String, Value>,
    ) -> Result<Option<String>, HttpExecutorError> {
        if let Some(json_str) = body_json {
            let context: HashMap<String, String> = params
                .iter()
                .map(|(k, v)| {
                    let value_str = match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => b.to_string(),
                        _ => v.to_string(),
                    };
                    (k.clone(), value_str)
                })
                .collect();

            let rendered = self
                .engine
                .substitute(json_str, &context)
                .map_err(|e| HttpExecutorError::TemplateError(e.to_string()))?;

            Ok(Some(rendered))
        } else {
            Ok(None)
        }
    }

    fn generate_curl_command(
        &self,
        method: &str,
        url: &str,
        headers: &HeaderMap,
        body: &Option<String>,
    ) -> String {
        let mut curl_parts = vec![format!("curl -X {}", method.to_uppercase())];

        // Add headers
        for (key, value) in headers.iter() {
            if let Ok(value_str) = value.to_str() {
                curl_parts.push(format!("-H '{}: {}'", key, value_str));
            }
        }

        // Add body if present
        if let Some(body_content) = body {
            // Escape single quotes in body
            let escaped_body = body_content.replace('\'', "'\\''");
            curl_parts.push(format!("-d '{}'", escaped_body));
        }

        // Add URL (always last)
        curl_parts.push(format!("'{}'", url));

        curl_parts.join(" \\\n  ")
    }

    async fn format_response(
        &self,
        response: reqwest::Response,
        curl_command: String,
    ) -> Result<ExecutionResult, HttpExecutorError> {
        let status = response.status().as_u16();
        let is_success = response.status().is_success();

        let headers: HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let body = response
            .text()
            .await
            .map_err(|e| HttpExecutorError::ResponseBodyError(e.to_string()))?;

        Ok(ExecutionResult {
            status,
            body,
            headers,
            is_success,
            curl_command,
        })
    }

    /// Executes an HTTP request based on a Tool template with parameter substitution
    ///
    /// This method performs the following steps:
    /// 1. Renders the URL template with provided parameters
    /// 2. Renders headers template with provided parameters
    /// 3. Renders body template with provided parameters
    /// 4. Builds and executes the HTTP request with the tool's timeout
    /// 5. Formats and returns the response
    ///
    /// # Arguments
    ///
    /// * `tool` - The Tool containing HTTP method, URL, headers, body templates, and timeout
    /// * `params` - Parameter values to substitute into templates (e.g., {"user_id": 123})
    ///
    /// # Returns
    ///
    /// * `Ok(ExecutionResult)` - Successful execution with status, body, headers, and success flag
    /// * `Err(HttpExecutorError)` - Various error conditions including:
    ///   - `InvalidUrl` - Missing or invalid URL template
    ///   - `InvalidMethod` - Invalid HTTP method
    ///   - `Timeout` - Request exceeded tool's timeout_ms
    ///   - `TemplateError` - Parameter substitution failed
    ///   - `InvalidHeaders` - Header parsing or rendering failed
    ///   - `RequestFailed` - Network or HTTP error
    ///   - `ResponseBodyError` - Failed to read response body
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use saramcp::services::HttpExecutor;
    /// use serde_json::json;
    /// use std::collections::HashMap;
    /// # use saramcp::models::tool::Tool;
    ///
    /// # async fn example(tool: Tool) -> Result<(), Box<dyn std::error::Error>> {
    /// let executor = HttpExecutor::new();
    ///
    /// let mut params = HashMap::new();
    /// params.insert("user_id".to_string(), json!(123));
    /// params.insert("api_key".to_string(), json!("secret"));
    ///
    /// let result = executor.execute_tool(&tool, &params).await?;
    ///
    /// if result.is_success {
    ///     println!("Success: {}", result.body);
    /// } else {
    ///     println!("Error {}: {}", result.status, result.body);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn execute_tool(
        &self,
        tool: &Tool,
        params: &HashMap<String, Value>,
    ) -> Result<ExecutionResult, HttpExecutorError> {
        let url = self.render_url(tool.url.as_deref(), params)?;
        let headers = self.render_headers(tool.headers.as_deref(), params)?;
        let body = self.render_body(tool.body.as_deref(), params)?;

        let method = reqwest::Method::from_bytes(tool.method.as_bytes())
            .map_err(|_| HttpExecutorError::InvalidMethod(tool.method.clone()))?;

        let timeout = Duration::from_millis(tool.timeout_ms as u64);

        // Generate cURL command for debugging
        let curl_command = self.generate_curl_command(&tool.method, &url, &headers, &body);

        let mut request_builder = self.client.request(method, &url).headers(headers);

        if let Some(body_content) = &body {
            request_builder = request_builder.body(body_content.clone());
        }

        let request = request_builder
            .timeout(timeout)
            .build()
            .map_err(HttpExecutorError::RequestFailed)?;

        let response = self.client.execute(request).await.map_err(|e| {
            if e.is_timeout() {
                HttpExecutorError::Timeout(timeout.as_millis() as u64)
            } else {
                HttpExecutorError::RequestFailed(e)
            }
        })?;

        self.format_response(response, curl_command).await
    }
}
