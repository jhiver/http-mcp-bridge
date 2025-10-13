-- Fix timestamp columns in servers table
-- SQLite CURRENT_TIMESTAMP produces TEXT, not INTEGER
-- This migration converts to INTEGER Unix timestamps

-- Create new table with correct INTEGER timestamp columns
CREATE TABLE servers_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    created_at INTEGER DEFAULT (unixepoch()),
    updated_at INTEGER DEFAULT (unixepoch()),
    UNIQUE(user_id, name)
);

-- Copy data, converting text timestamps to Unix epoch
INSERT INTO servers_new (id, user_id, name, description, created_at, updated_at)
SELECT
    id,
    user_id,
    name,
    description,
    unixepoch(created_at),
    unixepoch(updated_at)
FROM servers;

-- Drop old table and rename new one
DROP TABLE servers;
ALTER TABLE servers_new RENAME TO servers;

-- Recreate index
CREATE INDEX idx_servers_user_id ON servers(user_id);
