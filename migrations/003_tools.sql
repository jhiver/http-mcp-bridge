-- Create tools table
CREATE TABLE IF NOT EXISTS tools (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    toolkit_id INTEGER NOT NULL REFERENCES toolkits(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    mcp_server TEXT,
    mcp_command TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(toolkit_id, name)
);

CREATE INDEX idx_tools_toolkit_id ON tools(toolkit_id);