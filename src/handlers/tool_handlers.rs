use crate::error::AppError;
use crate::models::{CreateToolForm, ExtractedParameter, Tool, UpdateToolForm};
use crate::services::http_executor::ExecutionResult;
use crate::services::tool_test_service;
use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use serde::Deserialize;
use std::collections::HashMap;
use tower_sessions::Session;

// Helper structs to simplify template rendering
struct ToolDisplay {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub method: String,
    pub url: String,
    pub headers: String,
    pub body: String,
    pub timeout_ms: i32,
}

impl From<Tool> for ToolDisplay {
    fn from(t: Tool) -> Self {
        ToolDisplay {
            id: t.id,
            name: t.name,
            description: t.description.unwrap_or_default(),
            method: t.method,
            url: t.url.unwrap_or_default(),
            headers: t.headers.unwrap_or_else(|| "{}".to_string()),
            body: t.body.unwrap_or_else(|| "{}".to_string()),
            timeout_ms: t.timeout_ms,
        }
    }
}

struct ParameterDisplay {
    pub name: String,
    pub param_type: String,
    #[allow(dead_code)]
    pub source: String,
}

impl From<ExtractedParameter> for ParameterDisplay {
    fn from(p: ExtractedParameter) -> Self {
        ParameterDisplay {
            name: p.name,
            param_type: p.param_type,
            source: p.source,
        }
    }
}

// Form structures
#[derive(Deserialize)]
pub struct TestToolForm {
    #[allow(dead_code)]
    csrf_token: String,
    #[serde(flatten)]
    params: HashMap<String, String>,
}

// Template structures
#[derive(Template, WebTemplate)]
#[template(path = "tools/new.html")]
struct NewToolTemplate {
    user_email: String,
    toolkit_id: i64,
    toolkit_title: String,
    csrf_token: String,
    error: Option<String>,
}

#[derive(Template, WebTemplate)]
#[template(path = "tools/edit.html")]
struct EditToolTemplate {
    user_email: String,
    toolkit_id: i64,
    toolkit_title: String,
    tool: ToolDisplay,
    detected_params: Vec<crate::models::ExtractedParameter>,
    csrf_token: String,
    error: Option<String>,
}

#[derive(Template, WebTemplate)]
#[template(path = "tools/test.html")]
struct TestToolTemplate {
    user_email: String,
    toolkit_id: i64,
    toolkit_title: String,
    tool: ToolDisplay,
    parameters: Vec<ParameterDisplay>,
    csrf_token: String,
    result: Option<ExecutionResultDisplay>,
    error: Option<String>,
    submitted_values: HashMap<String, String>,
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

// Handlers

/// GET /toolkits/:toolkit_id/tools/new - Display add tool form
pub async fn create_tool_page(
    State(state): State<AppState>,
    session: Session,
    Path(toolkit_id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    // Check authentication
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)?;

    let toolkit_service = state
        .toolkit_service
        .as_ref()
        .ok_or(AppError::InternalError)?;

    // Get toolkit to verify ownership and get title
    let toolkit = toolkit_service.get_toolkit(toolkit_id, user_id).await?;

    let user_email = session
        .get::<String>("email")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let template = NewToolTemplate {
        user_email,
        toolkit_id,
        toolkit_title: toolkit.title,
        csrf_token: generate_csrf_token(),
        error: None,
    };

    Ok(Html(
        template.render().map_err(|_| AppError::InternalError)?,
    ))
}

/// POST /toolkits/:toolkit_id/tools - Create new tool
pub async fn create_tool_handler(
    State(state): State<AppState>,
    session: Session,
    Path(toolkit_id): Path<i64>,
    Form(form): Form<CreateToolForm>,
) -> Result<Response, AppError> {
    // Check authentication
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)?;

    // TODO: Validate CSRF token

    let tool_service = state.tool_service.as_ref().ok_or(AppError::InternalError)?;

    // Convert form to request
    let request = form.into_request();

