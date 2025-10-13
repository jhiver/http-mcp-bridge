//! MCP server instance management
//!
//! Each [`McpServerInstance`] wraps an SSE server and MCP service for one
//! configured server.

use crate::mcp::registry::RegistryError;
use crate::mcp::service::SaraMcpService;
use axum::Router as AxumRouter;
use rmcp::handler::server::router::Router as McpRouter;
use rmcp::transport::sse_server::{SseServer, SseServerConfig};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Individual MCP server instance
///
/// Wraps SSE server and MCP service with lifecycle management.
/// Each instance corresponds to one row in the `servers` table.
///
/// # Fields
///
/// * `server_id` - Database ID of the server
/// * `server_uuid` - Unique identifier used for routing
/// * `subdomain_sse_router` - Subdomain SSE router at `/` (doxyde pattern)
/// * `service` - MCP protocol handler
/// * `ct` - Cancellation token for graceful shutdown
pub struct McpServerInstance {
    pub server_id: i64,
    pub server_uuid: String,
    pub subdomain_sse_router: AxumRouter,
    service: Arc<SaraMcpService>,
    ct: CancellationToken,
}

impl McpServerInstance {
    /// Creates a new MCP server instance
    ///
    /// # Arguments
    ///
    /// * `pool` - Database connection pool
    /// * `server_id` - Database ID of the server
    /// * `uuid` - Server UUID for routing
    ///
    /// # Returns
    ///
    /// * `Ok(Self)` - Instance created successfully
    /// * `Err(RegistryError)` - Failed to create instance
    pub async fn new(
        pool: SqlitePool,
        server_id: i64,
        uuid: String,
    ) -> Result<Self, RegistryError> {
        let ct = CancellationToken::new();

        let service = Arc::new(SaraMcpService::new(server_id, pool.clone()).await.map_err(
            |e| RegistryError::InstanceCreation(format!("Failed to create MCP service: {}", e)),
        )?);

        // Get tool routes from the service
        let tool_routes = service.get_tool_routes().await.map_err(|e| {
            RegistryError::InstanceCreation(format!("Failed to get tool routes: {}", e))
        })?;

        let bind_addr = "127.0.0.1:0"
            .parse()
            .map_err(|e| RegistryError::InstanceCreation(format!("Invalid bind addr: {}", e)))?;

        // Create subdomain SSE server at / and /message (doxyde pattern)
        let subdomain_config = SseServerConfig {
            bind: bind_addr,
            sse_path: "/".to_string(),
            post_path: "/message".to_string(),
            ct: ct.clone(),
            sse_keep_alive: Some(std::time::Duration::from_secs(30)),
        };

        let (subdomain_sse_server, subdomain_sse_router) = SseServer::new(subdomain_config);
        let service_clone = service.clone();
        subdomain_sse_server.with_service(move || {
            // Create a Router with the service and tool routes
            let router = McpRouter::new((*service_clone).clone());
            router.with_tools(tool_routes.clone())
        });

        Ok(Self {
            server_id,
            server_uuid: uuid,
            subdomain_sse_router,
            service,
            ct,
        })
    }

    /// Reloads tool definitions from the database
    ///
    /// Rebuilds the tool router from the current database state without
    /// requiring a full server restart. This allows dynamic updates to
    /// tool instances.
    pub async fn reload_tools(&self) -> Result<(), RegistryError> {
        self.service
            .reload_tools()
            .await
            .map_err(|e| RegistryError::InstanceCreation(e.to_string()))?;
        Ok(())
    }

    /// Initiates graceful shutdown of the instance
    ///
    /// Signals the cancellation token to stop the SSE server.
    pub async fn shutdown(&self) -> Result<(), RegistryError> {
        self.ct.cancel();
        Ok(())
    }

    /// Returns a clone of the MCP service
    pub fn get_service(&self) -> Arc<SaraMcpService> {
        Arc::clone(&self.service)
    }
}
