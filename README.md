# SaraMCP

**A powerful Model Context Protocol (MCP) server management platform for building, configuring, and deploying HTTP-based AI tools.**

SaraMCP enables you to create reusable toolkits of HTTP API tools, configure them with flexible parameter bindings, and expose them as MCP servers that work seamlessly with Claude Desktop and other MCP clients.

## Features

### Core Capabilities

- **Reusable Toolkits** - Create collections of HTTP-based tools (similar to Postman collections) that can be shared across multiple MCP servers
- **Flexible Parameter System** - Configure parameters with three-tier resolution:
  - **Instance-level**: Fixed values with variable substitution
  - **Server-level**: Shared defaults and encrypted secrets
  - **Exposed**: Dynamic parameters provided by LLMs at runtime
- **Type Safety** - Strongly-typed parameter system with validation (string, number, integer, boolean, json, url)
- **Secrets Management** - AES-256-GCM encryption for API keys and sensitive configuration
- **Hot Reload** - Update tool configurations without server restarts
- **OAuth 2.0 Integration** - Three-tier access control (public/organization/private)
- **MCP Protocol** - Full JSON-RPC 2.0 implementation with HTTP and SSE transports

### Developer Experience

- **Web UI** - Complete web interface for managing servers, toolkits, and tool instances
- **CLI Tools** - Command-line utilities for automation and scripting
- **Execution Tracking** - Built-in logging and debugging capabilities
- **Auto-discovery** - Standard `.well-known` endpoints for MCP server discovery
- **Docker Support** - Production-ready containerization with docker-compose

## Quick Start

### Prerequisites

- Rust 1.75+ (for building from source)
- SQLite 3
- Docker & Docker Compose (for containerized deployment)

### Installation

#### Option 1: Docker (Recommended)

```bash
# Clone the repository
git clone https://github.com/yourusername/saramcp.git
cd saramcp

# Copy environment template
cp .env.example .env

# Edit .env and set your configuration
# Minimal required: DATABASE_URL, SESSION_SECRET, SARAMCP_MASTER_KEY

# Start the server
docker-compose up -d
```

The server will be available at `http://localhost:8080`

#### Option 2: Build from Source

```bash
# Clone the repository
git clone https://github.com/yourusername/saramcp.git
cd saramcp

# Create data directory
mkdir -p data

# Copy environment template
cp .env.example .env

# Run database migrations
sqlx migrate run

# Build and run
cargo build --release
./target/release/saramcp
```

### First Steps

1. **Create an Account**
   - Navigate to `http://localhost:8080/signup`
   - Create your first user account

2. **Create a Toolkit**
   - Go to "Toolkits" and click "Create New Toolkit"
   - Add HTTP tools with parameter templates like `https://api.example.com/users/{{integer:user_id}}`

3. **Create a Server**
   - Go to "Servers" and click "Create New Server"
   - Import your toolkit
   - Configure tool instances with parameter bindings

4. **Use with Claude Desktop**
   - Copy your server's MCP endpoint: `http://localhost:8080/s/{server-uuid}`
   - Add to Claude Desktop's MCP configuration
   - Start using your tools in conversations!

## Configuration

### Environment Variables

Create a `.env` file with the following variables:

```bash
# Database
DATABASE_URL=sqlite://data/saramcp.db

# Security (generate random 64-character strings)
SESSION_SECRET=your_64_character_random_string_here
SARAMCP_MASTER_KEY=your_base64_encoded_32_byte_key_here

# Server
HOST=127.0.0.1
PORT=8080
RUST_LOG=info

# Email (optional - for password reset)
SMTP_HOST=smtp.example.com
SMTP_PORT=587
SMTP_USERNAME=noreply@example.com
SMTP_PASSWORD=your_smtp_password
SMTP_FROM_EMAIL=noreply@example.com
SMTP_FROM_NAME=SaraMCP
SMTP_ENCRYPTION=starttls
```

### Generating Secrets

```bash
# Generate SESSION_SECRET
openssl rand -hex 32

# Generate SARAMCP_MASTER_KEY
openssl rand -base64 32
```

## Usage Examples

### Example 1: Currency Converter Tool

**Tool Definition:**
```
Name: get_exchange_rate
URL: https://api.exchangerate-api.com/v4/latest/{{string:base_currency}}
Method: GET
```

**Tool Instance Configuration:**
```
Instance Name: convert_usd_to_eur
Parameters:
  - base_currency: "USD" (instance-level, fixed)
```

When Claude calls `convert_usd_to_eur`, it automatically fetches USD exchange rates.

### Example 2: Authenticated API with Secrets

**Tool Definition:**
```
Name: create_issue
URL: https://api.github.com/repos/{{string:repo}}/issues
Method: POST
Headers:
  Authorization: Bearer {{string:github_token}}
Body: {"title": "{{string:title}}", "body": "{{string:description}}"}
```

**Server Configuration:**
```
Server Globals (encrypted):
  - github_token: ghp_your_secret_token_here (secret)
```

