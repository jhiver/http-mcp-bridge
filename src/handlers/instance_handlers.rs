use crate::error::AppError;
use crate::models::{ConfigureInstanceForm, ExtractedParameter, Server};
use crate::services::http_executor::ExecutionResult;
use crate::services::instance_executor::InstanceExecutor;
use crate::services::secrets_manager::SecretsManager;
use crate::services::variable_engine::VariableType;
use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    body::Bytes,
    extract::{FromRequest, Path, Query, Request, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    Form,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::HashMap;
use tower_sessions::Session;

// Custom form extractor that uses serde_qs for deserialization
pub struct QsForm<T>(pub T);

impl<T, S> FromRequest<S> for QsForm<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state).await.map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Failed to read body: {}", e),
            )
        })?;

        let body_str = std::str::from_utf8(&bytes)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid UTF-8: {}", e)))?;

        // Configure serde_qs to use bracket notation for parsing indexed arrays
        let config = serde_qs::Config::new(10, false);
        let value = config.deserialize_str(body_str).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Failed to parse form: {}", e),
            )
        })?;

        Ok(QsForm(value))
    }
}

#[derive(Deserialize)]
pub struct NewInstanceQuery {
    tool_id: i64,
}

// Templates
#[derive(Template, WebTemplate)]
#[template(path = "instances/configure.html")]
struct ConfigureInstanceTemplate {
    csrf_token: String,
    server: Server,
    tool: SimpleTool,
    tool_params: Vec<ExtractedParameter>,
    server_globals: Vec<String>,
    suggested_name: String,
    suggested_description: Option<String>,
    user_email: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "instances/edit.html")]
struct EditInstanceTemplate {
    csrf_token: String,
    server: Server,
    instance: crate::models::InstanceDetail,
    params_with_config: Vec<ParameterWithConfig>,
    signature: String,
    user_email: String,
}

// Simple tool struct for template
#[derive(serde::Serialize)]
struct SimpleTool {
    id: i64,
    name: String,
    description: Option<String>,
}

// Parameter with merged config for editing
#[derive(serde::Serialize)]
struct ParameterWithConfig {
    name: String,
    param_type: String,
    source: String,        // where the param comes from (url, header, body)
    config_source: String, // how it's configured (exposed, server, instance)
    config_value: Option<String>,
    server_binding: Option<String>, // value from server globals if applicable
    final_value: Option<String>,    // computed final value based on config_source
}

