# SaraMCP - System Architecture Documentation

## Project Overview

SaraMCP is a Model Context Protocol (MCP) server management platform that allows users to:
- Create reusable toolkits of HTTP-based tools (Postman-like request templates)
- Configure MCP servers with tool instances
- Manage parameter bindings with typed variables
- Securely store secrets and server-wide configuration
- Execute MCP protocol operations with HTTP execution

**Current Status:**
- ✅ Backend & UI fully implemented
- ✅ Parameter binding system complete
- ✅ Secrets management operational
- ✅ MCP protocol fully implemented (JSON-RPC 2.0 via rmcp)
- ✅ HTTP & SSE transport layers operational
- ✅ OAuth 2.0 authentication with three-tier access control

---

## Data Model & Entity Relationships

### Core Entities

```
User (1) ──owns──> (N) Toolkit ──contains──> (N) Tool
                │
                └──owns──> (N) Server ──imports──> (N) Toolkit
                                    │
                                    ├──has──> (N) ServerGlobal (variables/secrets)
                                    │
                                    └──has──> (N) ToolInstance ──configures──> (1) Tool
                                                              │
                                                              └──has──> (N) InstanceParam
```

### Entity Details

#### 1. User
- `id`, `email`, `password_hash`, `email_verified`, `created_at`
- Authentication and ownership

#### 2. Toolkit
- `id`, `user_id`, `title`, `description`, `visibility` (private/public)
- Collections of related tools
- Can be shared across multiple servers via `server_toolkits` junction table

#### 3. Tool
- `id`, `toolkit_id`, `name`, `description`
- `method` (GET, POST, PUT, DELETE, PATCH)
- `url`, `headers`, `body` - HTTP request templates with parameter placeholders
- `timeout_ms`
- **Parameter Extraction**: Dynamically extracts parameters from templates using `{{type:name}}` syntax

#### 4. Server
- `id`, `uuid`, `user_id`, `name`, `description`
- Represents an MCP server instance
- Imports toolkits via many-to-many relationship
- Has server-wide globals (variables and secrets)

#### 5. ServerGlobal
- `id`, `server_id`, `key`, `value`, `is_secret`
- Server-wide configuration and secrets
- Secrets are encrypted with AES-256-GCM
- Used as default values in parameter resolution

#### 6. ToolInstance
- `id`, `server_id`, `tool_id`, `instance_name`, `description`
- Configured instance of a tool within a server
- Unique `instance_name` per server (becomes MCP function name)
- Links to Tool for template structure

#### 7. InstanceParam
- `id`, `instance_id`, `param_name`, `source`, `value`
- **Source types:**
  - `instance`: Fixed value (stored in `value` field)
  - `server`: Use server global default
  - `exposed`: LLM provides at execution time
- Parameter binding configuration

---

## Architecture & Code Organization

### 3-Layer Architecture

```
┌─────────────────────────────────────┐
│  Handlers (Web/CLI)                 │  ← Axum routes, request/response
│  - auth/handlers.rs                 │
│  - handlers/server_handlers.rs      │
│  - handlers/instance_handlers.rs    │
│  - handlers/toolkit_handlers.rs     │
│  - handlers/tool_handlers.rs        │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│  Services (Business Logic)          │  ← Core functionality
│  - ServerService                    │
│  - InstanceService                  │
│  - ToolkitService                   │
│  - ToolService                      │
│  - ParameterResolver                │
│  - SecretsManager                   │
│  - TypedVariableEngine              │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│  Models & Repositories              │  ← Data access
│  - models/server.rs                 │
│  - models/instance.rs               │
│  - models/tool.rs                   │
│  - models/toolkit.rs                │
│  - repositories/*                   │
└─────────────────────────────────────┘
```

### Key Service Components

#### ServerService (src/services/server_service.rs)
- Server CRUD operations
- Server toolkit management (import/remove)
- Server globals management (variables & secrets)
- Encryption/decryption of secrets

