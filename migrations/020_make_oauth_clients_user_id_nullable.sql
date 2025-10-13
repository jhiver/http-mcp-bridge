-- Make oauth_clients.user_id nullable to support public client registration
-- This follows RFC 7591 OAuth 2.0 Dynamic Client Registration Protocol
-- which allows unauthenticated client registration

-- SQLite doesn't support ALTER COLUMN, so we need to recreate the table
-- 1. Create new table with nullable user_id
CREATE TABLE oauth_clients_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    client_id TEXT UNIQUE NOT NULL,           -- Format: mcp_{uuid}
    client_secret_hash TEXT NOT NULL,         -- SHA-256 hash
    user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,  -- Now nullable
    name TEXT NOT NULL,                       -- Human-readable client name
    redirect_uris TEXT NOT NULL,              -- JSON array of allowed redirect URIs
    created_at INTEGER DEFAULT (unixepoch()),
    updated_at INTEGER DEFAULT (unixepoch())
);

-- 2. Copy data from old table
INSERT INTO oauth_clients_new (id, client_id, client_secret_hash, user_id, name, redirect_uris, created_at, updated_at)
SELECT id, client_id, client_secret_hash, user_id, name, redirect_uris, created_at, updated_at
FROM oauth_clients;

-- 3. Drop old table
DROP TABLE oauth_clients;

-- 4. Rename new table
ALTER TABLE oauth_clients_new RENAME TO oauth_clients;

-- 5. Recreate indexes
CREATE INDEX idx_oauth_clients_client_id ON oauth_clients(client_id);
CREATE INDEX idx_oauth_clients_user_id ON oauth_clients(user_id);
