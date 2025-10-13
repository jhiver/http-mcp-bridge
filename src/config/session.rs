use std::env;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use sha2::{Digest, Sha512};
use time::Duration;
use tower_sessions::{
    cookie::{Key, SameSite},
    service::SignedCookie,
    Expiry, SessionManagerLayer,
};
use tower_sessions_sqlx_store::SqliteStore;
use tracing::warn;

/// Convenience alias for the signed session layer produced by `SessionConfig`.
pub type SessionLayer = SessionManagerLayer<SqliteStore, SignedCookie>;

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub secure: bool,
    pub http_only: bool,
    pub same_site: SameSite,
    pub expiry: Duration,
    pub name: String,
}

impl SessionConfig {
    pub fn from_env() -> Self {
        let environment = current_environment();
        let is_production = environment == "production";

        if is_production {
            SessionConfig {
                secure: true,
                http_only: true,
                same_site: SameSite::Strict,
                expiry: Duration::hours(2),
                name: "__Host-session".to_string(),
            }
        } else {
            SessionConfig {
                secure: false,
                http_only: true,
                same_site: SameSite::Lax,
                expiry: Duration::days(7),
                name: "session".to_string(),
            }
        }
    }

    pub fn create_layer(&self, store: SqliteStore) -> SessionLayer {
        let key = load_session_key();

        SessionManagerLayer::new(store)
            .with_secure(self.secure)
            .with_http_only(self.http_only)
            .with_same_site(self.same_site)
            .with_name(self.name.clone())
            .with_expiry(Expiry::OnInactivity(self.expiry))
            .with_signed(key)
    }
}

pub fn validate_production_config() {
    if current_environment() != "production" {
        return;
    }

    if !env_flag_enabled("FORCE_HTTPS") {
        panic!("FATAL: Production environment requires HTTPS. Set FORCE_HTTPS=true");
    }

    let secret = env::var("SESSION_SECRET").expect("SESSION_SECRET must be set in production");
    let decoded_secret = decode_secret_bytes(&secret);

    if decoded_secret.len() < 64 {
        panic!("FATAL: SESSION_SECRET must be at least 64 bytes in production");
    }

    let lowered = secret.to_ascii_lowercase();
    if lowered.contains("example") || lowered.contains("changeme") || lowered.contains("default") {
        panic!("FATAL: SESSION_SECRET appears to be a default value. Generate a secure secret!");
    }
}

fn current_environment() -> String {
    env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string())
}

fn env_flag_enabled(key: &str) -> bool {
    env::var(key)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "True"))
        .unwrap_or(false)
}

fn load_session_key() -> Key {
    match env::var("SESSION_SECRET") {
        Ok(secret) if !secret.is_empty() => {
            let bytes = decode_secret_bytes(&secret);
            key_from_secret_bytes(&bytes)
        }
        _ => {
            warn!("SESSION_SECRET not set; generating ephemeral key (development only)");
            Key::generate()
        }
    }
}

fn decode_secret_bytes(secret: &str) -> Vec<u8> {
    STANDARD
        .decode(secret.as_bytes())
        .unwrap_or_else(|_| secret.as_bytes().to_vec())
}

fn key_from_secret_bytes(bytes: &[u8]) -> Key {
    if bytes.len() >= 64 {
        Key::from(&bytes[..64])
    } else {
        let digest = Sha512::digest(bytes);
        Key::from(digest.as_slice())
    }
}