#### InstanceService (src/services/instance_service.rs)
- Tool instance CRUD
- Parameter configuration management
- Available tools discovery
- Instance signature generation
- Parameter usage analysis across instances

#### ParameterResolver (src/services/parameter_resolver.rs)
- 3-tier parameter resolution (instance → server → exposed)
- Variable substitution in values
- Type casting and validation
- Decrypts secrets during resolution

#### SecretsManager (src/services/secrets_manager.rs)
- AES-256-GCM encryption/decryption
- Master key: `SARAMCP_MASTER_KEY` environment variable
- Nonce-based encryption (different ciphertext each time)

#### TypedVariableEngine (src/services/variable_engine.rs)
- Template string substitution
- Variable pattern: `{{type:name}}` or `{{name}}`
- Type casting: string, number, integer, boolean, json, url
- Validation and error reporting

#### HttpExecutor (src/services/http_executor.rs)
- HTTP client for executing tool requests with reqwest
- Template rendering with resolved parameters
- Request/response handling with headers and body
- Configurable timeouts per tool
- cURL command generation for debugging

#### InstanceExecutor (src/services/instance_executor.rs)
- Runtime execution engine for MCP tool instances
- Orchestrates parameter resolution + HTTP execution
- Execution tracking and logging
- Returns MCP CallToolResult (success/error)

#### SchemaGenerator (src/services/schema_generator.rs)
- Generates JSON Schema for MCP tool definitions
- Maps typed parameters to JSON Schema types
- Extracts exposed parameters from instance configuration

---

## MCP Runtime Architecture

### Overview

SaraMCP implements the Model Context Protocol (MCP) using a dynamic server registry pattern. Each configured server becomes an independent MCP endpoint accessible via HTTP or SSE.

### Server Lifecycle

```
1. Application Startup
   ├─> McpServerRegistry.load_all_servers()
   │   └─> For each Server in database with UUID:
   │       ├─> Create SaraMcpService (load instances, build tool router)
   │       ├─> Create McpServerInstance (wrap SSE server + service)
   │       └─> Register instance in registry by UUID

2. Request Routing
   ├─> HTTP: POST /s/{uuid}
   │   ├─> mcp_auth_middleware (check access level)
   │   ├─> Registry.get_instance(uuid)
   │   ├─> Service.handle_request(json_rpc)
   │   └─> Return JSON-RPC response
   │
   └─> SSE: GET /s/{uuid}/sse
       ├─> mcp_auth_middleware_sse (check access level)
       └─> Route to instance SSE router

3. Tool Execution
   ├─> tools/call JSON-RPC request
   ├─> InstanceExecutor.execute(llm_params)
   │   ├─> ParameterResolver.resolve_parameters()
   │   │   └─> Merge instance/server/exposed params
   │   ├─> HttpExecutor.execute_tool(tool, resolved_params)
   │   │   └─> Send HTTP request to external API
   │   └─> ExecutionTracker.record_execution()
   └─> Return CallToolResult

4. Hot Reload (Dynamic Tool Updates)
   ├─> Web handler modifies instance (create/update/delete)
   ├─> Handler calls reload_server_tools()
   ├─> Registry.reload_tools(uuid)
   │   └─> McpServerInstance.reload_tools()
   │       └─> SaraMcpService.reload_tools()
   │           ├─> Build new ToolRouter from database
   │           ├─> Acquire write lock on tool_router
   │           └─> Swap router (atomic operation)
   └─> New tools immediately available (no restart needed!)
```

### Hot-Reload Architecture

The hot-reload system uses **interior mutability** to allow dynamic tool updates:

**Design Pattern:**
- `SaraMcpService.tool_router` is wrapped in `Arc<RwLock<ToolRouter>>`
- **Read operations** (tool calls, tools/list): Acquire read lock with `.read().await`
- **Write operations** (reload): Acquire write lock with `.write().await`
- Multiple readers can access simultaneously (read-heavy optimization)
- Writes are rare and brief (just swapping the router)

