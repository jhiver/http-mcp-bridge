use saramcp::{
    auth,
    config::session::{validate_production_config, SessionConfig},
    db, handlers, mcp, repositories, services, AppState,
};

use axum::{
    body::Body,
    extract::{Query, State},
    http::{HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use repositories::{
    tool_repository::SqliteToolRepository, toolkit_repository::SqliteToolkitRepository,
    user_repository::SqliteUserRepository,
};
use services::{
    auth_service::AuthService, tool_service::ToolService, toolkit_service::ToolkitService,
    user_service::UserService,
};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tower_sessions::Session;
use tower_sessions_sqlx_store::SqliteStore;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "saramcp=debug,tower_http=debug,axum::rejection=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Database connection
    let pool = db::create_pool().await?;

    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await?;

    // Initialize repositories
    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let toolkit_repository = Arc::new(SqliteToolkitRepository::new(pool.clone()));
    let tool_repository = Arc::new(SqliteToolRepository::new(pool.clone()));

    // Initialize services
    let user_service = Arc::new(UserService::new(user_repository.clone()));
    let auth_service = Arc::new(AuthService::new(user_repository.clone()));
    let toolkit_service = Arc::new(ToolkitService::new(
        toolkit_repository.clone(),
        tool_repository.clone(),
    ));
    let tool_service = Arc::new(ToolService::new(
        tool_repository.clone(),
        toolkit_repository.clone(),
    ));

    // Initialize server and instance services
    let secrets_manager = services::SecretsManager::new()?;
    let server_service = Arc::new(services::ServerService::new(
        pool.clone(),
        secrets_manager.clone(),
    ));
    let instance_service = Arc::new(services::InstanceService::new(
        pool.clone(),
        secrets_manager,
    ));

    // Initialize OAuth service
    let oauth_service = Arc::new(services::OAuthService::new(pool.clone()));

    // Initialize email and auth token services
    let email_service = services::create_email_service();
    let auth_token_service = Arc::new(services::AuthTokenService::new(
        pool.clone(),
        email_service,
        user_repository.clone(),
        user_service.clone(),
    ));

    // Initialize MCP registry
    let mcp_registry = Arc::new(RwLock::new(mcp::McpServerRegistry::new(pool.clone())));

    tracing::info!("Loading MCP servers into registry...");

    // Load existing servers into registry
    {
        let mut registry_write = mcp_registry.write().await;
        if let Err(e) = registry_write.load_all_servers().await {
            tracing::warn!("Failed to load MCP servers: {}", e);
        } else {
            tracing::info!("MCP servers loaded successfully");
        }
    }

    // Create app state
    let app_state = AppState {
        user_service,
        auth_service,
        auth_token_service,
        toolkit_service: Some(toolkit_service),
        tool_service: Some(tool_service),
        server_service: Some(server_service),
        instance_service: Some(instance_service),
        oauth_service,
        toolkit_repository: Some(toolkit_repository as Arc<dyn repositories::ToolkitRepository>),
        tool_repository: Some(tool_repository as Arc<dyn repositories::ToolRepository>),
        mcp_registry: Some(mcp_registry.clone()),
        pool: pool.clone(),
    };

    // Session store
    validate_production_config();
    let session_store = SqliteStore::new(pool.clone())
        .with_table_name("sessions")
        .expect("Invalid session table name for sessions");
    session_store.migrate().await?;

    let session_layer = SessionConfig::from_env().create_layer(session_store);

    // Build application routes
    let protected_routes = Router::new()
        .route("/dashboard", get(handlers::dashboard_handler))
        .route("/toolkits", get(toolkits_handler))
        // Settings routes
        .route("/settings", get(handlers::show_settings_page))
        .route(
            "/settings/password",
            post(handlers::update_password_handler),
        )
        .route("/settings/email", post(handlers::update_email_handler))
        // Toolkit routes
        .route("/toolkits/new", get(handlers::create_toolkit_page))
        .route("/toolkits/explore", get(handlers::explore_toolkits_handler))
        .route("/toolkits", post(handlers::create_toolkit_handler))
        .route("/toolkits/{id}", get(handlers::view_toolkit_handler))
        .route(
            "/toolkits/{id}/public",
            get(handlers::view_public_toolkit_handler),
        )
        .route(
            "/toolkits/{id}/clone",
            post(handlers::clone_toolkit_handler),
        )
        .route("/toolkits/{id}/edit", get(handlers::edit_toolkit_page))
        .route("/toolkits/{id}", post(handlers::update_toolkit_handler))
        .route(
            "/toolkits/{id}/delete",
            post(handlers::delete_toolkit_handler),
        )
        // Tool routes
        .route(
            "/toolkits/{toolkit_id}/tools/new",
            get(handlers::create_tool_page),
        )
        .route(
            "/toolkits/{toolkit_id}/tools",
            post(handlers::create_tool_handler),
        )
        .route(
            "/toolkits/{toolkit_id}/tools/{tool_id}",
            get(handlers::view_tool_handler),
        )
        .route(
            "/toolkits/{toolkit_id}/tools/{tool_id}/edit",
            get(handlers::edit_tool_page),
        )
        .route(
            "/toolkits/{toolkit_id}/tools/{tool_id}",
            post(handlers::update_tool_handler),
        )
        .route(
            "/toolkits/{toolkit_id}/tools/{tool_id}/delete",
            post(handlers::delete_tool_handler),
        )
        .route(
            "/toolkits/{toolkit_id}/tools/{tool_id}/test",
            get(handlers::test_tool_page),
        )
        .route(
            "/toolkits/{toolkit_id}/tools/{tool_id}/test",
            post(handlers::test_tool_execute),
        )
        // Server routes
        .route("/servers", get(handlers::list_servers_page))
        .route("/servers/new", get(handlers::create_server_page))
        .route("/servers", post(handlers::create_server_handler))
        .route("/servers/{id}", get(handlers::view_server_handler))
        .route("/servers/{id}/edit", get(handlers::edit_server_page))
        .route("/servers/{id}", post(handlers::update_server_handler))
        .route(
            "/servers/{id}/delete",
            post(handlers::delete_server_handler),
        )
        .route(
            "/servers/{id}/bindings",
            post(handlers::save_bindings_handler),
        )
        .route(
            "/servers/{id}/access",
            post(handlers::update_server_access_handler),
        )
        .route(
            "/servers/{id}/install-toolkit",
            post(handlers::install_toolkit_handler),
        )
        // Instance routes
        .route(
            "/servers/{id}/instances/new",
            get(handlers::configure_instance_page),
        )
        .route(
            "/servers/{id}/instances",
            post(handlers::create_instance_handler),
        )
        .route(
            "/servers/{id}/instances/{instance_id}",
            get(handlers::edit_instance_page),
        )
        .route(
            "/servers/{id}/instances/{instance_id}",
            post(handlers::update_instance_handler),
        )
        .route(
            "/servers/{id}/instances/{instance_id}/delete",
            post(handlers::delete_instance_handler),
        )
        .route(
            "/servers/{id}/instances/{instance_id}/test",
            get(handlers::test_instance_page),
        )
        .route(
            "/servers/{id}/instances/{instance_id}/test",
            post(handlers::test_instance_execute),
        )
        .layer(middleware::from_fn(auth::middleware::require_auth));

    // MCP routers will be merged after the main app is built with state
    // Each SSE router comes with its paths pre-configured (/s/{uuid})

    // Discovery routes (no auth) - state added at the end
    let discovery_routes = Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(handlers::authorization_server_metadata).options(handlers::options_handler),
        )
        .route(
            "/.well-known/mcp-servers",
            get(handlers::mcp_servers_discovery).options(handlers::options_handler),
        )
        .route(
            "/.well-known/oauth-protected-resource/s/{uuid}",
            get(handlers::oauth_protected_resource_metadata).options(handlers::options_handler),
        )
        .route(
            "/.well-known/oauth-protected-resource/s/{uuid}/sse",
            get(handlers::oauth_protected_resource_metadata).options(handlers::options_handler),
        )
        // Subdomain discovery routes (for {uuid}.saramcp.com/.well-known/...)
        .route(
            "/.well-known/oauth-protected-resource",
            get(handlers::oauth_protected_resource_metadata_subdomain)
                .options(handlers::options_handler),
        );

    // Build main app with state first
    let mut app = Router::new()
        // Discovery routes
        .merge(discovery_routes)
        // Public routes
        .route("/", get(index_handler).options(root_options_handler))
        .route("/tutorial", get(handlers::tutorial_handler))
        .route(
            "/contact",
            get(handlers::show_contact_form).post(handlers::submit_contact_form),
        )
        .route(
            "/auth",
            post(handlers::unified_auth_handlers::unified_auth_handler),
        )
        .route(
            "/auth/verify/{token}",
            get(handlers::unified_auth_handlers::verify_token_handler),
        )
        .route(
            "/auth/magic/{token}",
            get(handlers::unified_auth_handlers::magic_login_handler),
        )
        .route(
            "/logout",
            get(handlers::unified_auth_handlers::logout_handler),
        )
        // Dedicated login page for OAuth flow compatibility
        .route("/login", get(login_page_handler).post(login_post_handler))
        .route("/signup", get(|| async { Redirect::to("/") }))
        // OAuth endpoints (handle their own authentication)
        .route(
            "/.oauth/register",
            post(handlers::register_client).options(handlers::options_handler),
        )
        .route(
            "/.oauth/authorize",
            get(handlers::authorize).post(handlers::authorize_consent),
        )
        .route(
            "/.oauth/token",
            post(handlers::token).options(handlers::options_handler),
        )
        // Protected routes
        .merge(protected_routes)
        // Static files
        .nest_service("/static", ServeDir::new("static"))
        // Layers
        .layer(session_layer)
        .layer(middleware::from_fn(add_security_headers))
        .layer(TraceLayer::new_for_http())
        .with_state(app_state.clone());

    // Create CORS layer for MCP endpoints (must be created before applying to routes)
    // Note: Authorization header must be explicitly listed (not covered by wildcard)
    // MCP-specific headers like mcp-protocol-version must also be included
    use axum::http::HeaderName;
    let cors_layer = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            axum::http::header::ACCEPT,
            axum::http::header::CACHE_CONTROL,
            axum::http::header::USER_AGENT,
            HeaderName::from_static("mcp-protocol-version"),
        ])
        .max_age(std::time::Duration::from_secs(3600));

    // Add Streamable HTTP routes (before merging SSE routers)
    // These handle POST requests to /s/{uuid} for simple request/response
    let http_routes = Router::new()
        .route(
            "/s/{uuid}",
            post(mcp::http_transport::handle_streamable_http)
                .options(mcp::http_transport::handle_streamable_http_options),
        )
        .with_state(mcp_registry.clone())
        .layer(middleware::from_fn_with_state(
            app_state.clone(),
            saramcp::middleware::mcp_auth_middleware,
        ))
        .layer(cors_layer.clone());

    app = app.merge(http_routes);

    tracing::info!("Registered HTTP transport routes at /s/{{uuid}}");

    // Dynamic subdomain SSE routing (doxyde pattern)
    // POST /message for JSON-RPC messages at subdomain root
    // Note: GET / is handled by index_handler which detects subdomains and delegates
    let message_root_routes = Router::new()
        .route(
            "/message",
            axum::routing::post(handlers::root_message_handler),
        )
        .with_state(mcp_registry.clone())
        .layer(middleware::from_fn_with_state(
            app_state.clone(),
            saramcp::middleware::mcp_auth_middleware_subdomain,
        ))
        .layer(cors_layer.clone());

    app = app.merge(message_root_routes);

    tracing::info!("Registered subdomain MCP handler: POST /message (JSON-RPC)");
    tracing::info!("Note: GET / subdomain SSE handled by index_handler with delegation");

    let app = app;

    // Start server
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()?;

    let addr = SocketAddr::from((host.parse::<std::net::IpAddr>()?, port));

    tracing::info!("Server running on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn add_security_headers(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "X-XSS-Protection",
        HeaderValue::from_static("1; mode=block"),
    );
    headers.insert(
        "Content-Security-Policy",
        HeaderValue::from_static(
            "default-src 'self'; \
             script-src 'self' 'unsafe-inline'; \
             style-src 'self' 'unsafe-inline'; \
             img-src 'self' data: https:; \
             connect-src 'self'; \
             frame-ancestors 'none';",
        ),
    );
    headers.insert(
        "Referrer-Policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );

    if std::env::var("ENVIRONMENT")
        .map(|env| env == "production")
        .unwrap_or(false)
    {
        headers.insert(
            "Strict-Transport-Security",
            HeaderValue::from_static("max-age=31536000; includeSubDomains; preload"),
        );
    }

    response
}