    match tool_service.create_tool(toolkit_id, user_id, request).await {
        Ok(_tool_id) => Ok(Redirect::to(&format!("/toolkits/{}", toolkit_id)).into_response()),
        Err(AppError::Validation(msg)) => {
            // Get toolkit for title
            let toolkit_service = state
                .toolkit_service
                .as_ref()
                .ok_or(AppError::InternalError)?;
            let toolkit = toolkit_service.get_toolkit(toolkit_id, user_id).await?;

            let user_email = session
                .get::<String>("email")
                .await
                .ok()
                .flatten()
                .unwrap_or_default();

            let template = NewToolTemplate {
                user_email,
                toolkit_id,
                toolkit_title: toolkit.title,
                csrf_token: generate_csrf_token(),
                error: Some(msg),
            };
            Ok(Html(template.render().map_err(|_| AppError::InternalError)?).into_response())
        }
        Err(e) => Err(e),
    }
}

/// GET /toolkits/:toolkit_id/tools/:tool_id - Redirect to edit (view page removed)
pub async fn view_tool_handler(
    Path((toolkit_id, tool_id)): Path<(i64, i64)>,
) -> Result<impl IntoResponse, AppError> {
    // Redirect to edit page since we removed the view page
    Ok(Redirect::to(&format!(
        "/toolkits/{}/tools/{}/edit",
        toolkit_id, tool_id
    )))
}

/// GET /toolkits/:toolkit_id/tools/:tool_id/edit - Display edit tool form
pub async fn edit_tool_page(
    State(state): State<AppState>,
    session: Session,
    Path((toolkit_id, tool_id)): Path<(i64, i64)>,
) -> Result<impl IntoResponse, AppError> {
    // Check authentication
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)?;

    let toolkit_service = state
        .toolkit_service
        .as_ref()
        .ok_or(AppError::InternalError)?;
    let tool_service = state.tool_service.as_ref().ok_or(AppError::InternalError)?;

    // Get toolkit
    let toolkit = toolkit_service.get_toolkit(toolkit_id, user_id).await?;

    // Get tool and parameters
    let (tool, _parameters) = tool_service.get_tool(tool_id, user_id).await?;

    // Extract parameters from the tool's templates
    let detected_params = tool.extract_parameters();

    let user_email = session
        .get::<String>("email")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let template = EditToolTemplate {
        user_email,
        toolkit_id,
        toolkit_title: toolkit.title,
        tool: ToolDisplay::from(tool),
        detected_params,
        csrf_token: generate_csrf_token(),
        error: None,
    };

    Ok(Html(
        template.render().map_err(|_| AppError::InternalError)?,
    ))
}

/// POST /toolkits/:toolkit_id/tools/:tool_id - Update tool
pub async fn update_tool_handler(
    State(state): State<AppState>,
    session: Session,
    Path((toolkit_id, tool_id)): Path<(i64, i64)>,
    Form(form): Form<UpdateToolForm>,
) -> Result<Response, AppError> {
    // Check authentication
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)?;

    // TODO: Validate CSRF token

    let tool_service = state.tool_service.as_ref().ok_or(AppError::InternalError)?;

    // Convert form to request
    let request = form.into_request();

    match tool_service.update_tool(tool_id, user_id, request).await {
        Ok(()) => Ok(Redirect::to(&format!("/toolkits/{}", toolkit_id)).into_response()),
        Err(AppError::Validation(msg)) => {
            // Reload data for form
            let toolkit_service = state
                .toolkit_service
                .as_ref()
                .ok_or(AppError::InternalError)?;
            let toolkit = toolkit_service.get_toolkit(toolkit_id, user_id).await?;
            let (tool, _parameters) = tool_service.get_tool(tool_id, user_id).await?;

            // Extract parameters from the tool's templates
            let detected_params = tool.extract_parameters();

            let user_email = session
                .get::<String>("email")
                .await
                .ok()
                .flatten()
                .unwrap_or_default();

            let template = EditToolTemplate {
                user_email,
                toolkit_id,
                toolkit_title: toolkit.title,
                tool: ToolDisplay::from(tool),
                detected_params,
                csrf_token: generate_csrf_token(),
                error: Some(msg),
            };
            Ok(Html(template.render().map_err(|_| AppError::InternalError)?).into_response())
        }
        Err(e) => Err(e),
    }
}