**Concurrency Guarantees:**
- No race conditions between tool execution and reload
- No deadlocks (RwLock is fair and async-aware)
- Existing tool calls complete successfully during reload
- New tool calls see updated router immediately after reload

**Performance:**
- Zero-copy cloning via `Arc` (cheap service clones)
- Read locks are non-blocking for concurrent readers
- Write locks only held briefly during router swap

### Transport Options

#### 1. Streamable HTTP (Primary)
- **Endpoint**: `POST /s/{uuid}`
- **Protocol**: Single request/response JSON-RPC
- **CORS**: Enabled for cross-origin access
- **Auth**: Bearer token in Authorization header
- **Use case**: Simple tool calls, Claude SDK integration

#### 2. SSE (Server-Sent Events)
- **Endpoint**: `GET /s/{uuid}/sse`
- **Protocol**: Long-lived connection with event stream
- **Framework**: rmcp SseServer with ToolRouter
- **Auth**: Bearer token in Authorization header
- **Use case**: Real-time updates, streaming responses

### Three-Tier Access Control

Servers can be configured with different access levels:

1. **public** - No authentication required (default for demos)
2. **organization** - Requires valid OAuth token (any authenticated user)
3. **private** - Requires valid OAuth token + ownership check

The `mcp_auth_middleware` enforces these policies before routing requests.

### Discovery Endpoints

#### `/.well-known/mcp-servers`
Lists all accessible MCP servers for the authenticated user (or public servers).

**Response:**
```json
{
  "servers": [
    {
      "name": "Demo Server",
      "description": "Currency conversion tools",
      "base_url": "http://localhost:8080/s/550e8400-...",
      "access_level": "public"
    }
  ]
}
```

#### `/.well-known/oauth-authorization-server`
OAuth 2.0 Authorization Server Metadata for client registration and token endpoints.

#### `/.well-known/oauth-protected-resource/s/{uuid}`
Per-server OAuth resource metadata including supported scopes and token endpoints.

---

## Parameter System

### Template Syntax

Parameters in Tool templates use double-brace syntax with optional type prefix:

```
{{name}}              → string (default)
{{string:api_key}}    → string
{{integer:timeout}}   → integer
{{number:rate}}       → number/float
{{boolean:debug}}     → boolean
{{json:config}}       → JSON object/array
{{url:endpoint}}      → URL (validated)
```

**Example Tool URL:**
```
https://api.example.com/{{url:base}}/users/{{integer:user_id}}?debug={{boolean:debug}}
```

### Parameter Sources (3-Tier Resolution)

When a tool instance is executed, parameters are resolved in this priority:

1. **instance** (highest priority)
   - Fixed value configured at instance creation
   - Stored in `instance_params.value`
   - Can contain variables that get substituted from server globals
   - Example: `value = "https://{{base_url}}/api"` → resolved using server globals

2. **server** (medium priority)
   - Uses value from `server_globals` table
   - Server-wide defaults
   - Can be plain values or encrypted secrets

3. **exposed** (lowest priority)
   - LLM provides value at execution time
   - These become function parameters in MCP tool definitions
   - Not stored anywhere, purely runtime

### Parameter Resolution Flow

```rust
// Example from ParameterResolver::resolve_parameters()

For parameter "api_key":
1. Check instance_params where param_name='api_key' and source='instance'
   → If found, substitute variables and return

2. Check instance_params where param_name='api_key' and source='server'
   → Look up in server_globals, decrypt if secret, return

3. Check instance_params where param_name='api_key' and source='exposed'
   → Use value from LLM-provided HashMap (execution time)

4. If no config found → parameter is missing
```

### Variable Substitution

Instance-level values can reference server globals:

```
Server Globals:
  api_host = "api.example.com"
  api_version = "v2"

Instance Param (source=instance):
  base_url = "https://{{api_host}}/{{api_version}}"

Resolved:
  base_url = "https://api.example.com/v2"
```

