-- OAuth 2.0 tables for MCP server authentication
-- Following SaraMCP pattern with INTEGER timestamps

-- OAuth clients (registered applications)
CREATE TABLE IF NOT EXISTS oauth_clients (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    client_id TEXT UNIQUE NOT NULL,           -- Format: mcp_{uuid}
    client_secret_hash TEXT NOT NULL,         -- SHA-256 hash
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,                       -- Human-readable client name
    redirect_uris TEXT NOT NULL,              -- JSON array of allowed redirect URIs
    created_at INTEGER DEFAULT (unixepoch()),
    updated_at INTEGER DEFAULT (unixepoch())
);

CREATE INDEX idx_oauth_clients_client_id ON oauth_clients(client_id);
CREATE INDEX idx_oauth_clients_user_id ON oauth_clients(user_id);

-- Authorization codes with PKCE support
CREATE TABLE IF NOT EXISTS oauth_authorization_codes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code TEXT UNIQUE NOT NULL,                -- Format: code_{uuid}
    client_id TEXT NOT NULL REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    redirect_uri TEXT NOT NULL,               -- Must match client's registered URIs
    scope TEXT NOT NULL,                      -- Space-separated scopes
    code_challenge TEXT,                      -- PKCE code challenge (optional)
    code_challenge_method TEXT,               -- 'S256' or 'plain' (optional)
    expires_at INTEGER NOT NULL,              -- Unix timestamp
    used INTEGER DEFAULT 0,                   -- 0=unused, 1=used (prevent replay)
    created_at INTEGER DEFAULT (unixepoch())
);

CREATE INDEX idx_oauth_auth_codes_code ON oauth_authorization_codes(code);
CREATE INDEX idx_oauth_auth_codes_client_id ON oauth_authorization_codes(client_id);
CREATE INDEX idx_oauth_auth_codes_user_id ON oauth_authorization_codes(user_id);
CREATE INDEX idx_oauth_auth_codes_expires_at ON oauth_authorization_codes(expires_at);

-- Access tokens (hashed)
CREATE TABLE IF NOT EXISTS oauth_access_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    token_hash TEXT UNIQUE NOT NULL,          -- SHA-256 hash of token
    client_id TEXT NOT NULL REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope TEXT NOT NULL,                      -- Space-separated scopes
    expires_at INTEGER NOT NULL,              -- Unix timestamp
    created_at INTEGER DEFAULT (unixepoch()),
    last_used_at INTEGER                      -- Track usage for analytics
);

CREATE INDEX idx_oauth_access_tokens_token_hash ON oauth_access_tokens(token_hash);
CREATE INDEX idx_oauth_access_tokens_client_id ON oauth_access_tokens(client_id);
CREATE INDEX idx_oauth_access_tokens_user_id ON oauth_access_tokens(user_id);
CREATE INDEX idx_oauth_access_tokens_expires_at ON oauth_access_tokens(expires_at);

-- Refresh tokens (hashed) with rotation support
CREATE TABLE IF NOT EXISTS oauth_refresh_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    token_hash TEXT UNIQUE NOT NULL,          -- SHA-256 hash of token
    client_id TEXT NOT NULL REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope TEXT NOT NULL,                      -- Space-separated scopes
    expires_at INTEGER NOT NULL,              -- Unix timestamp
    used INTEGER DEFAULT 0,                   -- 0=unused, 1=used (for rotation)
    replaced_by_id INTEGER REFERENCES oauth_refresh_tokens(id), -- Token rotation chain
    created_at INTEGER DEFAULT (unixepoch())
);

CREATE INDEX idx_oauth_refresh_tokens_token_hash ON oauth_refresh_tokens(token_hash);
CREATE INDEX idx_oauth_refresh_tokens_client_id ON oauth_refresh_tokens(client_id);
CREATE INDEX idx_oauth_refresh_tokens_user_id ON oauth_refresh_tokens(user_id);
CREATE INDEX idx_oauth_refresh_tokens_expires_at ON oauth_refresh_tokens(expires_at);
CREATE INDEX idx_oauth_refresh_tokens_replaced_by ON oauth_refresh_tokens(replaced_by_id);