/// Dedicated login page for OAuth flow compatibility
/// This handler works on both main domain and subdomains
async fn login_page_handler(
    session: Session,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    use askama::Template;

    #[derive(Template)]
    #[template(path = "auth/oauth_login.html")]
    struct OAuthLoginTemplate {
        error: Option<String>,
        csrf_token: String,
    }

    // Store return_to in session for after login (used by unified_auth_handler)
    if let Some(return_to) = params.get("return_to") {
        let _ = session.insert("oauth_return_to", return_to.as_str()).await;
    }

    let csrf_token = saramcp::middleware::csrf::get_or_create_csrf_token(&session)
        .await
        .unwrap_or_else(|_| String::from("error"));

    let template = OAuthLoginTemplate {
        error: None,
        csrf_token,
    };

    Html(template.render().unwrap_or_else(|_| {
        "<html><body><h1>Error rendering login page</h1></body></html>".to_string()
    }))
}

/// POST handler for login form - delegates to unified auth
async fn login_post_handler(
    State(state): State<AppState>,
    session: Session,
    form: axum::extract::Form<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    use askama::Template;

    #[derive(Template)]
    #[template(path = "auth/oauth_login.html")]
    struct OAuthLoginTemplate {
        error: Option<String>,
        csrf_token: String,
    }

    // Extract credentials
    let email = form.get("email").map(|s| s.as_str()).unwrap_or("");
    let password = form.get("password").map(|s| s.as_str()).unwrap_or("");

    if email.is_empty() {
        let csrf_token = saramcp::middleware::csrf::get_or_create_csrf_token(&session)
            .await
            .unwrap_or_else(|_| String::from("error"));
        let template = OAuthLoginTemplate {
            error: Some("Please enter your email address".to_string()),
            csrf_token,
        };
        return Html(
            template
                .render()
                .unwrap_or_else(|_| "<html><body><h1>Error</h1></body></html>".to_string()),
        )
        .into_response();
    }

    // Attempt authentication
    let request = services::auth_service::LoginRequest {
        email: email.to_string(),
        password: password.to_string(),
    };

    match state.auth_service.authenticate(request).await {
        Ok(user) => {
            // Set session
            if session.insert("user_id", user.id).await.is_err()
                || session.insert("email", user.email).await.is_err()
                || session
                    .insert("auth_timestamp", chrono::Utc::now().timestamp())
                    .await
                    .is_err()
            {
                let csrf_token = saramcp::middleware::csrf::get_or_create_csrf_token(&session)
                    .await
                    .unwrap_or_else(|_| String::from("error"));
                let template = OAuthLoginTemplate {
                    error: Some("Failed to create session".to_string()),
                    csrf_token,
                };
                return Html(
                    template
                        .render()
                        .unwrap_or_else(|_| "<html><body><h1>Error</h1></body></html>".to_string()),
                )
                .into_response();
            }

            // Check for OAuth return_to URL
            let redirect_url = match session.get::<String>("oauth_return_to").await {
                Ok(Some(return_to)) => {
                    // Clear the return_to from session
                    let _ = session.remove::<String>("oauth_return_to").await;
                    return_to
                }
                _ => "/dashboard".to_string(),
            };

            Redirect::to(&redirect_url).into_response()
        }
        Err(_) => {
            let csrf_token = saramcp::middleware::csrf::get_or_create_csrf_token(&session)
                .await
                .unwrap_or_else(|_| String::from("error"));
            let template = OAuthLoginTemplate {
                error: Some("Invalid email or password".to_string()),
                csrf_token,
            };
            Html(
                template
                    .render()
                    .unwrap_or_else(|_| "<html><body><h1>Error</h1></body></html>".to_string()),
            )
            .into_response()
        }
    }
}

