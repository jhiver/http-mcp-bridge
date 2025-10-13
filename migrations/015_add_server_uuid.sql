-- Add uuid column to servers table
ALTER TABLE servers ADD COLUMN uuid TEXT;

-- Backfill UUIDs for existing servers (generate unique UUIDs)
-- SQLite doesn't have native UUID, so we'll use a combination approach
UPDATE servers SET uuid = lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) || '-' || substr('89ab', abs(random()) % 4 + 1, 1) || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6))) WHERE uuid IS NULL;

-- Make uuid NOT NULL and UNIQUE after backfill
-- SQLite doesn't support ALTER COLUMN, so we need to recreate the constraint check
CREATE UNIQUE INDEX idx_servers_uuid ON servers(uuid);
