//! MCP ServerHandler implementation for SaraMCP
//!
//! This module provides the main MCP protocol implementation for SaraMCP. It loads
//! tool instances from the database and exposes them as MCP tools through the rmcp
//! framework's ServerHandler trait.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │  SaraMcpService                     │
//! │  - Loads instances from database    │
//! │  - Builds ToolRouter                │
//! │  - Implements ServerHandler         │
//! └──────────────┬──────────────────────┘
//!                │
//!                ├─> SchemaGenerator (per instance)
//!                │   └─> JSON Schema for exposed params
//!                │
//!                ├─> InstanceExecutor (per instance)
//!                │   └─> HTTP execution handler
//!                │
//!                └─> ToolRouter
//!                    └─> Dynamic tool registration
//! ```
//!
//! # Lifecycle
//!
//! 1. **Initialization**: `SaraMcpService::new(server_id, pool)`
//!    - Loads all tool instances for the server
//!    - Generates JSON Schema for each instance
//!    - Creates InstanceExecutor for each instance
//!    - Registers tools in ToolRouter
//!
//! 2. **Runtime**: MCP protocol calls
//!    - `initialize`: Returns server info and capabilities
//!    - `tools/list`: Returns all registered tools
//!    - `tools/call`: Routes to appropriate InstanceExecutor
//!
//! 3. **Reload**: `reload_tools()` (when instances change)
//!    - Rebuilds ToolRouter with updated instances
//!
//! # Integration
//!
//! Used by MCP server runtime to handle protocol operations. Each SaraMCP server
//! (identified by server_id) gets its own SaraMcpService instance.
//!
//! # Usage
//!
//! ```rust,no_run
//! use saramcp::mcp::service::SaraMcpService;
//! use sqlx::SqlitePool;
//!
//! async fn start_mcp_server(pool: SqlitePool, server_id: i64) {
//!     // Create service for the server
//!     let service = SaraMcpService::new(server_id, pool)
//!         .await
//!         .expect("Failed to create MCP service");
//!
//!     // Service implements rmcp::handler::server::ServerHandler
//!     // Can be used with rmcp's transport layers (stdio, SSE, etc.)
//! }
//! ```

use crate::error::McpServiceError;
use crate::models::instance::ToolInstance;
use crate::models::tool::Tool;
use crate::services::instance_executor::InstanceExecutor;
use crate::services::schema_generator::SchemaGenerator;
use crate::services::secrets_manager::SecretsManager;
use rmcp::handler::server::tool::{ToolCallContext, ToolRoute, ToolRouter};
use rmcp::handler::server::ServerHandler;
use rmcp::model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Instance data loaded from database
struct InstanceData {
    server_id: i64,
    instance_id: i64,
    instance_name: String,
    description: Option<String>,
    tool: Tool,
}

#[derive(Clone)]
pub struct SaraMcpService {
    server_id: i64,
    pool: SqlitePool,
    tool_router: Arc<RwLock<ToolRouter<Self>>>,
    secrets: SecretsManager,
}

impl SaraMcpService {
    /// Load all tool instances for this server from database
    ///
    /// Fetches all tool instances and their associated tool definitions
    /// for the given server ID.
    ///
    /// # Arguments
    ///
    /// * `pool` - Database connection pool
    /// * `server_id` - ID of the server whose instances to load
    ///
    /// # Returns
    ///
    /// Vector of InstanceData containing instance metadata and tool definitions
    ///
    /// # Errors
    ///
    /// * `McpServiceError::Database` - Database query failure
    /// * `McpServiceError::Internal` - Instance missing ID
    /// * `McpServiceError::ToolNotFound` - Referenced tool does not exist
    async fn load_instances(
        pool: &SqlitePool,
        server_id: i64,
    ) -> Result<Vec<InstanceData>, McpServiceError> {
        let instances = ToolInstance::list_by_server(pool, server_id).await?;

        let mut result = Vec::new();
        for instance in instances {
            let instance_id = instance
                .id
                .ok_or_else(|| McpServiceError::Internal("Instance missing ID".to_string()))?;

            let tool = Tool::get_by_id(pool, instance.tool_id)
                .await?
                .ok_or_else(|| {
                    McpServiceError::ToolNotFound(format!("Tool {} not found", instance.tool_id))
                })?;

            result.push(InstanceData {
                server_id,
                instance_id,
                instance_name: instance.instance_name,
                description: instance.description,
                tool,
            });
        }

        Ok(result)
    }

