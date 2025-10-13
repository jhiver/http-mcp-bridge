-- Migration to create tool_instances table
-- Represents configured instances of tools with custom names

CREATE TABLE IF NOT EXISTS tool_instances (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id INTEGER NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    tool_id INTEGER NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    instance_name TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(server_id, instance_name)
);

CREATE INDEX idx_tool_instances_server_id ON tool_instances(server_id);
CREATE INDEX idx_tool_instances_tool_id ON tool_instances(tool_id);