pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod handlers;
pub mod mcp;
pub mod middleware;
pub mod models;
pub mod repositories;
pub mod services;
pub mod variables;

// Make test_utils available for both unit tests and integration tests
pub mod test_utils;

use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub user_service: Arc<services::user_service::UserService>,
    pub auth_service: Arc<services::auth_service::AuthService>,
    pub auth_token_service: Arc<services::auth_token_service::AuthTokenService>,
    pub toolkit_service: Option<Arc<services::toolkit_service::ToolkitService>>,
    pub tool_service: Option<Arc<services::tool_service::ToolService>>,
    pub server_service: Option<Arc<services::server_service::ServerService>>,
    pub instance_service: Option<Arc<services::instance_service::InstanceService>>,
    pub oauth_service: Arc<services::oauth_service::OAuthService>,
    pub toolkit_repository: Option<Arc<dyn repositories::ToolkitRepository>>,
    pub tool_repository: Option<Arc<dyn repositories::ToolRepository>>,
    pub mcp_registry: Option<Arc<RwLock<crate::mcp::registry::McpServerRegistry>>>,
    pub pool: sqlx::SqlitePool,
}
