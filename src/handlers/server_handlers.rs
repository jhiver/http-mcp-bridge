use crate::models::{CreateServerForm, GlobalsForm, ServerGlobal, UpdateServerForm};
use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    Form,
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

#[derive(Debug, Deserialize)]
pub struct UpdateServerAccessForm {
    pub access_level: String,
    pub csrf_token: String,
}

// Templates
#[derive(Template, WebTemplate)]
#[template(path = "servers/list.html")]
struct ListServersTemplate {
    servers: Vec<crate::models::ServerSummary>,
    user_email: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "servers/new.html")]
struct NewServerTemplate {
    csrf_token: String,
    user_email: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "servers/view.html")]
struct ViewServerTemplate {
    csrf_token: String,
    server: crate::models::Server,
    installed_toolkits: Vec<crate::models::ServerToolkit>,
    available_toolkits: Vec<crate::models::Toolkit>,
    instances: Vec<crate::models::InstanceDetail>,
    available_tools: Vec<crate::services::ToolWithParams>,
    active_tab: String,
    user_email: String,
    bindings: Vec<BindingRow>,
}

#[derive(Deserialize)]
pub struct ServerViewQuery {
    #[serde(default = "default_tab")]
    tab: String,
}

fn default_tab() -> String {
    "server".to_string()
}

#[derive(Serialize)]
pub struct BindingRow {
    pub param_name: String,
    pub param_type: String,
    pub usage_count: usize,
    pub current_value: Option<String>,
    pub is_secret: bool,
}

// Helper function to build binding rows from discovered params and globals
fn build_binding_rows(
    discovered: Vec<crate::services::ParameterUsageCount>,
    globals: Vec<ServerGlobal>,
) -> Vec<BindingRow> {
    let mut bindings = Vec::new();

    for param in discovered {
        let matching_global = globals.iter().find(|g| g.key == param.param_name);

        let (current_value, is_secret) = if let Some(global) = matching_global {
            (
                Some(global.value.clone()),
                global.is_secret.unwrap_or(false),
            )
        } else {
            (None, false)
        };

        bindings.push(BindingRow {
            param_name: param.param_name,
            param_type: param.param_type,
            usage_count: param.usage_count,
            current_value,
            is_secret,
        });
    }

    bindings
}

#[derive(Template, WebTemplate)]
#[template(path = "servers/edit.html")]
struct EditServerTemplate {
    csrf_token: String,
    server: crate::models::Server,
    user_email: String,
}

