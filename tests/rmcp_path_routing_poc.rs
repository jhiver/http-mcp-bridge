// Proof of Concept: Test if rmcp SseServer supports path-based routing
// This test validates Solution 1 from the implementation plan
//
// Test Cases:
// 1. Can we create multiple SSE servers with different paths?
// 2. Can we merge multiple rmcp routers into one Axum app?
// 3. Does each path route to the correct service instance?

use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use tower::ServiceExt; // for oneshot

// Mock structures to simulate SaraMCP services
#[allow(dead_code)]
#[derive(Clone, Debug)]
struct MockMcpService {
    server_name: String,
}

#[allow(dead_code)]
impl MockMcpService {
    fn new(server_name: String) -> Self {
        Self { server_name }
    }
}

#[derive(Serialize, Deserialize)]
struct TestResponse {
    server: String,
    message: String,
}

// Test 1: Basic path routing without rmcp
#[tokio::test]
async fn test_basic_axum_path_routing() {
    let app = Router::new()
        .route("/s/server1", get(|| async { "Server 1" }))
        .route("/s/server2", get(|| async { "Server 2" }))
        .route("/health", get(|| async { "OK" }));

    // Test server1 path
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/s/server1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Server 1");

    // Test server2 path
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/s/server2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Server 2");

    // Test health endpoint
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

// Test 2: Nested routing with path parameters
#[tokio::test]
async fn test_nested_path_routing() {
    use axum::extract::Path;

    async fn server_handler(Path(server_id): Path<String>) -> String {
        format!("Server: {}", server_id)
    }

    let app = Router::new()
        .route("/s/{server_id}", get(server_handler))
        .route("/s/{server_id}/message", get(server_handler));

    // Test dynamic path
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/s/abc-123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Server: abc-123");

    // Test message path
    let response = app
        .oneshot(
            Request::builder()
                .uri("/s/xyz-789/message")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Server: xyz-789");
}

// Test 3: Multiple router merging (simulates multiple SSE servers)
#[tokio::test]
async fn test_multiple_router_merging() {
    // Create router for server 1
    let server1_router = Router::new()
        .route("/s/server1", get(|| async { "Server 1 SSE" }))
        .route("/s/server1/message", get(|| async { "Server 1 POST" }));

    // Create router for server 2
    let server2_router = Router::new()
        .route("/s/server2", get(|| async { "Server 2 SSE" }))
        .route("/s/server2/message", get(|| async { "Server 2 POST" }));

    // Merge routers
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .merge(server1_router)
        .merge(server2_router);

    // Test server 1 SSE
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/s/server1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Server 1 SSE");

    // Test server 2 message
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/s/server2/message")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Server 2 POST");
}

// Test 4: State isolation between routers
#[tokio::test]
async fn test_router_state_isolation() {
    use axum::extract::State;
    use std::sync::Arc;

    #[derive(Clone)]
    struct ServerState {
        name: String,
    }

    async fn handler(State(state): State<Arc<ServerState>>) -> String {
        format!("Server: {}", state.name)
    }

    let state1 = Arc::new(ServerState {
        name: "Server 1".to_string(),
    });
    let state2 = Arc::new(ServerState {
        name: "Server 2".to_string(),
    });

    let router1 = Router::new()
        .route("/s/server1", get(handler))
        .with_state(state1);

    let router2 = Router::new()
        .route("/s/server2", get(handler))
        .with_state(state2);

    let app = Router::new().merge(router1).merge(router2);

    // Test server 1
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/s/server1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Server: Server 1");

    // Test server 2
    let response = app
        .oneshot(
            Request::builder()
                .uri("/s/server2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Server: Server 2");
}

// Test 5: CRITICAL TEST - Can rmcp SseServer work with custom paths and be merged?
// NOTE: This requires rmcp dependency. Uncomment when ready to test.
/*
use rmcp::transport::sse_server::{SseServer, SseServerConfig};

#[tokio::test]
async fn test_rmcp_multiple_sse_servers_different_paths() -> Result<()> {
    let ct = CancellationToken::new();

    // Create first SSE server on /s/server1
    let config1 = SseServerConfig {
        bind: "127.0.0.1:0".parse()?, // Use port 0 for testing (OS assigns)
        sse_path: "/s/server1".to_string(),
        post_path: "/s/server1/message".to_string(),
        ct: ct.clone(),
        sse_keep_alive: Some(Duration::from_secs(30)),
    };

    let (sse_server1, sse_router1) = SseServer::new(config1);

    // Mock service for server 1
    sse_server1.with_service(|| MockMcpService::new("Server 1".to_string()));

    // Create second SSE server on /s/server2
    let config2 = SseServerConfig {
        bind: "127.0.0.1:0".parse()?,
        sse_path: "/s/server2".to_string(),
        post_path: "/s/server2/message".to_string(),
        ct: ct.clone(),
        sse_keep_alive: Some(Duration::from_secs(30)),
    };

    let (sse_server2, sse_router2) = SseServer::new(config2);

    // Mock service for server 2
    sse_server2.with_service(|| MockMcpService::new("Server 2".to_string()));

    // Merge both SSE routers
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .merge(sse_router1)
        .merge(sse_router2);

    // Test that routes exist
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);

    // Note: Full SSE functionality requires an actual server running
    // This test validates that:
    // 1. Multiple SseServer instances can be created
    // 2. Each with different paths
    // 3. Their routers can be merged
    // 4. The merged router can be used in an Axum app

    Ok(())
}
*/

// Test 6: Simulate the full architecture pattern
#[tokio::test]
async fn test_full_multi_server_pattern() {
    use axum::extract::State;
    use std::sync::Arc;

    // Simulate database state
    #[derive(Clone)]
    struct AppState {
        servers: Arc<Vec<(String, String)>>, // (uuid, name)
    }

    // Simulate SSE handler
    async fn sse_handler(
        axum::extract::Path(server_uuid): axum::extract::Path<String>,
        State(state): State<Arc<AppState>>,
    ) -> Result<String, StatusCode> {
        state
            .servers
            .iter()
            .find(|(uuid, _)| uuid == &server_uuid)
            .map(|(_, name)| format!("SSE for {}", name))
            .ok_or(StatusCode::NOT_FOUND)
    }

    // Simulate POST handler
    async fn post_handler(
        axum::extract::Path(server_uuid): axum::extract::Path<String>,
        State(state): State<Arc<AppState>>,
    ) -> Result<String, StatusCode> {
        state
            .servers
            .iter()
            .find(|(uuid, _)| uuid == &server_uuid)
            .map(|(_, name)| format!("POST for {}", name))
            .ok_or(StatusCode::NOT_FOUND)
    }

    let state = Arc::new(AppState {
        servers: Arc::new(vec![
            ("uuid-1".to_string(), "Production".to_string()),
            ("uuid-2".to_string(), "Staging".to_string()),
        ]),
    });

    let app = Router::new()
        .route("/s/{server_uuid}", get(sse_handler))
        .route("/s/{server_uuid}/message", get(post_handler))
        .with_state(state);

    // Test production server SSE
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/s/uuid-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"SSE for Production");

    // Test staging server POST
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/s/uuid-2/message")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"POST for Staging");

    // Test non-existent server
    let response = app
        .oneshot(
            Request::builder()
                .uri("/s/non-existent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// Test 7: Benchmark - how many concurrent SSE servers can we handle?
#[tokio::test]
async fn test_scalability_many_servers() {
    // Create 100 virtual servers with STATIC paths (not dynamic)
    let mut app = Router::new();
    for i in 0..100 {
        let route = format!("/s/server{}", i);
        let response_text = format!("Server {}", i);
        app = app.route(&route, get(move || async move { response_text.clone() }));
    }

    // Test first server
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/s/server0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Server 0");

    // Test last server
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/s/server99")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Server 99");

    // Test middle server
    let response = app
        .oneshot(
            Request::builder()
                .uri("/s/server50")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Server 50");

    println!("✅ Successfully routed 100 different server paths");
}
