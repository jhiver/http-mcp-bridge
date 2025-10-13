//! MCP server dynamic routing and registry management
//!
//! This module enables hot-reload of MCP servers without application restart.
//! It provides a thread-safe registry of server instances with dynamic routing
//! via wildcard paths.
//!
//! # Architecture
//!
//! - [`McpServerRegistry`] - Thread-safe registry of active MCP server instances
//! - [`McpServerInstance`] - Individual server with SSE transport and service
//! - [`SaraMcpService`] - MCP protocol handler (stub for Task 002, full impl in Task 003)
//! - [`mcp_sse_handler`] and [`mcp_message_handler`] - HTTP request dispatchers
//!
//! # Example
//!
//! ```rust,no_run
//! use saramcp::mcp::registry::McpServerRegistry;
//! use sqlx::SqlitePool;
//! use std::sync::Arc;
//! use tokio::sync::RwLock;
//!
//! # async fn example(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
//! // Create and initialize registry
//! let registry = Arc::new(RwLock::new(McpServerRegistry::new(pool.clone())));
//! registry.write().await.load_all_servers().await?;
//!
//! // Register a new server
//! registry.write().await.register_server("server-uuid-1234").await?;
//!
//! // Get an instance
//! let instance = registry.read().await.get_instance("server-uuid-1234");
//! # Ok(())
//! # }
//! ```

pub mod handlers;
pub mod http_transport;
pub mod instance;
pub mod registry;
pub mod service;

pub use handlers::{mcp_message_handler, mcp_sse_handler};
pub use instance::McpServerInstance;
pub use registry::{McpServerRegistry, RegistryError, SharedRegistry};
pub use service::SaraMcpService;
