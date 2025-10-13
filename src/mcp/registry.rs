//! Thread-safe MCP server registry
//!
//! Manages dynamic registration, unregistration, and lookup of MCP server instances.
//! Uses `Arc<RwLock<HashMap<>>>` for thread-safe concurrent access.

use crate::mcp::instance::McpServerInstance;
use crate::models::server::Server;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Server not found: {0}")]
    ServerNotFound(String),

    #[error("Server already registered: {0}")]
    AlreadyRegistered(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Instance creation failed: {0}")]
    InstanceCreation(String),

    #[error("Server query error: {0}")]
    ServerQuery(#[from] anyhow::Error),
}

/// Thread-safe registry of MCP server instances
///
/// Manages a collection of [`McpServerInstance`]s indexed by server UUID.
/// Supports concurrent read access and exclusive write access via RwLock.
///
/// # Thread Safety
///
/// This struct is designed to be wrapped in `Arc<RwLock<>>` for sharing
/// across multiple handlers and tasks.
///
/// # Examples
///
/// ```rust,no_run
/// use saramcp::mcp::registry::McpServerRegistry;
/// use sqlx::SqlitePool;
///
/// # async fn example(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
/// let mut registry = McpServerRegistry::new(pool);
/// registry.load_all_servers().await?;
/// registry.register_server("uuid-1234").await?;
/// # Ok(())
/// # }
/// ```
pub struct McpServerRegistry {
    instances: HashMap<String, Arc<McpServerInstance>>,
    pool: SqlitePool,
}

pub type SharedRegistry = Arc<RwLock<McpServerRegistry>>;

impl McpServerRegistry {
    /// Creates a new empty registry
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            instances: HashMap::new(),
            pool,
        }
    }

    /// Registers a new MCP server instance
    ///
    /// # Arguments
    ///
    /// * `uuid` - Server UUID to register
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Server registered successfully
    /// * `Err(RegistryError::AlreadyRegistered)` - Server already exists
    /// * `Err(RegistryError::Database)` - Database error
    pub async fn register_server(&mut self, uuid: &str) -> Result<(), RegistryError> {
        if self.instances.contains_key(uuid) {
            return Err(RegistryError::AlreadyRegistered(uuid.to_string()));
        }

        let server = Server::get_by_uuid(&self.pool, uuid)
            .await?
            .ok_or_else(|| RegistryError::ServerNotFound(uuid.to_string()))?;

        let server_id = server
            .id
            .ok_or_else(|| RegistryError::InstanceCreation("Server has no ID".to_string()))?;

        let instance = McpServerInstance::new(self.pool.clone(), server_id, uuid.to_string())
            .await
            .map_err(|e| RegistryError::InstanceCreation(e.to_string()))?;

        self.instances.insert(uuid.to_string(), Arc::new(instance));

        Ok(())
    }

    /// Unregisters and shuts down an MCP server instance
    ///
    /// # Arguments
    ///
    /// * `uuid` - Server UUID to unregister
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Server unregistered successfully
    /// * `Err(RegistryError::ServerNotFound)` - Server doesn't exist
    pub async fn unregister_server(&mut self, uuid: &str) -> Result<(), RegistryError> {
        let instance = self
            .instances
            .remove(uuid)
            .ok_or_else(|| RegistryError::ServerNotFound(uuid.to_string()))?;

        instance
            .shutdown()
            .await
            .map_err(|e| RegistryError::InstanceCreation(e.to_string()))?;

        Ok(())
    }

    /// Reloads tool definitions for a server
    ///
    /// # Arguments
    ///
    /// * `uuid` - Server UUID to reload
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Tools reloaded successfully
    /// * `Err(RegistryError::ServerNotFound)` - Server doesn't exist
    ///
    /// # Note
    ///
    /// This is currently a stub. Full implementation will be added in Task 003.
    pub async fn reload_tools(&mut self, uuid: &str) -> Result<(), RegistryError> {
        let instance = self
            .instances
            .get(uuid)
            .ok_or_else(|| RegistryError::ServerNotFound(uuid.to_string()))?;

        instance
            .reload_tools()
            .await
            .map_err(|e| RegistryError::InstanceCreation(e.to_string()))?;

        Ok(())
    }

    /// Retrieves an MCP server instance by UUID
    ///
    /// # Arguments
    ///
    /// * `uuid` - Server UUID to lookup
    ///
    /// # Returns
    ///
    /// * `Some(Arc<McpServerInstance>)` - Instance found
    /// * `None` - Instance not found
    pub fn get_instance(&self, uuid: &str) -> Option<Arc<McpServerInstance>> {
        self.instances.get(uuid).map(Arc::clone)
    }

    /// Loads all servers from the database on startup
    ///
    /// # Returns
    ///
    /// * `Ok(())` - All servers loaded successfully
    /// * `Err(RegistryError)` - Database error
    ///
    /// # Behavior
    ///
    /// Continues loading even if some servers fail to register.
    /// Failures are logged but don't stop the startup process.
    pub async fn load_all_servers(&mut self) -> Result<(), RegistryError> {
        let servers = sqlx::query!("SELECT id, uuid FROM servers")
            .fetch_all(&self.pool)
            .await?;

        for server in servers {
            let uuid = match &server.uuid {
                Some(u) => u,
                None => {
                    tracing::warn!("Server {:?} has no UUID, skipping", server.id);
                    continue;
                }
            };

            if let Err(e) = self.register_server(uuid).await {
                tracing::warn!(
                    "Failed to register server {} ({:?}): {}",
                    uuid,
                    server.id,
                    e
                );
            }
        }

        Ok(())
    }

    /// Shuts down all registered server instances
    ///
    /// # Returns
    ///
    /// * `Ok(())` - All instances shutdown successfully
    pub async fn shutdown_all(&mut self) -> Result<(), RegistryError> {
        let instances: Vec<_> = self.instances.drain().collect();

        for (uuid, instance) in instances {
            if let Err(e) = instance.shutdown().await {
                tracing::error!("Failed to shutdown server {}: {}", uuid, e);
            }
        }

        Ok(())
    }
}
