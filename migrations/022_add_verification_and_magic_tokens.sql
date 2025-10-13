-- Add pending registrations table for email verification before account creation
CREATE TABLE IF NOT EXISTS pending_registrations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT,  -- Nullable: if null, generate random password on verification
    token TEXT UNIQUE NOT NULL,
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_pending_registrations_token ON pending_registrations(token);
CREATE INDEX idx_pending_registrations_email ON pending_registrations(email);
CREATE INDEX idx_pending_registrations_expires_at ON pending_registrations(expires_at);

-- Add magic login tokens table for passwordless login (existing users)
CREATE TABLE IF NOT EXISTS magic_login_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    token TEXT UNIQUE NOT NULL,
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    used_at TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_magic_login_tokens_token ON magic_login_tokens(token);
CREATE INDEX idx_magic_login_tokens_user_id ON magic_login_tokens(user_id);
CREATE INDEX idx_magic_login_tokens_expires_at ON magic_login_tokens(expires_at);