### Type Casting

The `VariableType` enum handles casting:

```rust
"8080"        → integer → JSON: 8080
"true"        → boolean → JSON: true
"3.14"        → number  → JSON: 3.14
'{"foo":"bar"}' → json → JSON: {"foo":"bar"}
"hello"       → string  → JSON: "hello"
```

---

## Secrets Management

### Encryption Details

- **Algorithm**: AES-256-GCM (authenticated encryption)
- **Key Size**: 32 bytes (256 bits)
- **Nonce**: 12 bytes (96 bits), randomly generated per encryption
- **Storage Format**: Base64(nonce || ciphertext)

### Master Key Configuration

Set via environment variable:
```bash
export SARAMCP_MASTER_KEY="<base64-encoded-32-bytes>"
```

Generate a new key:
```rust
SecretsManager::generate_master_key()
```

⚠️ **Development Mode**: If not set, uses insecure default key with warning

### Storage

Secrets stored in `server_globals` table:
- `is_secret = true` → value is encrypted
- `is_secret = false` → value is plaintext

Decryption happens:
- When displaying in UI (`get_server_globals_decrypted()`)
- During parameter resolution (`ParameterResolver::load_globals()`)

---

## Technology Stack

### Core Framework
- **Rust 2021 Edition**
- **Axum 0.7** - Web framework
- **Tokio** - Async runtime
- **SQLx 0.7** - Database (SQLite)
- **Tower** - Middleware layers
- **rmcp** - Model Context Protocol implementation
- **reqwest** - HTTP client for tool execution

### Templating & Serialization
- **Askama** - HTML templates
- **Serde** - JSON serialization
- **Regex** - Parameter extraction

### Security
- **Argon2** - Password hashing
- **AES-GCM** - Secret encryption
- **Tower Sessions** - Session management

### Database
- **SQLite** with SQLx compile-time query checking
- **Migrations** in `migrations/` directory
- **Connection Pooling** via SqlitePool

### Testing
- **sqlx::test** - Database-backed tests
- **mockall** - Mocking (dev dependency)
- **tempfile** - Temporary test databases

---

## Implementation Status

### ✅ Completed

1. **Data Layer**
   - Complete database schema with migrations
   - All model structs with database queries
   - Repository pattern for data access
   - OAuth client registration and token management tables

2. **Service Layer**
   - ServerService, InstanceService, ToolkitService, ToolService
   - ParameterResolver with 3-tier resolution
   - SecretsManager with AES-GCM encryption
   - TypedVariableEngine for variable substitution
   - HttpExecutor for HTTP request execution
   - InstanceExecutor for MCP tool execution
   - SchemaGenerator for MCP tool schemas
   - OAuthService for OAuth 2.0 flows
   - ExecutionTracker for logging tool executions

3. **Web Layer**
   - Full CRUD web interface for all entities
   - Authentication & session management
   - Tabbed UI for servers (Toolkits, Bindings, Instances)
   - Instance configuration UI with parameter source selection
   - Tool testing interface with live execution

4. **Parameter System**
   - Template parsing with `{{type:name}}` syntax
   - Dynamic parameter extraction from tool templates
   - Parameter binding configuration (instance/server/exposed)
   - Variable substitution in instance values
   - Type casting and validation

5. **Secrets Management**
   - Encryption/decryption of sensitive values
   - Decrypted display in UI
   - Secure parameter resolution

6. **MCP Protocol Implementation**
   - JSON-RPC 2.0 message handling via rmcp library
   - MCP method handlers (initialize, tools/list, tools/call)
   - SaraMcpService implementing ServerHandler trait
   - Dynamic tool registration with ToolRouter

7. **Transport Layers**
   - Streamable HTTP transport (POST /s/{uuid})
   - SSE transport (GET /s/{uuid}/sse) via rmcp SseServer
   - CORS support for cross-origin requests
   - Both transports fully operational

