-- Create toolkits table
CREATE TABLE IF NOT EXISTS toolkits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    visibility TEXT DEFAULT 'private' CHECK (visibility IN ('private', 'public')),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_toolkits_user_id ON toolkits(user_id);
CREATE INDEX idx_toolkits_visibility ON toolkits(visibility);