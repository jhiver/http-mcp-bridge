-- Migration to create instance_params table
-- Configures how each parameter is handled in a tool instance

CREATE TABLE IF NOT EXISTS instance_params (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    instance_id INTEGER NOT NULL REFERENCES tool_instances(id) ON DELETE CASCADE,
    param_name TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('exposed', 'server', 'instance')),
    value TEXT, -- Only used when source = 'instance'
    UNIQUE(instance_id, param_name)
);

CREATE INDEX idx_instance_params_instance_id ON instance_params(instance_id);