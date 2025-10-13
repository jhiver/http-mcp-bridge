-- Migration to create server_toolkits junction table
-- Links servers to imported toolkits

CREATE TABLE IF NOT EXISTS server_toolkits (
    server_id INTEGER NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    toolkit_id INTEGER NOT NULL REFERENCES toolkits(id) ON DELETE CASCADE,
    imported_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (server_id, toolkit_id)
);