/// OPTIONS handler for root path (CORS preflight)
async fn root_options_handler() -> impl IntoResponse {
    use axum::http::header;
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        header::HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        header::HeaderValue::from_static("Content-Type, Authorization"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        header::HeaderValue::from_static("3600"),
    );
    (StatusCode::NO_CONTENT, headers)
}

async fn index_handler(
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    session: Session,
    request: axum::extract::Request,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    use saramcp::models::server::Server;

    // Check if this is a subdomain MCP request
    if let Some(uuid) = saramcp::middleware::extract_server_uuid_from_headers(&headers) {
        // This is a subdomain request - perform auth check first
        // Get server to check access level
        let server = Server::get_by_uuid(&state.pool, &uuid)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .ok_or((StatusCode::NOT_FOUND, "Server not found".to_string()))?;

        // Apply three-tier access control
        match server.access_level.as_deref() {
            Some("public") | None => {
                // Public servers: no authentication required
            }
            Some("organization") | Some("private") => {
                // Require valid OAuth token
                let auth_header = headers
                    .get("authorization")
                    .ok_or((
                        StatusCode::UNAUTHORIZED,
                        "Missing authorization header".to_string(),
                    ))?
                    .to_str()
                    .map_err(|_| {
                        (
                            StatusCode::UNAUTHORIZED,
                            "Invalid authorization format".to_string(),
                        )
                    })?;

                if !auth_header.starts_with("Bearer ") {
                    return Err((
                        StatusCode::UNAUTHORIZED,
                        "Invalid authorization format".to_string(),
                    ));
                }

                let token = &auth_header["Bearer ".len()..];
                let validated = state
                    .oauth_service
                    .validate_access_token(token)
                    .await
                    .map_err(|e| {
                        (
                            StatusCode::UNAUTHORIZED,
                            format!("Token validation failed: {}", e),
                        )
                    })?;

                // For private servers, check ownership
                if server.access_level.as_deref() == Some("private") {
                    let can_access = state
                        .oauth_service
                        .can_access_server(&uuid, validated.user_id)
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                    if !can_access {
                        return Err((StatusCode::FORBIDDEN, "Forbidden".to_string()));
                    }
                }
            }
            _ => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Invalid access level".to_string(),
                ));
            }
        }

        // Auth passed - delegate to root_sse_handler
        let registry = state.mcp_registry.as_ref().ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Registry not available".to_string(),
        ))?;

        match handlers::root_sse_handler(headers, State(registry.clone()), request).await {
            Ok(mut response) => {
                // Add CORS headers to the SSE response
                use axum::http::header;
                let headers_mut = response.headers_mut();
                headers_mut.insert(
                    header::ACCESS_CONTROL_ALLOW_ORIGIN,
                    header::HeaderValue::from_static("*"),
                );
                headers_mut.insert(
                    header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                    header::HeaderValue::from_static("true"),
                );
                return Ok(response);
            }
            Err(status) => return Err((status, "MCP SSE request failed".to_string())),
        }
    }

    // Main domain request - show homepage or redirect to dashboard
    // Check if user is authenticated
    let user_id = session
        .get::<i64>("user_id")
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if user_id.is_some() {
        // User is logged in, redirect to dashboard
        Ok(Redirect::to("/dashboard").into_response())
    } else {
        // User not logged in, show landing page with unified auth form
        let csrf_token = saramcp::middleware::csrf::get_or_create_csrf_token(&session)
            .await
            .unwrap_or_else(|_| String::from("error"));
        let template = templates::IndexTemplate { csrf_token };
        let html = template
            .render()
            .unwrap_or_else(|_| "Template error".to_string());
        Ok(Html(html).into_response())
    }
}

