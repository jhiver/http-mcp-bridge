-- Fix timestamp columns in tools and toolkits tables
-- SQLite CURRENT_TIMESTAMP produces TEXT, not INTEGER
-- This migration converts to INTEGER Unix timestamps

-- Fix tools table
CREATE TABLE tools_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    toolkit_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    method VARCHAR(10) DEFAULT 'GET',
    url TEXT,
    headers TEXT,
    body TEXT,
    timeout_ms INTEGER DEFAULT 30000,
    created_at INTEGER DEFAULT (unixepoch()),
    updated_at INTEGER DEFAULT (unixepoch()),
    FOREIGN KEY (toolkit_id) REFERENCES toolkits(id) ON DELETE CASCADE,
    CHECK (method IN ('GET', 'POST', 'PUT', 'DELETE', 'PATCH'))
);

-- Copy data, converting text timestamps to Unix epoch
INSERT INTO tools_new (id, toolkit_id, name, description, method, url, headers, body, timeout_ms, created_at, updated_at)
SELECT
    id,
    toolkit_id,
    name,
    description,
    method,
    url,
    headers,
    body,
    timeout_ms,
    CASE
        WHEN typeof(created_at) = 'text' THEN unixepoch(created_at)
        ELSE created_at
    END,
    CASE
        WHEN typeof(updated_at) = 'text' THEN unixepoch(updated_at)
        ELSE updated_at
    END
FROM tools;

-- Drop old table and rename new one
DROP TABLE tools;
ALTER TABLE tools_new RENAME TO tools;

-- Recreate indexes
CREATE INDEX idx_tools_toolkit_id ON tools(toolkit_id);
CREATE UNIQUE INDEX idx_tools_toolkit_name ON tools(toolkit_id, name);

-- Fix toolkits table
CREATE TABLE toolkits_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    visibility TEXT DEFAULT 'private' CHECK(visibility IN ('private', 'public')),
    created_at INTEGER DEFAULT (unixepoch()),
    updated_at INTEGER DEFAULT (unixepoch()),
    UNIQUE(user_id, title)
);

-- Copy data, converting text timestamps to Unix epoch
INSERT INTO toolkits_new (id, user_id, title, description, visibility, created_at, updated_at)
SELECT
    id,
    user_id,
    title,
    description,
    visibility,
    CASE
        WHEN typeof(created_at) = 'text' THEN unixepoch(created_at)
        ELSE created_at
    END,
    CASE
        WHEN typeof(updated_at) = 'text' THEN unixepoch(updated_at)
        ELSE updated_at
    END
FROM toolkits;

-- Drop old table and rename new one
DROP TABLE toolkits;
ALTER TABLE toolkits_new RENAME TO toolkits;

-- Recreate index
CREATE INDEX idx_toolkits_user_id ON toolkits(user_id);
