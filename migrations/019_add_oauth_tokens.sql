-- Phase 5: Modify oauth_refresh_tokens to use used_at instead of used/replaced_by_id
-- SQLite doesn't support DROP COLUMN, so we need to recreate the table

-- Create new table with correct schema
CREATE TABLE oauth_refresh_tokens_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    token_hash TEXT UNIQUE NOT NULL,
    client_id TEXT NOT NULL REFERENCES oauth_clients(client_id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at INTEGER,  -- Marks when token was rotated (NULL = not used)
    created_at INTEGER DEFAULT (unixepoch())
);

-- Copy data from old table (converting used=1 to used_at=created_at)
INSERT INTO oauth_refresh_tokens_new (id, token_hash, client_id, user_id, scope, expires_at, used_at, created_at)
SELECT id, token_hash, client_id, user_id, scope, expires_at,
       CASE WHEN used = 1 THEN created_at ELSE NULL END,
       created_at
FROM oauth_refresh_tokens;

-- Drop old table
DROP TABLE oauth_refresh_tokens;

-- Rename new table
ALTER TABLE oauth_refresh_tokens_new RENAME TO oauth_refresh_tokens;

-- Recreate indexes
CREATE INDEX idx_oauth_refresh_tokens_token_hash ON oauth_refresh_tokens(token_hash);
CREATE INDEX idx_oauth_refresh_tokens_client_id ON oauth_refresh_tokens(client_id);
CREATE INDEX idx_oauth_refresh_tokens_user_id ON oauth_refresh_tokens(user_id);
CREATE INDEX idx_oauth_refresh_tokens_expires_at ON oauth_refresh_tokens(expires_at);