    /// Register a single instance as a dynamic tool
    ///
    /// Creates a ToolRoute for a single instance, including:
    /// - JSON Schema generation for exposed parameters
    /// - MCP tool definition with metadata
    /// - InstanceExecutor for handling calls
    ///
    /// # Arguments
    ///
    /// * `pool` - Database connection pool
    /// * `instance` - Instance data including ID, name, and tool definition
    /// * `secrets` - Secrets manager for parameter resolution
    ///
    /// # Returns
    ///
    /// A ToolRoute ready to be added to the ToolRouter
    ///
    /// # Errors
    ///
    /// * `McpServiceError::SchemaGeneration` - Schema is not a JSON object
    /// * Other errors from SchemaGenerator
    async fn build_tool_route(
        pool: &SqlitePool,
        instance: InstanceData,
        secrets: SecretsManager,
    ) -> Result<ToolRoute<Self>, McpServiceError> {
        let schema = SchemaGenerator::generate_for_instance(pool, instance.instance_id).await?;

        let schema_map = if let serde_json::Value::Object(map) = schema {
            map
        } else {
            return Err(McpServiceError::SchemaGeneration(
                "Schema is not an object".to_string(),
            ));
        };

        let tool_def = rmcp::model::Tool {
            name: instance.instance_name.clone().into(),
            description: instance.description.clone().map(Into::into),
            input_schema: std::sync::Arc::new(schema_map),
            annotations: None,
            title: None,
            icons: None,
            output_schema: None,
        };

        let executor = InstanceExecutor::new(
            pool.clone(),
            instance.server_id,
            instance.instance_id,
            instance.tool.clone(),
            secrets,
        );

        let route = ToolRoute::new_dyn(tool_def, move |context: ToolCallContext<'_, Self>| {
            let exec = executor.clone();
            Box::pin(async move { exec.execute(context.arguments).await })
        });

