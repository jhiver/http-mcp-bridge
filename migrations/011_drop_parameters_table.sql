-- Drop the unused parameters table
-- The system now uses dynamic extraction from tool templates instead of storing
-- parameter metadata in a separate table. Variables like {{search}} are extracted
-- at runtime from the tool's URL, headers, and body templates.

DROP TABLE IF EXISTS parameters;
