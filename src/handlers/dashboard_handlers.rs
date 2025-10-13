use crate::models::ToolUsageStats;
use crate::services::{DashboardService, DashboardStats, RecentExecution, ServerStats};
use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::{extract::State, http::StatusCode, response::IntoResponse};
use tower_sessions::Session;

#[derive(Template, WebTemplate)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    user_email: String,
    stats: DashboardStats,
    server_stats: Vec<ServerStats>,
    most_used_tools: Vec<ToolUsageStats>,
    recent_executions: Vec<RecentExecution>,
}

/// GET /dashboard - Show user dashboard
pub async fn dashboard_handler(
    State(state): State<AppState>,
    session: Session,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Get user from session
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::UNAUTHORIZED, "Not authenticated".to_string()))?;

    let user_email = session
        .get::<String>("email")
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .unwrap_or_else(|| "Unknown".to_string());

    // Create dashboard service
    let dashboard_service = DashboardService::new(state.pool.clone());

    // Fetch all dashboard data
    let stats = dashboard_service
        .get_dashboard_stats(user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let server_stats = dashboard_service
        .get_server_stats(user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let most_used_tools = dashboard_service
        .get_most_used_tools(user_id, 10)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let recent_executions = dashboard_service
        .get_recent_executions(user_id, 20)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Render template
    let template = DashboardTemplate {
        user_email,
        stats,
        server_stats,
        most_used_tools,
        recent_executions,
    };

    Ok(template.into_response())
}