// Handlers
pub async fn configure_instance_page(
    State(state): State<AppState>,
    session: Session,
    Path(server_id): Path<i64>,
    Query(query): Query<NewInstanceQuery>,
) -> Result<Html<String>, StatusCode> {
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let user_email = session
        .get::<String>("email")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let csrf_token = session
        .get::<String>("csrf_token")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .unwrap_or_default();

    let server_service = state
        .server_service
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let tool_service = state
        .tool_service
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get server
    let server = server_service
        .get_server(server_id, user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Get tool
    let (tool_model, _) = tool_service
        .get_tool(query.tool_id, user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let tool = SimpleTool {
        id: tool_model.id,
        name: tool_model.name.clone(),
        description: tool_model.description.clone(),
    };

    // Extract tool parameters dynamically
    let tool_params = tool_model.extract_parameters();

    // Get server globals (for showing available defaults)
    let globals = server_service
        .get_server_globals(server_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let server_globals: Vec<String> = globals
        .into_iter()
        .filter(|g| !g.is_secret.unwrap_or(false))
        .map(|g| g.key)
        .collect();

    // Generate suggested instance name
    let instance_service = state
        .instance_service
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let suggested_name = instance_service
        .generate_instance_name(server_id, &tool_model.name)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let suggested_description = tool_model.description.clone();

    let template = ConfigureInstanceTemplate {
        csrf_token,
        server,
        tool,
        tool_params,
        server_globals,
        suggested_name,
        suggested_description,
        user_email,
    };

    Ok(Html(
        template
            .render()
            .unwrap_or_else(|_| "Template error".to_string()),
    ))
}

pub async fn create_instance_handler(
    State(state): State<AppState>,
    session: Session,
    Path(server_id): Path<i64>,
    QsForm(mut form): QsForm<ConfigureInstanceForm>,
) -> Result<impl IntoResponse, StatusCode> {
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let server_service = state
        .server_service
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Verify ownership
    if !server_service
        .user_owns_server(server_id, user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let instance_service = state
        .instance_service
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Generate unique instance name if needed
    let unique_name = instance_service
        .generate_instance_name(server_id, &form.instance_name)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    form.instance_name = unique_name;

    instance_service
        .create_instance_with_config(server_id, form)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    reload_server_tools(&state.pool, server_id, &state.mcp_registry).await;

    Ok(Redirect::to(&format!("/servers/{}?tab=tools", server_id)))
}

pub async fn edit_instance_page(
    State(state): State<AppState>,
    session: Session,
    Path((server_id, instance_id)): Path<(i64, i64)>,
) -> Result<Html<String>, StatusCode> {
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let user_email = session
        .get::<String>("email")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let csrf_token = session
        .get::<String>("csrf_token")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .unwrap_or_default();

    let server_service = state
        .server_service
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let instance_service = state
        .instance_service
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get server
    let server = server_service
        .get_server(server_id, user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Get instance
    let instance = instance_service
        .get_instance_detail(instance_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Get tool to extract parameters
    let tool = state
        .tool_repository
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?
        .get_by_id(instance.tool_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Extract tool parameters dynamically
    let tool_params = tool.extract_parameters();

    // Get server globals (both for display and for computing final values)
    let globals = server_service
        .get_server_globals(server_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create a map of server globals for easy lookup
    let server_globals_map: std::collections::HashMap<String, String> = globals
        .iter()
        .map(|g| (g.key.clone(), g.value.clone()))
        .collect();

    // Merge tool params with instance config
    let mut params_with_config = Vec::new();
    for param in tool_params {
        // Find matching config from instance params
        let config = instance.params.iter().find(|p| p.param_name == param.name);

        let config_source = config
            .map(|c| c.source.clone())
            .unwrap_or_else(|| "exposed".to_string());
        let config_value = config.and_then(|c| c.value.clone());

        // Get server binding if parameter name matches a server global
        let server_binding = server_globals_map.get(&param.name).cloned();

        // Compute final value based on config source
        let final_value = match config_source.as_str() {
            "server" => server_binding.clone(),
            "instance" => config_value.clone(),
            _ => None, // "exposed" has no final value until LLM provides it
        };

        params_with_config.push(ParameterWithConfig {
            name: param.name,
            param_type: param.param_type,
            source: param.source,
            config_source,
            config_value,
            server_binding,
            final_value,
        });
    }

    // Get instance signature
    let signature = instance_service
        .get_instance_signature(instance_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let template = EditInstanceTemplate {
        csrf_token,
        server,
        instance,
        params_with_config,
        signature,
        user_email,
    };

    Ok(Html(
        template
            .render()
            .unwrap_or_else(|_| "Template error".to_string()),
    ))
}

pub async fn update_instance_handler(
    State(state): State<AppState>,
    session: Session,
    Path((server_id, instance_id)): Path<(i64, i64)>,
    QsForm(form): QsForm<ConfigureInstanceForm>,
) -> Result<impl IntoResponse, StatusCode> {
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let server_service = state
        .server_service
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Verify ownership
    if !server_service
        .user_owns_server(server_id, user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let instance_service = state
        .instance_service
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Update instance name and description
    instance_service
        .update_instance(
            instance_id,
            &form.instance_name,
            form.description.as_deref(),
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Update parameters
    let params: Vec<crate::models::InstanceParam> = form
        .param_configs
        .into_iter()
        .map(|pc| crate::models::InstanceParam {
            id: Some(0), // Will be ignored on insert
            instance_id,
            param_name: pc.name,
            source: pc.source,
            value: pc.value,
        })
        .collect();

    instance_service
        .update_instance_params(instance_id, params)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    reload_server_tools(&state.pool, server_id, &state.mcp_registry).await;

    Ok(Redirect::to(&format!(
        "/servers/{}/instances/{}",
        server_id, instance_id
    )))
}

pub async fn delete_instance_handler(
    State(state): State<AppState>,
    session: Session,
    Path((server_id, instance_id)): Path<(i64, i64)>,
) -> Result<impl IntoResponse, StatusCode> {
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let server_service = state
        .server_service
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Verify ownership
    if !server_service
        .user_owns_server(server_id, user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let instance_service = state
        .instance_service
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    instance_service
        .delete_instance(instance_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    reload_server_tools(&state.pool, server_id, &state.mcp_registry).await;

    Ok(Redirect::to(&format!("/servers/{}?tab=tools", server_id)))
}

/// Helper function to reload tools for a server in the MCP registry
async fn reload_server_tools(
    pool: &sqlx::SqlitePool,
    server_id: i64,
    mcp_registry: &Option<
        std::sync::Arc<tokio::sync::RwLock<crate::mcp::registry::McpServerRegistry>>,
    >,
) {
    if let Some(ref registry) = mcp_registry {
        if let Ok(Some(server)) = Server::get_by_id(pool, server_id).await {
            if let Err(e) = registry.write().await.reload_tools(&server.uuid).await {
                tracing::error!(
                    "Failed to reload MCP tools for server {}: {}",
                    server.uuid,
                    e
                );
            }
        }
    }
}

// Display structs for test template
#[derive(serde::Serialize)]
struct ExposedParameterDisplay {
    name: String,
    param_type: String,
}

#[derive(Debug)]
struct ExecutionResultDisplay {
    status: u16,
    is_success: bool,
    body: String,
    headers: Vec<(String, String)>,
    curl_command: String,
}

impl From<ExecutionResult> for ExecutionResultDisplay {
    fn from(result: ExecutionResult) -> Self {
        let mut headers: Vec<(String, String)> = result.headers.into_iter().collect();
        headers.sort_by(|a, b| a.0.cmp(&b.0));

        Self {
            status: result.status,
            is_success: result.is_success,
            body: result.body,
            headers,
            curl_command: result.curl_command,
        }
    }
}

// Template for instance test page
#[derive(Template, WebTemplate)]
#[template(path = "instances/test.html")]
struct TestInstanceTemplate {
    user_email: String,
    server: Server,
    instance_name: String,
    instance_description: Option<String>,
    tool_name: String,
    exposed_params: Vec<ExposedParameterDisplay>,
    csrf_token: String,
    result: Option<ExecutionResultDisplay>,
    error: Option<String>,
    submitted_values: HashMap<String, String>,
}

// Form structure for test submission
#[derive(Deserialize)]
pub struct TestInstanceForm {
    #[allow(dead_code)]
    csrf_token: String,
    #[serde(flatten)]
    params: HashMap<String, String>,
}

/// GET /servers/{server_id}/instances/{instance_id}/test
pub async fn test_instance_page(
    State(state): State<AppState>,
    session: Session,
    Path((server_id, instance_id)): Path<(i64, i64)>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = get_user_id(&session).await?;
    let user_email = get_user_email(&session).await?;
    let csrf_token = get_csrf_token(&session).await?;

    // Verify server ownership
    verify_server_ownership(&state, server_id, user_id).await?;

    // Load server, instance, and tool
    let (server, instance, tool, exposed_params) =
        load_instance_test_data(&state, server_id, instance_id, user_id).await?;

    let template = TestInstanceTemplate {
        user_email,
        server,
        instance_name: instance.instance_name,
        instance_description: instance.description,
        tool_name: tool.name,
        exposed_params,
        csrf_token,
        result: None,
        error: None,
        submitted_values: HashMap::new(),
    };

    Ok(Html(
        template.render().map_err(|_| AppError::InternalError)?,
    ))
}

/// POST /servers/{server_id}/instances/{instance_id}/test
pub async fn test_instance_execute(
    State(state): State<AppState>,
    session: Session,
    Path((server_id, instance_id)): Path<(i64, i64)>,
    Form(form): Form<TestInstanceForm>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = get_user_id(&session).await?;
    let user_email = get_user_email(&session).await?;
    let csrf_token = get_csrf_token(&session).await?;

    // Verify server ownership
    verify_server_ownership(&state, server_id, user_id).await?;

    // Load server, instance, and tool
    let (server, instance, tool, exposed_params) =
        load_instance_test_data(&state, server_id, instance_id, user_id).await?;

    let submitted_values = form.params.clone();
    let tool_name = tool.name.clone();

    // Prepare exposed parameters
    let llm_params = prepare_exposed_parameters(form.params, &exposed_params);

    let template = match llm_params {
        Ok(params) => {
            // Execute instance with prepared parameters
            let result = execute_instance_test(&state, server_id, instance_id, tool, params).await;

            match result {
                Ok(exec_result) => TestInstanceTemplate {
                    user_email,
                    server,
                    instance_name: instance.instance_name,
                    instance_description: instance.description,
                    tool_name: tool_name.clone(),
                    exposed_params,
                    csrf_token,
                    result: Some(ExecutionResultDisplay::from(exec_result)),
                    error: None,
                    submitted_values,
                },
                Err(e) => TestInstanceTemplate {
                    user_email,
                    server,
                    instance_name: instance.instance_name,
                    instance_description: instance.description,
                    tool_name: tool_name.clone(),
                    exposed_params,
                    csrf_token,
                    result: None,
                    error: Some(e.to_string()),
                    submitted_values,
                },
            }
        }
        Err(e) => TestInstanceTemplate {
            user_email,
            server,
            instance_name: instance.instance_name,
            instance_description: instance.description,
            tool_name,
            exposed_params,
            csrf_token,
            result: None,
            error: Some(e.to_string()),
            submitted_values,
        },
    };

    Ok(Html(
        template.render().map_err(|_| AppError::InternalError)?,
    ))
}

// Helper functions

async fn get_user_id(session: &Session) -> Result<i64, AppError> {
    session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)
}

async fn get_user_email(session: &Session) -> Result<String, AppError> {
    session
        .get::<String>("email")
        .await
        .ok()
        .flatten()
        .ok_or(AppError::AuthenticationFailed)
}

async fn get_csrf_token(session: &Session) -> Result<String, AppError> {
    Ok(session
        .get::<String>("csrf_token")
        .await
        .ok()
        .flatten()
        .unwrap_or_default())
}

async fn verify_server_ownership(
    state: &AppState,
    server_id: i64,
    user_id: i64,
) -> Result<(), AppError> {
    let server_service = state
        .server_service
        .as_ref()
        .ok_or(AppError::InternalError)?;

    if !server_service
        .user_owns_server(server_id, user_id)
        .await
        .map_err(|_| AppError::InternalError)?
    {
        return Err(AppError::Validation(
            "Unauthorized access to server".to_string(),
        ));
    }

    Ok(())
}

async fn load_instance_test_data(
    state: &AppState,
    server_id: i64,
    instance_id: i64,
    user_id: i64,
) -> Result<
    (
        Server,
        crate::models::InstanceDetail,
        crate::models::tool::Tool,
        Vec<ExposedParameterDisplay>,
    ),
    AppError,
> {
    let server_service = state
        .server_service
        .as_ref()
        .ok_or(AppError::InternalError)?;
    let instance_service = state
        .instance_service
        .as_ref()
        .ok_or(AppError::InternalError)?;
    let tool_repository = state
        .tool_repository
        .as_ref()
        .ok_or(AppError::InternalError)?;

    // Load server
    let server = server_service
        .get_server(server_id, user_id)
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or_else(|| AppError::Validation("Server not found".to_string()))?;

    // Load instance
    let instance = instance_service
        .get_instance_detail(instance_id)
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or_else(|| AppError::Validation("Instance not found".to_string()))?;

    // Load tool
    let tool = tool_repository
        .get_by_id(instance.tool_id)
        .await?
        .ok_or_else(|| AppError::Validation("Tool not found".to_string()))?;

    // Get only exposed parameters
    let exposed_params: Vec<ExposedParameterDisplay> = instance
        .params
        .iter()
        .filter(|p| p.source == "exposed")
        .map(|p| {
            // Get parameter type from tool
            let tool_params = tool.extract_parameters();
            let param_type = tool_params
                .iter()
                .find(|tp| tp.name == p.param_name)
                .map(|tp| tp.param_type.clone())
                .unwrap_or_else(|| "string".to_string());

            ExposedParameterDisplay {
                name: p.param_name.clone(),
                param_type,
            }
        })
        .collect();

    Ok((server, instance, tool, exposed_params))
}

fn prepare_exposed_parameters(
    form_params: HashMap<String, String>,
    exposed_params: &[ExposedParameterDisplay],
) -> Result<serde_json::Map<String, serde_json::Value>, AppError> {
    let mut typed_params = serde_json::Map::new();

    for param in exposed_params {
        let param_name = &param.name;
        let param_type_str = &param.param_type;

        // Get string value from form
        let string_value = form_params.get(param_name).ok_or_else(|| {
            AppError::Validation(format!("Required parameter '{}' is missing", param_name))
        })?;

        // Convert type string to VariableType
        let var_type = match param_type_str.to_lowercase().as_str() {
            "number" => VariableType::Number,
            "integer" => VariableType::Integer,
            "boolean" | "bool" => VariableType::Boolean,
            "json" => VariableType::Json,
            "url" => VariableType::Url,
            _ => VariableType::String,
        };

        // Cast string to typed JSON value
        let typed_value = var_type.cast(string_value).map_err(|e| {
            AppError::Validation(format!(
                "Failed to cast parameter '{}' to type '{}': {}",
                param_name, param_type_str, e
            ))
        })?;

        typed_params.insert(param_name.clone(), typed_value);
    }

    Ok(typed_params)
}

async fn execute_instance_test(
    state: &AppState,
    server_id: i64,
    instance_id: i64,
    tool: crate::models::tool::Tool,
    llm_params: serde_json::Map<String, serde_json::Value>,
) -> Result<ExecutionResult, AppError> {
    // Create secrets manager
    let secrets = SecretsManager::new().map_err(|e| {
        AppError::Validation(format!("Failed to initialize secrets manager: {}", e))
    })?;

    // Create instance executor
    let executor = InstanceExecutor::new(state.pool.clone(), server_id, instance_id, tool, secrets);

    // Execute with provided parameters
    let result = executor
        .execute(Some(llm_params))
        .await
        .map_err(|e| AppError::Validation(format!("Execution failed: {}", e.message)))?;

    // Convert CallToolResult to ExecutionResult
    // Serialize the content to get a text representation
    let content_text = serde_json::to_string_pretty(&result.content)
        .unwrap_or_else(|_| format!("{:?}", result.content));

    Ok(ExecutionResult {
        status: if result.is_error.unwrap_or(false) {
            500
        } else {
            200
        },
        is_success: !result.is_error.unwrap_or(false),
        body: content_text,
        headers: HashMap::new(),
        curl_command: String::new(),
    })
}