8. **Server Runtime**
   - McpServerRegistry for lifecycle management
   - **Hot-reload capability**: Dynamic tool updates without server restart
     - Uses `Arc<RwLock<ToolRouter>>` for interior mutability
     - Thread-safe concurrent access (many readers, rare writers)
     - Supports create/update/delete of tool instances in real-time
     - Comprehensive test coverage for concurrent operations
   - McpServerInstance wrapping SSE server and service
   - Graceful shutdown with cancellation tokens
   - Automatic server loading on application startup

9. **HTTP Execution Engine**
   - HttpExecutor with reqwest client
   - Template rendering with resolved parameters
   - Request/response handling with headers and body
   - Configurable timeouts per tool
   - cURL command generation for debugging

10. **MCP Tool Definition Generation**
    - SchemaGenerator creates JSON Schema from instances
    - Maps instance signatures to MCP tool parameters
    - Type mapping (SaraMCP types → JSON Schema types)
    - Only exposed parameters appear in MCP tool signatures

11. **OAuth 2.0 Authentication**
    - Three-tier access control (public/organization/private)
    - OAuth 2.0 authorization code flow
    - Dynamic client registration
    - Access token validation with Bearer token
    - Per-server access control enforcement
    - Authorization Server Metadata (.well-known endpoints)

12. **MCP Discovery & Integration**
    - /.well-known/mcp-servers endpoint for server discovery
    - /.well-known/oauth-protected-resource per-server metadata
    - /.well-known/oauth-authorization-server metadata
    - Ready for Claude Desktop/SDK integration

### ❌ Missing / To Implement

1. **Execution History UI**
   - Web interface to view execution logs
   - Filtering and search capabilities
   - Export to CSV/JSON

2. **Rate Limiting**
   - Per-user rate limits
   - Per-server rate limits
   - Token bucket algorithm

3. **Webhook Support**
   - POST results to external URLs
   - Retry logic for failed webhooks
   - Webhook configuration UI

4. **Advanced Features**
   - Tool composition/chaining
   - Conditional execution
   - Response transformations
   - Mock responses for testing

---

## Code Location Reference

### Models (Data Structures)
- `src/models/user.rs` - User authentication
- `src/models/toolkit.rs` - Toolkit entity
- `src/models/tool.rs` - Tool templates, parameter extraction
- `src/models/server.rs` - Server entity, summaries
- `src/models/server_global.rs` - Server variables/secrets
- `src/models/instance.rs` - Tool instances, instance parameters

### Services (Business Logic)
- `src/services/auth_service.rs` - Authentication
- `src/services/user_service.rs` - User management
- `src/services/toolkit_service.rs` - Toolkit operations
- `src/services/tool_service.rs` - Tool operations
- `src/services/server_service.rs` - Server management
- `src/services/instance_service.rs` - Instance configuration
- `src/services/parameter_resolver.rs` - **Parameter resolution logic**
- `src/services/secrets_manager.rs` - **Encryption/decryption**
- `src/services/variable_engine.rs` - **Variable substitution**
- `src/services/http_executor.rs` - **HTTP request execution**
- `src/services/instance_executor.rs` - **MCP tool execution orchestration**
- `src/services/schema_generator.rs` - **JSON Schema generation for MCP**
- `src/services/oauth_service.rs` - OAuth 2.0 flows and token validation
- `src/services/execution_tracker.rs` - Tool execution logging

### MCP Protocol Layer
- `src/mcp/mod.rs` - MCP module entry point
- `src/mcp/service.rs` - **SaraMcpService (MCP protocol handler)**
- `src/mcp/registry.rs` - **McpServerRegistry (lifecycle management)**
- `src/mcp/instance.rs` - McpServerInstance (per-server wrapper)
- `src/mcp/handlers.rs` - HTTP request handlers for MCP endpoints
- `src/mcp/http_transport.rs` - **Streamable HTTP transport**

### Middleware
- `src/middleware/mod.rs` - Middleware exports
- `src/middleware/mcp_auth.rs` - **OAuth authentication for MCP endpoints**

