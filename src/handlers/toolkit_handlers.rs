use crate::error::AppError;
use crate::models::{CreateToolkitForm, Toolkit, UpdateToolkitForm};
use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use tower_sessions::Session;

// Helper struct to simplify template rendering
struct ToolkitDisplay {
    pub id: i64,
    pub title: String,
    pub description: String, // Convert Option to String for templates
    pub visibility: String,
}

impl From<Toolkit> for ToolkitDisplay {
    fn from(t: Toolkit) -> Self {
        ToolkitDisplay {
            id: t.id,
            title: t.title,
            description: t.description.unwrap_or_default(),
            visibility: t.visibility,
        }
    }
}

// Template structures
#[derive(Template, WebTemplate)]
#[template(path = "toolkits/new.html")]
struct NewToolkitTemplate {
    user_email: String,
    csrf_token: String,
    error: Option<String>,
    form: CreateToolkitForm,
}

#[derive(Template, WebTemplate)]
#[template(path = "toolkits/view.html")]
struct ViewToolkitTemplate {
    user_email: String,
    toolkit: ToolkitDisplay,
    tools: Vec<ToolWithParameterCount>,
}

#[derive(Template, WebTemplate)]
#[template(path = "toolkits/edit.html")]
struct EditToolkitTemplate {
    user_email: String,
    toolkit: ToolkitDisplay,
    csrf_token: String,
    error: Option<String>,
}

// Helper struct for tool display
pub struct ToolWithParameterCount {
    pub id: i64,
    pub name: String,
    pub description: String, // Convert Option to String for templates
    pub method: String,
    pub url: String, // Convert Option to String for templates
    pub parameters_count: i32,
}

// Handlers

/// GET /toolkits/new - Display create toolkit form
pub async fn create_toolkit_page(session: Session) -> Result<impl IntoResponse, AppError> {
    // Check authentication
    let _user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)?;

    let user_email = session
        .get::<String>("email")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let template = NewToolkitTemplate {
        user_email,
        csrf_token: generate_csrf_token(),
        error: None,
        form: CreateToolkitForm {
            title: String::new(),
            description: String::new(),
            visibility: "private".to_string(),
            csrf_token: String::new(),
        },
    };

    Ok(Html(
        template.render().map_err(|_| AppError::InternalError)?,
    ))
}

/// POST /toolkits - Create new toolkit
pub async fn create_toolkit_handler(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<CreateToolkitForm>,
) -> Result<Response, AppError> {
    // Check authentication
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)?;

    // TODO: Validate CSRF token

    // Create toolkit through service
    let toolkit_service = state
        .toolkit_service
        .as_ref()
        .ok_or(AppError::InternalError)?;

    match toolkit_service
        .create_toolkit(user_id, form.clone().into())
        .await
    {
        Ok(id) => Ok(Redirect::to(&format!("/toolkits/{}", id)).into_response()),
        Err(AppError::Validation(msg)) => {
            let user_email = session
                .get::<String>("email")
                .await
                .ok()
                .flatten()
                .unwrap_or_default();

            let template = NewToolkitTemplate {
                user_email,
                csrf_token: generate_csrf_token(),
                error: Some(msg),
                form,
            };
            Ok(Html(template.render().map_err(|_| AppError::InternalError)?).into_response())
        }
        Err(e) => Err(e),
    }
}

/// GET /toolkits/:id - View toolkit
pub async fn view_toolkit_handler(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
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
    let toolkit = toolkit_service.get_toolkit(id, user_id).await?;

    // Get tools for this toolkit
    let tools = tool_service.list_tools(id, user_id).await?;

    // Convert tools to display format with parameter counts
    let mut tools_with_counts = Vec::new();
    for tool in tools {
        // Extract parameters dynamically
        let params = tool.extract_parameters();

        tools_with_counts.push(ToolWithParameterCount {
            id: tool.id,
            name: tool.name,
            description: tool.description.unwrap_or_default(),
            method: tool.method,
            url: tool.url.unwrap_or_default(),
            parameters_count: params.len() as i32,
        });
    }

    let user_email = session
        .get::<String>("email")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let template = ViewToolkitTemplate {
        user_email,
        toolkit: ToolkitDisplay::from(toolkit),
        tools: tools_with_counts,
    };

    Ok(Html(
        template.render().map_err(|_| AppError::InternalError)?,
    ))
}

/// GET /toolkits/:id/edit - Display edit toolkit form
pub async fn edit_toolkit_page(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
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

    // Get toolkit
    let toolkit = toolkit_service.get_toolkit(id, user_id).await?;

    let user_email = session
        .get::<String>("email")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let template = EditToolkitTemplate {
        user_email,
        toolkit: ToolkitDisplay::from(toolkit),
        csrf_token: generate_csrf_token(),
        error: None,
    };

    Ok(Html(
        template.render().map_err(|_| AppError::InternalError)?,
    ))
}

/// POST /toolkits/:id - Update toolkit
pub async fn update_toolkit_handler(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<UpdateToolkitForm>,
) -> Result<Response, AppError> {
    // Check authentication
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)?;

    // TODO: Validate CSRF token

    let toolkit_service = state
        .toolkit_service
        .as_ref()
        .ok_or(AppError::InternalError)?;

    match toolkit_service
        .update_toolkit(id, user_id, form.clone().into())
        .await
    {
        Ok(()) => Ok(Redirect::to(&format!("/toolkits/{}", id)).into_response()),
        Err(AppError::Validation(msg)) => {
            // Reload toolkit for form
            let toolkit = toolkit_service.get_toolkit(id, user_id).await?;

            let user_email = session
                .get::<String>("email")
                .await
                .ok()
                .flatten()
                .unwrap_or_default();

            let template = EditToolkitTemplate {
                user_email,
                toolkit: ToolkitDisplay::from(toolkit),
                csrf_token: generate_csrf_token(),
                error: Some(msg),
            };
            Ok(Html(template.render().map_err(|_| AppError::InternalError)?).into_response())
        }
        Err(e) => Err(e),
    }
}

