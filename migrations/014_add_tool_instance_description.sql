-- Add description field to tool_instances table
-- Tool instances should inherit description from their parent tool by default
ALTER TABLE tool_instances ADD COLUMN description TEXT;