/// POST /toolkits/:toolkit_id/tools/:tool_id/delete - Delete tool
pub async fn delete_tool_handler(
    State(state): State<AppState>,
    session: Session,
    Path((toolkit_id, tool_id)): Path<(i64, i64)>,
) -> Result<impl IntoResponse, AppError> {
    // Check authentication
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)?;

    let tool_service = state.tool_service.as_ref().ok_or(AppError::InternalError)?;

    // Delete tool (parameters will cascade)
    tool_service.delete_tool(tool_id, user_id).await?;

    Ok(Redirect::to(&format!("/toolkits/{}", toolkit_id)))
}

/// GET /toolkits/:toolkit_id/tools/:tool_id/test - Display tool test page
pub async fn test_tool_page(
    State(state): State<AppState>,
    session: Session,
    Path((toolkit_id, tool_id)): Path<(i64, i64)>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)?;

    let toolkit_service = state
        .toolkit_service
        .as_ref()
        .ok_or(AppError::InternalError)?;
    let tool_service = state.tool_service.as_ref().ok_or(AppError::InternalError)?;

    let toolkit = toolkit_service.get_toolkit(toolkit_id, user_id).await?;
    let (tool, _) = tool_service.get_tool(tool_id, user_id).await?;
    let parameters = tool.extract_parameters();

    let user_email = session
        .get::<String>("email")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let template = TestToolTemplate {
        user_email,
        toolkit_id,
        toolkit_title: toolkit.title,
        tool: ToolDisplay::from(tool),
        parameters: parameters.into_iter().map(ParameterDisplay::from).collect(),
        csrf_token: generate_csrf_token(),
        result: None,
        error: None,
        submitted_values: HashMap::new(),
    };

    Ok(Html(
        template.render().map_err(|_| AppError::InternalError)?,
    ))
}

/// POST /toolkits/:toolkit_id/tools/:tool_id/test - Execute tool test
pub async fn test_tool_execute(
    State(state): State<AppState>,
    session: Session,
    Path((toolkit_id, tool_id)): Path<(i64, i64)>,
    Form(form): Form<TestToolForm>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)?;

    let toolkit_service = state
        .toolkit_service
        .as_ref()
        .ok_or(AppError::InternalError)?;
    let tool_service = state.tool_service.as_ref().ok_or(AppError::InternalError)?;

    let toolkit = toolkit_service.get_toolkit(toolkit_id, user_id).await?;
    let (tool, _) = tool_service.get_tool(tool_id, user_id).await?;
    let parameters = tool.extract_parameters();

    let user_email = session
        .get::<String>("email")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let submitted_values = form.params.clone();
    let test_result =
        tool_test_service::test_tool(&state.pool, tool_id, user_id, form.params).await;

    let template = match test_result {
        Ok(result) => TestToolTemplate {
            user_email,
            toolkit_id,
            toolkit_title: toolkit.title,
            tool: ToolDisplay::from(tool),
            parameters: parameters.into_iter().map(ParameterDisplay::from).collect(),
            csrf_token: generate_csrf_token(),
            result: Some(ExecutionResultDisplay::from(result)),
            error: None,
            submitted_values: submitted_values.clone(),
        },
        Err(e) => TestToolTemplate {
            user_email,
            toolkit_id,
            toolkit_title: toolkit.title,
            tool: ToolDisplay::from(tool),
            parameters: parameters.into_iter().map(ParameterDisplay::from).collect(),
            csrf_token: generate_csrf_token(),
            result: None,
            error: Some(e.to_string()),
            submitted_values: submitted_values.clone(),
        },
    };

    Ok(Html(
        template.render().map_err(|_| AppError::InternalError)?,
    ))
}

// Utility function to generate CSRF tokens
fn generate_csrf_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let random_bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    hex::encode(random_bytes)
}
