// RMCP Integration Proof of Concept
// Tests if rmcp SseServer can support multiple instances with different paths

#![cfg(test)]
#![allow(clippy::redundant_closure)]
use anyhow::Result;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, ServerHandler},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::sse_server::{SseServer, SseServerConfig},
};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

// Mock MCP Service for Server 1
#[derive(Debug, Clone)]
struct Server1McpService {
    tool_router: ToolRouter<Self>,
}

impl Server1McpService {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl Server1McpService {
    #[tool(description = "Test tool from Server 1")]
    pub async fn server1_ping(&self) -> String {
        r#"{"server": "server1", "status": "pong"}"#.to_string()
    }
}

#[tool_handler]
impl ServerHandler for Server1McpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: rmcp::model::Implementation {
                name: "server1".to_string(),
                version: "1.0.0".to_string(),
                title: None,
                website_url: None,
                icons: None,
            },
            instructions: Some("Server 1 MCP Service".to_string()),
        }
    }
}

// Mock MCP Service for Server 2
#[derive(Debug, Clone)]
struct Server2McpService {
    tool_router: ToolRouter<Self>,
}

impl Server2McpService {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl Server2McpService {
    #[tool(description = "Test tool from Server 2")]
    pub async fn server2_ping(&self) -> String {
        r#"{"server": "server2", "status": "pong"}"#.to_string()
    }
}

#[tool_handler]
impl ServerHandler for Server2McpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: rmcp::model::Implementation {
                name: "server2".to_string(),
                version: "1.0.0".to_string(),
                title: None,
                website_url: None,
                icons: None,
            },
            instructions: Some("Server 2 MCP Service".to_string()),
        }
    }
}

// Test 1: Create single SSE server with custom paths
#[tokio::test]
async fn test_single_sse_server_custom_path() -> Result<()> {
    let ct = CancellationToken::new();

    let config = SseServerConfig {
        bind: "127.0.0.1:0".parse()?,
        sse_path: "/custom/sse".to_string(),
        post_path: "/custom/message".to_string(),
        ct: ct.clone(),
        sse_keep_alive: Some(Duration::from_secs(30)),
    };

    let (sse_server, sse_router) = SseServer::new(config);

    // Register service
    sse_server.with_service(|| Server1McpService::new());

    // Verify router was created
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .merge(sse_router);

    // Test health endpoint (proves routing works)
    let response = app
        .oneshot(Request::builder().uri("/health").body(Body::empty())?)
        .await?;

    assert_eq!(response.status(), StatusCode::OK);

    println!("✅ Single SSE server with custom paths created successfully");
    Ok(())
}

// Test 2: CRITICAL - Multiple SSE servers with different paths
#[tokio::test]
async fn test_multiple_sse_servers_different_paths() -> Result<()> {
    let ct = CancellationToken::new();

    // Server 1 on /s/uuid1
    let config1 = SseServerConfig {
        bind: "127.0.0.1:0".parse()?,
        sse_path: "/s/uuid1".to_string(),
        post_path: "/s/uuid1/message".to_string(),
        ct: ct.clone(),
        sse_keep_alive: Some(Duration::from_secs(30)),
    };

    let (sse_server1, sse_router1) = SseServer::new(config1);
    sse_server1.with_service(|| Server1McpService::new());

    // Server 2 on /s/uuid2
    let config2 = SseServerConfig {
        bind: "127.0.0.1:0".parse()?,
        sse_path: "/s/uuid2".to_string(),
        post_path: "/s/uuid2/message".to_string(),
        ct: ct.clone(),
        sse_keep_alive: Some(Duration::from_secs(30)),
    };

    let (sse_server2, sse_router2) = SseServer::new(config2);
    sse_server2.with_service(|| Server2McpService::new());

    // Merge both routers
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .merge(sse_router1)
        .merge(sse_router2);

    // Test health
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/health").body(Body::empty())?)
        .await?;

    assert_eq!(response.status(), StatusCode::OK);

    println!("✅ Multiple SSE servers merged successfully");
    println!("   - Server 1: /s/uuid1, /s/uuid1/message");
    println!("   - Server 2: /s/uuid2, /s/uuid2/message");

    Ok(())
}

