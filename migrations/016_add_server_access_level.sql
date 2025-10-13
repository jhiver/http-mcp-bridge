-- Add access_level column to servers table
-- Default to 'private' for security
-- CHECK constraint ensures only valid values

ALTER TABLE servers ADD COLUMN access_level TEXT DEFAULT 'private'
    CHECK(access_level IN ('private', 'organization', 'public'));

-- Index for future filtering queries when implementing access control
CREATE INDEX idx_servers_access_level ON servers(access_level);
