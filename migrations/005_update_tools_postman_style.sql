-- Migration to update tools table for Postman-like HTTP request configuration
-- SQLite version - requires manual column management

-- Create temporary table with new structure
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
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (toolkit_id) REFERENCES toolkits(id) ON DELETE CASCADE,
    CHECK (method IN ('GET', 'POST', 'PUT', 'DELETE', 'PATCH'))
);

-- Copy existing data (map old columns to new)
INSERT INTO tools_new (id, toolkit_id, name, description, created_at, updated_at)
SELECT id, toolkit_id, name, description, created_at, updated_at
FROM tools;

-- Drop old table
DROP TABLE tools;

-- Rename new table to original name
ALTER TABLE tools_new RENAME TO tools;

-- Create index for method since we might filter by it
CREATE INDEX idx_tools_method ON tools(method);