async fn toolkits_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    session: tower_sessions::Session,
) -> Result<Html<String>, StatusCode> {
    // Check if user is logged in
    let user_id = match session.get::<i64>("user_id").await {
        Ok(Some(id)) => id,
        _ => return Err(StatusCode::UNAUTHORIZED),
    };

    let user_email = session
        .get::<String>("email")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    // Get user's toolkits
    let toolkit_summaries = if let Some(toolkit_service) = &state.toolkit_service {
        toolkit_service
            .list_toolkit_summaries(user_id)
            .await
            .unwrap_or_else(|_| vec![])
    } else {
        vec![]
    };

    // Convert to template format
    let toolkits: Vec<templates::Toolkit> = toolkit_summaries
        .into_iter()
        .map(|summary| templates::Toolkit {
            id: summary.id,
            title: summary.title,
            description: summary.description,
            tools_count: summary.tools_count,
        })
        .collect();

    let template = templates::ToolkitsTemplate {
        user_email,
        toolkits,
    };

    Ok(Html(
        template
            .render()
            .unwrap_or_else(|_| "Template error".to_string()),
    ))
}

use askama::Template;
use askama_web::WebTemplate;

pub mod templates {
    use super::*;

    #[derive(Template, WebTemplate)]
    #[template(path = "index.html")]
    pub struct IndexTemplate {
        pub csrf_token: String,
    }

    #[derive(Template, WebTemplate)]
    #[template(path = "toolkits.html")]
    pub struct ToolkitsTemplate {
        pub user_email: String,
        pub toolkits: Vec<Toolkit>,
    }

    pub struct ServerSummary {
        pub id: i64,
        pub name: String,
        pub description: Option<String>,
        pub toolkit_count: i64,
        pub instance_count: i64,
    }

    pub struct Toolkit {
        pub id: i64,
        pub title: String,
        pub description: String,
        pub tools_count: i32,
    }
}