// Handlers
pub async fn list_servers_page(
    State(state): State<AppState>,
    session: Session,
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

    let server_service = state
        .server_service
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let servers = server_service
        .list_servers(user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let template = ListServersTemplate {
        servers,
        user_email,
    };

    Ok(Html(
        template
            .render()
            .unwrap_or_else(|_| "Template error".to_string()),
    ))
}

pub async fn create_server_page(
    State(_state): State<AppState>,
    session: Session,
) -> Result<Html<String>, StatusCode> {
    let _user_id = session
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

    let template = NewServerTemplate {
        csrf_token,
        user_email,
    };

    Ok(Html(
        template
            .render()
            .unwrap_or_else(|_| "Template error".to_string()),
    ))
}

pub async fn create_server_handler(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<CreateServerForm>,
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

    let server_id = server_service
        .create_server(user_id, form)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get the server to access its UUID
    let server = server_service
        .get_server(server_id, user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Register with MCP registry
    if let Some(ref mcp_registry) = state.mcp_registry {
        if let Err(e) = mcp_registry
            .write()
            .await
            .register_server(&server.uuid)
            .await
        {
            tracing::error!("Failed to register MCP server {}: {}", server.uuid, e);
        }
    }

    Ok(Redirect::to(&format!("/servers/{}", server_id)))
}

pub async fn view_server_handler(
    State(state): State<AppState>,
    session: Session,
    Path(server_id): Path<i64>,
    Query(query): Query<ServerViewQuery>,
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

    // Get installed toolkits
    let installed_toolkits = server_service
        .get_server_toolkits(server_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get all user's toolkits
    let all_toolkits = if let Some(toolkit_service) = &state.toolkit_service {
        toolkit_service
            .list_toolkits(user_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        vec![]
    };

    // Filter to get available (not installed) toolkits
    let installed_ids: std::collections::HashSet<i64> = installed_toolkits
        .iter()
        .filter_map(|t| t.toolkit_id)
        .collect();

    let available_toolkits: Vec<_> = all_toolkits
        .into_iter()
        .filter(|t| !installed_ids.contains(&t.id))
        .collect();

    // Get instances
    let instances = instance_service
        .list_instances_by_server(server_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get available tools
    let available_tools = instance_service
        .get_available_tools(server_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Discover parameters with usage
    let discovered = instance_service
        .discover_parameters_with_usage(server_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Fetch existing server globals (decrypted)
    let globals = server_service
        .get_server_globals_decrypted(server_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Match discovered params with globals
    let bindings = build_binding_rows(discovered, globals);

    // Validate tab parameter
    let active_tab = match query.tab.as_str() {
        "server" | "toolkits" | "tools" | "bindings" | "metadata" | "settings" => query.tab,
        _ => "server".to_string(),
    };

    let template = ViewServerTemplate {
        csrf_token,
        server,
        installed_toolkits,
        available_toolkits,
        instances,
        available_tools,
        active_tab,
        user_email,
        bindings,
    };

    Ok(Html(
        template
            .render()
            .unwrap_or_else(|_| "Template error".to_string()),
    ))
}

pub async fn edit_server_page(
    State(state): State<AppState>,
    session: Session,
    Path(server_id): Path<i64>,
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

    let server = server_service
        .get_server(server_id, user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let template = EditServerTemplate {
        csrf_token,
        server,
        user_email,
    };

    Ok(Html(
        template
            .render()
            .unwrap_or_else(|_| "Template error".to_string()),
    ))
}

pub async fn update_server_handler(
    State(state): State<AppState>,
    session: Session,
    Path(server_id): Path<i64>,
    Form(form): Form<UpdateServerForm>,
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

    server_service
        .update_server(server_id, user_id, form)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Redirect::to(&format!(
        "/servers/{}?tab=metadata",
        server_id
    )))
}

pub async fn update_server_access_handler(
    State(state): State<AppState>,
    session: Session,
    Path(server_id): Path<i64>,
    Form(form): Form<UpdateServerAccessForm>,
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

    server_service
        .update_server_access(server_id, user_id, &form.access_level)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Redirect::to(&format!(
        "/servers/{}?tab=settings",
        server_id
    )))
}

pub async fn delete_server_handler(
    State(state): State<AppState>,
    session: Session,
    Path(server_id): Path<i64>,
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

    // Get the server UUID before deletion
    let server = server_service
        .get_server(server_id, user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Unregister from MCP registry
    if let Some(ref mcp_registry) = state.mcp_registry {
        if let Err(e) = mcp_registry
            .write()
            .await
            .unregister_server(&server.uuid)
            .await
        {
            tracing::error!("Failed to unregister MCP server {}: {}", server.uuid, e);
        }
    }

    server_service
        .delete_server(server_id, user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Redirect::to("/toolkits"))
}

#[derive(serde::Deserialize)]
pub struct InstallToolkitForm {
    pub toolkit_id: i64,
    pub csrf_token: String,
}

pub async fn install_toolkit_handler(
    State(state): State<AppState>,
    session: Session,
    Path(server_id): Path<i64>,
    Form(form): Form<InstallToolkitForm>,
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

    server_service
        .add_toolkit_to_server(server_id, form.toolkit_id, user_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Redirect::to(&format!(
        "/servers/{}?tab=toolkits",
        server_id
    )))
}

// Helper to deserialize either a single value or a sequence for form fields
fn string_or_seq<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Deserialize};

    struct StringOrVec;

    impl<'de> de::Visitor<'de> for StringOrVec {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("string or sequence of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(vec![value.to_owned()])
        }

        fn visit_seq<S>(self, visitor: S) -> Result<Self::Value, S::Error>
        where
            S: de::SeqAccess<'de>,
        {
            Deserialize::deserialize(de::value::SeqAccessDeserializer::new(visitor))
        }
    }

    deserializer.deserialize_any(StringOrVec)
}

#[derive(serde::Deserialize)]
pub struct BindingsForm {
    #[serde(default, deserialize_with = "string_or_seq")]
    pub keys: Vec<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub values: Vec<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub is_secret: Vec<String>, // Only contains keys that are marked as secret
    pub csrf_token: String,
}

pub async fn save_bindings_handler(
    State(state): State<AppState>,
    session: Session,
    Path(server_id): Path<i64>,
    body: String,
) -> Result<impl IntoResponse, StatusCode> {
    // Manually parse form data to handle duplicate field names
    use form_urlencoded::parse;

    let mut keys = Vec::new();
    let mut values = Vec::new();
    let mut is_secret = Vec::new();
    let mut csrf_token = String::new();

    for (key, value) in parse(body.as_bytes()) {
        match key.as_ref() {
            "keys" => keys.push(value.into_owned()),
            "values" => values.push(value.into_owned()),
            "is_secret" => is_secret.push(value.into_owned()),
            "csrf_token" => csrf_token = value.into_owned(),
            _ => {}
        }
    }

    let form = BindingsForm {
        keys,
        values,
        is_secret,
        csrf_token,
    };
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let server_service = state
        .server_service
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Debug logging
    eprintln!("🔍 Form data received:");
    eprintln!("  keys: {:?}", form.keys);
    eprintln!("  values: {:?}", form.values);
    eprintln!("  is_secret: {:?}", form.is_secret);

    // Convert single list with is_secret flags to separate var/secret lists
    let secret_set: std::collections::HashSet<String> = form.is_secret.iter().cloned().collect();

    let mut var_keys = Vec::new();
    let mut var_values = Vec::new();
    let mut secret_keys = Vec::new();
    let mut secret_values = Vec::new();

    for (key, value) in form.keys.iter().zip(form.values.iter()) {
        if secret_set.contains(key) {
            secret_keys.push(key.clone());
            secret_values.push(value.clone());
        } else {
            var_keys.push(key.clone());
            var_values.push(value.clone());
        }
    }

    let globals_form = GlobalsForm {
        var_keys,
        var_values,
        secret_keys,
        secret_values,
        csrf_token: form.csrf_token,
    };

    server_service
        .save_server_globals(server_id, user_id, globals_form)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Redirect::to(&format!(
        "/servers/{}?tab=bindings",
        server_id
    )))
}

// ============================================================================
// Dynamic Subdomain MCP Routing Handlers
// ============================================================================

/// GET / - Dynamic subdomain SSE handler
///
/// Handles SSE connection requests at the root path for subdomain-based routing
/// (e.g., `https://{uuid}.saramcp.com/`).
///
/// # Dynamic Behavior
///
/// This handler looks up the MCP server instance in the registry **at request time**,
/// which means servers created after application startup are immediately accessible
/// without requiring a restart.
///
/// # Arguments
///
/// * `headers` - HTTP headers containing server UUID (X-Server-UUID or Host)
/// * `registry` - Shared MCP server registry
/// * `request` - Full HTTP request to be routed to the SSE handler
///
/// # Returns
///
/// * `Ok(Response)` - SSE connection established
/// * `Err(StatusCode::BAD_REQUEST)` - No UUID found in headers
/// * `Err(StatusCode::NOT_FOUND)` - Server UUID not registered
/// * `Err(StatusCode::INTERNAL_SERVER_ERROR)` - Routing failed
pub async fn root_sse_handler(
    headers: axum::http::HeaderMap,
    State(registry): State<crate::mcp::registry::SharedRegistry>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use tower::ServiceExt; // For .oneshot()

    // Extract UUID from headers (already validated by middleware)
    let uuid = crate::middleware::extract_server_uuid_from_headers(&headers)
        .ok_or(axum::http::StatusCode::BAD_REQUEST)?;

    tracing::debug!("Root SSE handler: looking up server UUID: {}", uuid);

    // Look up instance dynamically (works for newly created servers!)
    let instance = registry
        .read()
        .await
        .get_instance(&uuid)
        .ok_or(axum::http::StatusCode::NOT_FOUND)?;

    tracing::info!("Root SSE connection established for server: {}", uuid);

    // Route through the subdomain SSE router using tower::ServiceExt::oneshot
    // The subdomain_sse_router is pre-configured with paths "/" and "/message"
    instance
        .subdomain_sse_router
        .clone()
        .oneshot(request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to route SSE request: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })
}

/// POST /message - Dynamic subdomain message handler
///
/// Handles JSON-RPC message requests at `/message` for subdomain-based routing
/// (e.g., `https://{uuid}.saramcp.com/message`).
///
/// This follows the doxyde pattern where:
/// - GET / → SSE connection (long-lived)
/// - POST /message → JSON-RPC messages (one-shot)
///
/// # Dynamic Behavior
///
/// Like `root_sse_handler`, this handler performs registry lookup at request time,
/// enabling dynamic server creation/deletion without restarts.
///
/// # Arguments
///
/// * `headers` - HTTP headers containing server UUID
/// * `registry` - Shared MCP server registry
/// * `request` - JSON-RPC request payload
///
/// # Returns
///
/// * `Ok(Response)` - JSON-RPC response
/// * `Err(StatusCode::BAD_REQUEST)` - No UUID found or invalid JSON
/// * `Err(StatusCode::NOT_FOUND)` - Server UUID not registered
/// * `Err(StatusCode::INTERNAL_SERVER_ERROR)` - Request processing failed
pub async fn root_message_handler(
    headers: axum::http::HeaderMap,
    State(registry): State<crate::mcp::registry::SharedRegistry>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use tower::ServiceExt; // For .oneshot()

    // Extract UUID from headers (already validated by middleware)
    let uuid = crate::middleware::extract_server_uuid_from_headers(&headers)
        .ok_or(axum::http::StatusCode::BAD_REQUEST)?;

    tracing::debug!("Root message handler: looking up server UUID: {}", uuid);

    // Look up instance dynamically (works for newly created servers!)
    let instance = registry
        .read()
        .await
        .get_instance(&uuid)
        .ok_or(axum::http::StatusCode::NOT_FOUND)?;

    tracing::debug!("Processing SSE message request for server: {}", uuid);

    // Route through the subdomain SSE router using tower::ServiceExt::oneshot
    // The subdomain_sse_router is pre-configured with paths "/" and "/message"
    instance
        .subdomain_sse_router
        .clone()
        .oneshot(request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to route message request: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })
}