/// POST /toolkits/:id/delete - Delete toolkit
pub async fn delete_toolkit_handler(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
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

    // Delete toolkit (tools and parameters will cascade)
    toolkit_service.delete_toolkit(id, user_id).await?;

    Ok(Redirect::to("/toolkits"))
}

// Public toolkit browsing and cloning handlers

#[derive(Template, WebTemplate)]
#[template(path = "toolkits/explore.html")]
struct ExploreToolkitsTemplate {
    user_email: String,
    toolkits: Vec<ToolkitWithStatsDisplay>,
    csrf_token: String,
    current_sort: String,
}

// Helper struct for displaying toolkit stats in templates
struct ToolkitWithStatsDisplay {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    pub clone_count: i32,
    pub tools_count: i32,
    pub owner_email: String,
    pub parent_toolkit_id: Option<i64>,
}

impl From<crate::models::ToolkitWithStats> for ToolkitWithStatsDisplay {
    fn from(t: crate::models::ToolkitWithStats) -> Self {
        ToolkitWithStatsDisplay {
            id: t.id,
            title: t.title,
            description: t.description,
            clone_count: t.clone_count,
            tools_count: t.tools_count,
            owner_email: t.owner_email,
            parent_toolkit_id: t.parent_toolkit_id,
        }
    }
}

/// GET /toolkits/explore - Browse all public toolkits
pub async fn explore_toolkits_handler(
    State(state): State<AppState>,
    session: Session,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    // Check authentication
    let _user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)?;

    let user_email = session
        .get::<String>("email")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let toolkit_service = state
        .toolkit_service
        .as_ref()
        .ok_or(AppError::InternalError)?;

    // Get all public toolkits with stats
    let mut public_toolkits = toolkit_service.list_public_toolkits().await?;

    // Get sort parameter (default to "popular")
    let sort = params.get("sort").map(|s| s.as_str()).unwrap_or("popular");

    // Sort the toolkits based on the parameter
    match sort {
        "newest" => {
            public_toolkits.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        }
        "oldest" => {
            public_toolkits.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        }
        "most_tools" => {
            public_toolkits.sort_by(|a, b| b.tools_count.cmp(&a.tools_count));
        }
        _ => {
            // Default to "popular" (most cloned)
            public_toolkits.sort_by(|a, b| b.clone_count.cmp(&a.clone_count));
        }
    }

    // Convert to display format
    let toolkits_display: Vec<ToolkitWithStatsDisplay> = public_toolkits
        .into_iter()
        .map(ToolkitWithStatsDisplay::from)
        .collect();

    let template = ExploreToolkitsTemplate {
        user_email,
        toolkits: toolkits_display,
        csrf_token: generate_csrf_token(),
        current_sort: sort.to_string(),
    };

    Ok(Html(
        template.render().map_err(|_| AppError::InternalError)?,
    ))
}

#[derive(Template, WebTemplate)]
#[template(path = "toolkits/public_view.html")]
struct PublicToolkitTemplate {
    user_email: String,
    details: PublicToolkitDetails,
    csrf_token: String,
}

struct PublicToolkitDetails {
    toolkit: crate::models::Toolkit,
    tools: Vec<crate::models::tool::Tool>,
    owner_email: String,
    parent_toolkit: Option<crate::models::ToolkitSummary>,
}

/// GET /toolkits/:id/public - View a public toolkit (non-owner view)
pub async fn view_public_toolkit_handler(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    // Check authentication
    let _user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)?;

    let user_email = session
        .get::<String>("email")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let toolkit_service = state
        .toolkit_service
        .as_ref()
        .ok_or(AppError::InternalError)?;

    // Get public toolkit details
    let service_details = toolkit_service.get_public_toolkit_details(id).await?;

    // Convert to our local struct
    let details = PublicToolkitDetails {
        toolkit: service_details.toolkit,
        tools: service_details.tools,
        owner_email: service_details.owner_email,
        parent_toolkit: service_details.parent_toolkit,
    };

    let template = PublicToolkitTemplate {
        user_email,
        details,
        csrf_token: generate_csrf_token(),
    };

    Ok(Html(
        template.render().map_err(|_| AppError::InternalError)?,
    ))
}

/// POST /toolkits/:id/clone - Clone a public toolkit
pub async fn clone_toolkit_handler(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<i64>,
    Form(form): Form<crate::models::CloneToolkitRequest>,
) -> Result<Response, AppError> {
    // Check authentication
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|_| AppError::InternalError)?
        .ok_or(AppError::AuthenticationFailed)?;

    // TODO: Validate CSRF token

    let toolkit_service = state
        .toolkit_service
        .as_ref()
        .ok_or(AppError::InternalError)?;

    // Clone the toolkit
    let new_toolkit_id = toolkit_service
        .clone_toolkit(id, user_id, form.new_title)
        .await?;

    // Redirect to the newly cloned toolkit
    Ok(Redirect::to(&format!("/toolkits/{}", new_toolkit_id)).into_response())
}

// Utility function to generate CSRF tokens
fn generate_csrf_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let random_bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    hex::encode(random_bytes)
}