        Ok(route)
    }

    /// Build tool router from instances
    ///
    /// Loads all instances for a server and creates a complete ToolRouter
    /// with all tools registered.
    ///
    /// # Arguments
    ///
    /// * `pool` - Database connection pool
    /// * `server_id` - ID of the server
    /// * `secrets` - Secrets manager for all instances
    ///
    /// # Returns
    ///
    /// A ToolRouter containing all registered tool instances
    ///
    /// # Errors
    ///
    /// Propagates errors from load_instances and build_tool_route
    async fn build_router(
        pool: &SqlitePool,
        server_id: i64,
        secrets: SecretsManager,
    ) -> Result<ToolRouter<Self>, McpServiceError> {
        let instances = Self::load_instances(pool, server_id).await?;

        let mut router = ToolRouter::new();
        for instance in instances {
            let route = Self::build_tool_route(pool, instance, secrets.clone()).await?;
            router.add_route(route);
        }

        Ok(router)
    }

    /// Create new MCP service for a server
    ///
    /// Initializes a complete MCP service by loading all tool instances,
    /// generating schemas, and setting up the tool router.
    ///
    /// # Arguments
    ///
    /// * `server_id` - ID of the server to serve
    /// * `pool` - Database connection pool
    ///
    /// # Returns
    ///
    /// A fully initialized SaraMcpService ready to handle MCP protocol calls
    ///
    /// # Errors
    ///
    /// * `McpServiceError::Internal` - Secrets manager initialization failed
    /// * Other errors from build_router
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use saramcp::mcp::service::SaraMcpService;
    /// # use sqlx::SqlitePool;
    /// # async fn example(pool: SqlitePool) {
    /// let service = SaraMcpService::new(1, pool).await.unwrap();
    /// // Service is now ready to handle MCP calls
    /// # }
    /// ```
    pub async fn new(server_id: i64, pool: SqlitePool) -> Result<Self, McpServiceError> {
        let secrets = SecretsManager::new()
            .map_err(|e| McpServiceError::Internal(format!("Secrets manager error: {}", e)))?;

        let tool_router = Self::build_router(&pool, server_id, secrets.clone()).await?;

        Ok(Self {
            server_id,
            pool: pool.clone(),
            tool_router: Arc::new(RwLock::new(tool_router)),
            secrets,
        })
    }

    /// Reload all tools (called when instances change)
    ///
    /// Rebuilds the ToolRouter from the current database state. Call this
    /// after adding, removing, or modifying tool instances.
    ///
    /// This method uses interior mutability via RwLock to allow reloading
    /// without requiring exclusive mutable access to the service.
    ///
    /// # Errors
    ///
    /// Propagates errors from build_router
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use saramcp::mcp::service::SaraMcpService;
    /// # async fn example(service: &SaraMcpService) {
    /// // After modifying instances in the database
    /// service.reload_tools().await.unwrap();
    /// // Service now reflects the updated instance configuration
    /// # }
    /// ```
    pub async fn reload_tools(&self) -> Result<(), McpServiceError> {
        let new_router =
            Self::build_router(&self.pool, self.server_id, self.secrets.clone()).await?;
        let mut router = self.tool_router.write().await;
        *router = new_router;
        Ok(())
    }

    /// Get tool routes for Router creation
    ///
    /// Rebuilds all tool routes from the database for use with rmcp::handler::server::Router.
    /// This is used by the SSE transport to create a Router that implements Service.
    pub async fn get_tool_routes(
        &self,
    ) -> Result<Vec<rmcp::handler::server::tool::ToolRoute<Self>>, McpServiceError> {
        let instances = Self::load_instances(&self.pool, self.server_id).await?;
        let mut routes = Vec::new();
        for instance in instances {
            let route = Self::build_tool_route(&self.pool, instance, self.secrets.clone()).await?;
            routes.push(route);
        }
        Ok(routes)
    }

    /// Handle a single JSON-RPC request (for Streamable HTTP transport)
    ///
    /// Processes MCP protocol requests sent as JSON-RPC messages and returns
    /// the appropriate JSON-RPC response. This is used for the Streamable HTTP
    /// transport where each request gets a single response.
    ///
    /// # Arguments
    ///
    /// * `request` - JSON-RPC request object containing method and params
    ///
    /// # Returns
    ///
    /// * `Ok(Value)` - JSON-RPC response with result or error
    /// * `Err(McpServiceError)` - If request parsing or execution fails
    ///
    /// # Supported Methods
    ///
    /// * `initialize` - Returns server info and capabilities
    /// * `tools/list` - Returns list of available tools
    /// * `tools/call` - Executes a tool with given arguments
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use saramcp::mcp::service::SaraMcpService;
    /// # use serde_json::json;
    /// # async fn example(service: &SaraMcpService) {
    /// let request = json!({
    ///     "jsonrpc": "2.0",
    ///     "id": 1,
    ///     "method": "initialize",
    ///     "params": {}
    /// });
    /// let response = service.handle_request(request).await.unwrap();
    /// # }
    /// ```
    pub async fn handle_request(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, McpServiceError> {
        use serde_json::json;

        // Parse request ID
        let request_id = request.get("id").cloned();

        // Get method - handle missing method as an error, not early return
        let method_result = request
            .get("method")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpServiceError::Internal("Missing method field".to_string()));

        // Route based on method
        let result = match method_result {
            Ok(method) => match method {
                "initialize" => {
                    // Return server info
                    let info = self.get_info();
                    Ok(json!({
                        "protocolVersion": info.protocol_version,
                        "capabilities": info.capabilities,
                        "serverInfo": info.server_info,
                        "instructions": info.instructions,
                    }))
                }

                "tools/list" => {
                    // List all available tools from the router
                    let router = self.tool_router.read().await;
                    let tools = router.list_all();
                    Ok(json!({
                        "tools": tools,
                    }))
                }

                "tools/call" => {
                    // Extract tool call parameters
                    let params = request.get("params").ok_or_else(|| {
                        McpServiceError::Internal("Missing params field".to_string())
                    })?;

                    let tool_name =
                        params.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                            McpServiceError::Internal("Missing tool name".to_string())
                        })?;

                    let arguments = params.get("arguments").and_then(|v| v.as_object()).cloned();

                    // For HTTP transport, we need to call the tool directly without
                    // creating a full MCP session context. We'll find the instance
                    // and execute it directly using InstanceExecutor.

                    // Parse the tool name to get instance name
                    // (In our system, instance_name = tool name in MCP)
                    let instance = crate::models::instance::ToolInstance::find_by_server_and_name(
                        &self.pool,
                        self.server_id,
                        tool_name,
                    )
                    .await
                    .map_err(|e| McpServiceError::Internal(e.to_string()))?
                    .ok_or_else(|| {
                        McpServiceError::Internal(format!(
                            "Tool instance '{}' not found",
                            tool_name
                        ))
                    })?;

                    let instance_id = instance.id.ok_or_else(|| {
                        McpServiceError::Internal("Instance missing ID".to_string())
                    })?;

                    // Get the tool definition
                    let tool = crate::models::tool::Tool::get_by_id(&self.pool, instance.tool_id)
                        .await
                        .map_err(|e| McpServiceError::Internal(e.to_string()))?
                        .ok_or_else(|| {
                            McpServiceError::ToolNotFound(format!(
                                "Tool {} not found",
                                instance.tool_id
                            ))
                        })?;

                    // Execute using InstanceExecutor directly
                    let executor = crate::services::instance_executor::InstanceExecutor::new(
                        self.pool.clone(),
                        self.server_id,
                        instance_id,
                        tool,
                        self.secrets.clone(),
                    );

                    let call_result = executor
                        .execute(arguments)
                        .await
                        .map_err(|e| McpServiceError::Internal(e.to_string()))?;

                    Ok(serde_json::to_value(&call_result).map_err(|e| {
                        McpServiceError::Internal(format!("Failed to serialize result: {}", e))
                    })?)
                }

                _ => {
                    // Method not supported
                    Err(McpServiceError::Internal(format!(
                        "Method '{}' not supported",
                        method
                    )))
                }
            },
            Err(e) => Err(e),
        };

        // Build JSON-RPC response
        match result {
            Ok(res) => Ok(json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "result": res,
            })),
            Err(e) => Ok(json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {
                    "code": -32603,
                    "message": e.to_string(),
                },
            })),
        }
    }
}

impl ServerHandler for SaraMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: format!("saramcp-server-{}", self.server_id),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                website_url: None,
                icons: None,
            },
            instructions: Some(format!("SaraMCP Server {}", self.server_id)),
        }
    }
}
