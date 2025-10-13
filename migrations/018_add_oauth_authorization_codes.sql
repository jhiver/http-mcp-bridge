-- OAuth authorization codes (short-lived, single-use)
CREATE TABLE IF NOT EXISTS oauth_authorization_codes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code TEXT NOT NULL UNIQUE,
    client_id TEXT NOT NULL,
    user_id INTEGER NOT NULL,
    redirect_uri TEXT NOT NULL,
    scope TEXT NOT NULL DEFAULT 'mcp:read',
    code_challenge TEXT,
    code_challenge_method TEXT,
    expires_at INTEGER NOT NULL,
    used INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    FOREIGN KEY (client_id) REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Indexes for performance (code already has UNIQUE constraint which creates an index)
CREATE INDEX idx_oauth_auth_codes_client ON oauth_authorization_codes(client_id);
CREATE INDEX idx_oauth_auth_codes_user ON oauth_authorization_codes(user_id);
CREATE INDEX idx_oauth_auth_codes_expires ON oauth_authorization_codes(expires_at);
