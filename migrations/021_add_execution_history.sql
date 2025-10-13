-- Create execution_history table to track MCP tool executions
CREATE TABLE execution_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id INTEGER NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    instance_id INTEGER NOT NULL REFERENCES tool_instances(id) ON DELETE CASCADE,
    tool_id INTEGER NOT NULL REFERENCES tools(id) ON DELETE CASCADE,

    -- Execution timing
    started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at TEXT,
    duration_ms INTEGER,

    -- Execution result
    status TEXT NOT NULL CHECK (status IN ('success', 'error', 'timeout')),
    http_status_code INTEGER,
    error_message TEXT,

    -- Input/Output data
    input_params TEXT,  -- JSON string of input parameters
    response_body TEXT,  -- HTTP response body
    response_headers TEXT,  -- JSON string of response headers

    -- Optional detailed logging
    request_url TEXT,
    request_method TEXT,
    response_size_bytes INTEGER,

    -- Metadata
    transport TEXT CHECK (transport IN ('http', 'sse')),
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- Indexes for efficient queries
CREATE INDEX idx_execution_history_server_id ON execution_history(server_id);
CREATE INDEX idx_execution_history_instance_id ON execution_history(instance_id);
CREATE INDEX idx_execution_history_started_at ON execution_history(started_at);
CREATE INDEX idx_execution_history_status ON execution_history(status);
CREATE INDEX idx_execution_history_tool_id ON execution_history(tool_id);