// Test 3: Path conflicts detection
#[tokio::test]
async fn test_path_conflicts() -> Result<()> {
    let ct = CancellationToken::new();

    // Try to create two servers with same path
    let config1 = SseServerConfig {
        bind: "127.0.0.1:0".parse()?,
        sse_path: "/same/path".to_string(),
        post_path: "/same/path/message".to_string(),
        ct: ct.clone(),
        sse_keep_alive: Some(Duration::from_secs(30)),
    };

    let config2 = SseServerConfig {
        bind: "127.0.0.1:0".parse()?,
        sse_path: "/same/path".to_string(), // Same path!
        post_path: "/same/path/message".to_string(),
        ct: ct.clone(),
        sse_keep_alive: Some(Duration::from_secs(30)),
    };

    let (sse_server1, sse_router1) = SseServer::new(config1);
    sse_server1.with_service(|| Server1McpService::new());

    let (sse_server2, sse_router2) = SseServer::new(config2);
    sse_server2.with_service(|| Server2McpService::new());

    // In Axum 0.8, this now panics with overlapping routes
    // This is actually better behavior than 0.7's undefined behavior
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Router::new().merge(sse_router1).merge(sse_router2)
    }));

    // Verify that it panics as expected in Axum 0.8
    assert!(
        result.is_err(),
        "Expected panic due to overlapping routes in Axum 0.8"
    );

    println!("✓ Path conflict test passed: Axum 0.8 correctly detects overlapping routes");
    println!("   This is an improvement over Axum 0.7's undefined behavior");

    Ok(())
}

// Test 4: Dynamic server creation simulation
#[tokio::test]
async fn test_dynamic_server_creation() -> Result<()> {
    let ct = CancellationToken::new();

    // Simulate loading servers from database
    let servers = vec![
        ("uuid-1", "Production"),
        ("uuid-2", "Staging"),
        ("uuid-3", "Development"),
    ];

    let mut app = Router::new().route("/health", get(|| async { "OK" }));

    // Create SSE server for each
    for (uuid, _name) in &servers {
        let config = SseServerConfig {
            bind: "127.0.0.1:0".parse()?,
            sse_path: format!("/s/{}", uuid),
            post_path: format!("/s/{}/message", uuid),
            ct: ct.clone(),
            sse_keep_alive: Some(Duration::from_secs(30)),
        };

        let (sse_server, sse_router) = SseServer::new(config);

        // Register appropriate service based on server
        if uuid.ends_with('1') {
            sse_server.with_service(|| Server1McpService::new());
        } else {
            sse_server.with_service(|| Server2McpService::new());
        }

        app = app.merge(sse_router);
    }

    // Test health
    let response = app
        .oneshot(Request::builder().uri("/health").body(Body::empty())?)
        .await?;

    assert_eq!(response.status(), StatusCode::OK);

    println!("✅ Dynamic server creation successful");
    println!("   Created {} servers with unique paths", servers.len());

    Ok(())
}

// Test 5: Performance test - many servers
#[tokio::test]
async fn test_many_servers_performance() -> Result<()> {
    let ct = CancellationToken::new();
    let num_servers = 50;

    let mut app = Router::new().route("/health", get(|| async { "OK" }));

    let start = std::time::Instant::now();

    for i in 0..num_servers {
        let config = SseServerConfig {
            bind: "127.0.0.1:0".parse()?,
            sse_path: format!("/s/server-{}", i),
            post_path: format!("/s/server-{}/message", i),
            ct: ct.clone(),
            sse_keep_alive: Some(Duration::from_secs(30)),
        };

        let (sse_server, sse_router) = SseServer::new(config);
        sse_server.with_service(|| Server1McpService::new());
        app = app.merge(sse_router);
    }

    let duration = start.elapsed();

    // Test random server
    let response = app
        .oneshot(Request::builder().uri("/s/server-25").body(Body::empty())?)
        .await?;

    println!("✅ Performance test completed");
    println!("   Created {} SSE servers in {:?}", num_servers, duration);
    println!("   Test request status: {:?}", response.status());

    Ok(())
}