**Tool Instance:**
```
Instance Name: create_project_issue
Parameters:
  - repo: "myorg/myproject" (instance-level)
  - github_token: (server-level, uses encrypted secret)
  - title: (exposed, LLM provides)
  - description: (exposed, LLM provides)
```

Claude can now create GitHub issues by providing just the title and description!

## Architecture

SaraMCP uses a clean 3-layer architecture:

```
┌─────────────────────────────────────┐
│  Web Handlers (Axum Routes)        │  ← HTTP endpoints
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│  Services (Business Logic)          │  ← Core functionality
│  - Parameter Resolution             │
│  - Secrets Management               │
│  - HTTP Execution                   │
│  - MCP Protocol                     │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│  Models & Repositories              │  ← Data access
└─────────────────────────────────────┘
```

For detailed architecture documentation, see [CLAUDE.md](./CLAUDE.md).

## Development

### Setup Development Environment

```bash
# Install Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install sqlx-cli for database management
cargo install sqlx-cli --no-default-features --features sqlite

# Clone and setup
git clone https://github.com/yourusername/saramcp.git
cd saramcp
cp .env.example .env
mkdir -p data

# Run migrations
sqlx database create
sqlx migrate run

# Start development server
cargo run
```

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_parameter_resolution

# Run with coverage (requires tarpaulin)
cargo tarpaulin --out Html
```

### Code Quality

```bash
# Format code
cargo fmt

# Run linter
cargo clippy -- -D warnings

# Check without building
cargo check
```

## Project Structure

```
saramcp/
├── src/
│   ├── main.rs                 # Application entry point
│   ├── lib.rs                  # Library exports
│   ├── models/                 # Data models
│   ├── services/               # Business logic
│   │   ├── parameter_resolver.rs
│   │   ├── secrets_manager.rs
│   │   ├── http_executor.rs
│   │   └── instance_executor.rs
│   ├── mcp/                    # MCP protocol implementation
│   │   ├── service.rs          # MCP handler
│   │   ├── registry.rs         # Server lifecycle
│   │   └── handlers.rs         # HTTP endpoints
│   ├── handlers/               # Web request handlers
│   ├── middleware/             # Authentication & middleware
│   └── bin/
│       └── cli.rs             # CLI tools
├── migrations/                 # Database migrations
├── templates/                  # Askama HTML templates
├── static/                     # CSS, JS, images
├── tests/                      # Integration tests
├── Dockerfile
├── docker-compose.yml
└── CLAUDE.md                   # Detailed system design
```

## API Endpoints

### MCP Protocol Endpoints

- `POST /s/{uuid}` - MCP JSON-RPC HTTP transport
- `GET /s/{uuid}/sse` - MCP Server-Sent Events transport
- `GET /.well-known/mcp-servers` - MCP server discovery
- `GET /.well-known/oauth-authorization-server` - OAuth metadata
- `GET /.well-known/oauth-protected-resource/s/{uuid}` - Per-server OAuth metadata

### Web Interface

- `/` - Dashboard
- `/login` - Authentication
- `/toolkits` - Toolkit management
- `/servers` - Server management
- `/servers/{id}/instances` - Tool instance configuration

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](./CONTRIBUTING.md) for guidelines.

### Development Workflow

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes with tests
4. Ensure all tests pass (`cargo test`)
5. Run code quality checks (`cargo clippy -- -D warnings && cargo fmt`)
6. Commit your changes (`git commit -m 'Add amazing feature'`)
7. Push to the branch (`git push origin feature/amazing-feature`)
8. Open a Pull Request

## Security

### Reporting Vulnerabilities

If you discover a security vulnerability, please email security@yourdomain.com. Do not open a public issue.

### Security Features

- **Password Hashing**: Argon2id with secure defaults
- **Session Management**: Signed, encrypted sessions with SQLite storage
- **Secret Encryption**: AES-256-GCM for API keys and sensitive data
- **CSRF Protection**: Token-based CSRF protection on all forms
- **OAuth 2.0**: Industry-standard authorization with three-tier access control
- **Input Validation**: Type-safe parameter validation and sanitization

## License

This project is licensed under the MIT License - see the [LICENSE](./LICENSE) file for details.

## Acknowledgments

- Built with [Axum](https://github.com/tokio-rs/axum) web framework
- MCP protocol support via [rmcp](https://github.com/modelcontextprotocol/rmcp)
- Inspired by [Postman](https://www.postman.com/) and the Model Context Protocol specification

## Support

- **Documentation**: [CLAUDE.md](./CLAUDE.md)
- **Deployment Guide**: [DEPLOYMENT_GUIDE.md](./DEPLOYMENT_GUIDE.md)
- **Issues**: [GitHub Issues](https://github.com/yourusername/saramcp/issues)
- **Discussions**: [GitHub Discussions](https://github.com/yourusername/saramcp/discussions)

---

**Built with ❤️ for the AI agent ecosystem**
