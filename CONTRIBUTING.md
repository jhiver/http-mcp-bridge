# Contributing to SaraMCP

Thank you for your interest in contributing to SaraMCP! We welcome contributions from the community and are grateful for your support.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Development Workflow](#development-workflow)
- [Coding Standards](#coding-standards)
- [Testing Requirements](#testing-requirements)
- [Pull Request Process](#pull-request-process)
- [Commit Message Guidelines](#commit-message-guidelines)

## Code of Conduct

By participating in this project, you agree to maintain a respectful and inclusive environment. We expect all contributors to:

- Be respectful and considerate in communications
- Welcome newcomers and help them get started
- Focus on constructive feedback
- Assume good intentions
- Accept responsibility for mistakes

## Getting Started

1. **Fork the repository** on GitHub
2. **Clone your fork** locally:
   ```bash
   git clone https://github.com/your-username/saramcp.git
   cd saramcp
   ```
3. **Add upstream remote**:
   ```bash
   git remote add upstream https://github.com/original-owner/saramcp.git
   ```
4. **Create a branch** for your work:
   ```bash
   git checkout -b feature/my-amazing-feature
   ```

## Development Setup

### Prerequisites

- Rust 1.75 or later
- SQLite 3
- Docker (optional, for testing containerized builds)

### Initial Setup

```bash
# Install Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install sqlx-cli for database management
cargo install sqlx-cli --no-default-features --features sqlite

# Clone and setup
git clone https://github.com/your-username/saramcp.git
cd saramcp

# Create environment file
cp .env.example .env

# Generate secrets
echo "SESSION_SECRET=$(openssl rand -hex 32)" >> .env
echo "SARAMCP_MASTER_KEY=$(openssl rand -base64 32)" >> .env

# Create data directory
mkdir -p data

# Run migrations
sqlx database create
sqlx migrate run

# Start development server
cargo run
```

The server will be available at `http://localhost:8080`

## Development Workflow

### 1. Keep Your Fork Updated

```bash
git fetch upstream
git checkout master
git merge upstream/master
```

### 2. Create a Feature Branch

```bash
git checkout -b feature/descriptive-name
# or
git checkout -b fix/issue-description
```

### 3. Make Your Changes

- Write clean, idiomatic Rust code
- Add tests for new functionality
- Update documentation as needed
- Follow the coding standards below

### 4. Test Your Changes

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Check code formatting
cargo fmt -- --check

# Run linter
cargo clippy -- -D warnings
```

### 5. Commit Your Changes

See [Commit Message Guidelines](#commit-message-guidelines) below.

### 6. Push and Create Pull Request

```bash
git push origin feature/descriptive-name
```

Then open a Pull Request on GitHub.

## Coding Standards

### General Principles

- **DRY (Don't Repeat Yourself)**: Avoid code duplication
- **KISS (Keep It Simple, Stupid)**: Prefer simple solutions
- **Write idiomatic Rust**: Follow Rust best practices and conventions
- **Document public APIs**: All public functions, structs, and modules must have documentation

### Specific Rules

1. **Function Length**: Maximum 60 lines per function
2. **Parameters**: Maximum 4 parameters per function (use structs for more)
3. **Nesting**: Maximum 3 levels of nesting
4. **Error Handling**: Always use `Result<T, E>`, never use `unwrap()` or `expect()` in production code
5. **Naming Conventions**:
   - `snake_case` for functions and variables
   - `PascalCase` for types and traits
   - `SCREAMING_SNAKE_CASE` for constants
6. **Database Conventions**:
   - `lowercase_with_underscores` for all identifiers
   - Always include `created_at` and `updated_at` timestamps
   - Add indexes for foreign keys and frequently queried columns

### Code Organization

```rust
// 1. Imports (grouped: std, external crates, internal modules)
use std::collections::HashMap;

use axum::Router;
use sqlx::SqlitePool;

use crate::models::User;

// 2. Constants and type aliases
const MAX_RETRIES: usize = 3;
type UserId = i64;

// 3. Structs and enums
pub struct MyService {
    pool: SqlitePool,
}

// 4. Trait implementations
impl MyService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

// 5. Functions
pub async fn helper_function() -> Result<(), Error> {
    // ...
}

// 6. Tests (in separate module)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        // ...
    }
}
```

## Testing Requirements

### Test Coverage

- All new features **must** have tests
- Bug fixes **must** include regression tests
- Aim for high test coverage (we strive for >80%)

### Test Types

1. **Unit Tests**: Test individual functions and methods
   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn test_parameter_resolution() {
           // Test logic here
       }
   }
   ```

2. **Integration Tests**: Test component interactions
   ```rust
   // In tests/ directory
   #[sqlx::test]
   async fn test_server_creation(pool: SqlitePool) {
       // Test with real database
   }
   ```

3. **Property-Based Tests**: Use when appropriate
   ```rust
   #[quickcheck]
   fn prop_reversible(input: Vec<u8>) -> bool {
       decode(encode(input.clone())) == input
   }
   ```

### Test Database

- Use `#[sqlx::test]` attribute for database tests
- Tests automatically get isolated test databases
- Migrations run automatically before each test

### Running Tests

```bash
# All tests
cargo test

# Specific test
cargo test test_parameter_resolution

# With output
cargo test -- --nocapture

# Integration tests only
cargo test --test '*'

# With coverage
cargo tarpaulin --out Html
```

## Pull Request Process

### Before Submitting

1. **Update documentation**: README, CLAUDE.md, inline docs
2. **Run all quality checks**:
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   cargo test
   ```
3. **Update CHANGELOG** (if applicable)
4. **Rebase on latest master**:
   ```bash
   git fetch upstream
   git rebase upstream/master
   ```

### PR Description Template

```markdown
## Description
Brief description of the changes

## Motivation and Context
Why is this change needed? What problem does it solve?

## Types of Changes
- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to change)
- [ ] Documentation update

## Testing
How has this been tested? Please describe the tests you ran.

## Checklist
- [ ] My code follows the code style of this project
- [ ] I have added tests to cover my changes
- [ ] All new and existing tests pass
- [ ] My changes require a change to the documentation
- [ ] I have updated the documentation accordingly
- [ ] I have added an entry to CHANGELOG.md (if applicable)
```

### Review Process

- All PRs require at least one approval
- CI must pass (formatting, linting, tests)
- Maintainers may request changes
- Be responsive to feedback
- Once approved, a maintainer will merge

## Commit Message Guidelines

### Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

### Type

- **feat**: New feature
- **fix**: Bug fix
- **docs**: Documentation changes
- **style**: Code style changes (formatting, no logic change)
- **refactor**: Code refactoring
- **test**: Adding or updating tests
- **chore**: Maintenance tasks, dependencies

### Scope (Optional)

The part of the codebase affected:
- `auth`: Authentication
- `mcp`: MCP protocol
- `services`: Service layer
- `ui`: Web interface
- `db`: Database/migrations

### Examples

```
feat(mcp): add hot-reload support for tool instances

Implements dynamic tool router reloading using Arc<RwLock<ToolRouter>>.
This allows updating tool configurations without restarting the MCP server.

Closes #123
```

```
fix(auth): prevent CSRF attacks on login form

Add CSRF token validation to all authentication endpoints.
Tokens are generated per-session and validated on form submission.

Fixes #456
```

```
docs: update README with OAuth setup instructions

Added section explaining how to configure OAuth 2.0 authentication
including client registration and token endpoint usage.
```

## Types of Contributions

### Bug Reports

- Use GitHub Issues
- Include steps to reproduce
- Provide system information
- Include error messages and logs

### Feature Requests

- Use GitHub Discussions first
- Explain the use case
- Describe expected behavior
- Be open to alternative solutions

### Code Contributions

- Start with issues labeled "good first issue"
- Discuss major changes in an issue first
- Follow the development workflow above

### Documentation

- Fix typos and improve clarity
- Add examples and tutorials
- Update outdated information
- Translate to other languages

## Questions?

- Open a GitHub Discussion
- Check existing issues and discussions
- Read [CLAUDE.md](./CLAUDE.md) for architecture details

## License

By contributing to SaraMCP, you agree that your contributions will be licensed under the MIT License.

---

Thank you for contributing to SaraMCP! Your efforts help make this project better for everyone.
