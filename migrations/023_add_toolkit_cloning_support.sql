-- Add toolkit cloning support with parent tracking and clone count
-- Migration: 023_add_toolkit_cloning_support.sql
-- Created: 2025-01-09

-- Add parent_toolkit_id to track the original toolkit when cloning
ALTER TABLE toolkits ADD COLUMN parent_toolkit_id INTEGER REFERENCES toolkits(id) ON DELETE SET NULL;

-- Add clone_count to cache the number of times a toolkit has been cloned
-- This avoids expensive COUNT queries on every listing
ALTER TABLE toolkits ADD COLUMN clone_count INTEGER NOT NULL DEFAULT 0;

-- Create indexes for efficient querying
CREATE INDEX idx_toolkits_parent_toolkit_id ON toolkits(parent_toolkit_id);
CREATE INDEX idx_toolkits_clone_count ON toolkits(clone_count);

-- Create a compound index for finding public toolkits sorted by popularity
CREATE INDEX idx_toolkits_public_clones ON toolkits(visibility, clone_count DESC)
WHERE visibility = 'public';

-- Note: We use ON DELETE SET NULL for parent_toolkit_id so that if the original
-- toolkit is deleted, cloned toolkits remain but lose their parent reference.
-- This preserves user data while maintaining referential integrity.