### Handlers (Web Endpoints)
- `src/auth/handlers.rs` - Login/signup/logout
- `src/handlers/toolkit_handlers.rs` - Toolkit CRUD
- `src/handlers/tool_handlers.rs` - Tool CRUD
- `src/handlers/server_handlers.rs` - Server CRUD, bindings, discovery endpoints
- `src/handlers/instance_handlers.rs` - Instance configuration
- `src/handlers/oauth_handlers.rs` - OAuth 2.0 endpoints (register, authorize, token)

### Database
- `migrations/` - All database migrations
- `src/db.rs` - Database connection setup

### Configuration
- `.env` - Environment variables (DATABASE_URL, SESSION_SECRET)
- `Cargo.toml` - Dependencies and project config

---

# Coding Guidelines

## Development Standards

### Code Quality
- Run `cargo clippy -- -D warnings` before committing
- Format with `cargo fmt`
- Maximum function length: 60 lines
- Use `Result<T, E>` for error handling
- Document all public APIs

### Database Conventions
- Use lowercase with underscores for all identifiers
- Add indexes for foreign keys and frequently queried columns
- Always include created_at and updated_at timestamps

## Testing Strategy

All code must maintain:
- 100% test coverage
- Zero clippy warnings
- Functions limited to 60 lines
- Clear error handling with custom error types
- Integration tests for all API endpoints
- Unit tests for all business logic

## Development Process Rules

**MANDATORY: One Function at a Time**

- ✅ Incremental development strategy
    1. Write ONE function
    2. Write tests for that function
    3. Run tests and ensure they pass
    4. ONLY then move to the next function
- ✅ DRY, KISS principles
- ✅ Idiomatic rust code
- ✅ Unit testing everywhere possible

Each function should be treated as a unit of development.

**FORBIDDEN:**
- ❌ Writing multiple functions before testing
- ❌ Using `unwrap()` or `expect()` anywhere
- ❌ Functions longer than 60 lines
- ❌ Nesting more than 3 levels deep
- ❌ More than 4 parameters per function
- ❌ Skipping tests "for now"
- ❌ NEVER edit existing migrations if they have been committed

## Deployment

### Automatic Deployment via GitHub Webhook

SaraMCP uses automatic deployment triggered by git pushes:

```bash
# 1. Build locally to verify (optional)
cargo build --release

# 2. Commit and push changes
git add -A
git commit -m "Description of changes"
git push origin master
```

The push triggers a GitHub webhook that:
1. Pulls latest code on the atlantic server at `/opt/saramcp`
2. Rebuilds the Docker image (5-10 minute Rust compilation)
3. Restarts the `saramcp` Docker container
4. Service is available at `https://saramcp.com`

### Manual Deployment Verification

To check deployment status:

```bash
# Check container status
ssh root@atlantic "docker ps | grep saramcp"

# View recent logs
ssh root@atlantic "docker logs --tail 50 saramcp"

# Test the service
curl https://saramcp.com/
```

### Production Database

The SQLite database is stored at `/opt/saramcp-data/saramcp.db` on the atlantic server (Docker volume mount).

---

## Task Execution Strategy

Follow this 5-step process for all feature implementation:

### Step 1 - ANALYSIS
Use a sub-agent to run an analysis phase - analyse what exists in the codebase. We want to reuse as much code as possible.

### Step 2 - PLANNING
Make an implementation plan. Ultrathink hard at the planning phase.

### Step 3 - IMPLEMENTATION
Use a sub-agent to write the implementation. Make sure it separates channel-specific code (e.g., web/api/cli layer) from actual logic (service layer). The sub-agent should only exit when the code compiles.

### Step 4 - TESTS
Write unit tests in separate test files (separate production/test code). The test files should use their own isolated test databases and environments.

### Step 5 - CLEANUP
Code formatting, no warnings (should be fixed by understanding what's going on, not just disabling the warnings).
