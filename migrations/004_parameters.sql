-- Create parameters table
CREATE TABLE IF NOT EXISTS parameters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tool_id INTEGER NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    param_type TEXT NOT NULL CHECK (param_type IN ('string', 'number', 'integer', 'boolean', 'json', 'url', 'file')),
    required BOOLEAN DEFAULT FALSE,
    default_value TEXT,
    description TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    UNIQUE(tool_id, name)
);

CREATE INDEX idx_parameters_tool_id ON parameters(tool_id);