use std::{collections::HashMap, env};

use axum::{
    body::Body,
    http::{header, Request},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use saramcp::{
    config::session::{validate_production_config, SessionConfig},
    test_utils::test_helpers,
};
use serial_test::serial;
use tower::ServiceExt;
use tower_sessions::{cookie::SameSite, Session};
use tower_sessions_sqlx_store::SqliteStore;

#[derive(Default)]
struct EnvGuard {
    original: HashMap<String, Option<String>>,
}

impl EnvGuard {
    fn set(&mut self, key: &str, value: impl Into<String>) {
        self.original
            .entry(key.to_string())
            .or_insert_with(|| env::var(key).ok());
        env::set_var(key, value.into());
    }

    fn remove(&mut self, key: &str) {
        self.original
            .entry(key.to_string())
            .or_insert_with(|| env::var(key).ok());
        env::remove_var(key);
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.original.drain() {
            match value {
                Some(v) => env::set_var(&key, v),
                None => env::remove_var(&key),
            }
        }
    }
}

#[tokio::test]
#[serial]
async fn session_cookie_flags_are_secure_in_production() {
    let mut env_guard = EnvGuard::default();
    env_guard.set("ENVIRONMENT", "production");
    env_guard.set("FORCE_HTTPS", "true");
    let secret = STANDARD.encode([42u8; 64]);
    env_guard.set("SESSION_SECRET", secret);

    validate_production_config();

    let pool = test_helpers::create_test_db().await.unwrap();
    let session_store = SqliteStore::new(pool)
        .with_table_name("sessions_test")
        .expect("valid session table name for tests");
    session_store
        .migrate()
        .await
        .expect("session table migration to succeed");

    let session_layer = SessionConfig::from_env().create_layer(session_store);

    async fn set_session(session: Session) -> &'static str {
        session.insert("csrf", "token").await.unwrap();
        "ok"
    }

    let app = Router::new().route("/", get(set_session)).layer(session_layer);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request to build"),
        )
        .await
        .expect("router to respond");

    let cookie_header = response
        .headers()
        .get(header::SET_COOKIE)
        .expect("session cookie to be issued")
        .to_str()
        .expect("cookie header to be valid ASCII");

    let cookie = tower_sessions::cookie::Cookie::parse(cookie_header)
        .expect("cookie header to parse correctly");

    assert_eq!(cookie.name(), "__Host-session", "cookie name should be hardened");
    assert_eq!(cookie.http_only(), Some(true), "HttpOnly flag must be set");
    assert_eq!(cookie.secure(), Some(true), "Secure flag must be enabled");
    assert_eq!(
        cookie.same_site(),
        Some(SameSite::Strict),
        "SameSite=Strict defends against CSRF"
    );
    assert_eq!(
        cookie.path().unwrap_or("/"),
        "/",
        "cookie path must be root for __Host- prefix"
    );
}

#[test]
#[serial]
fn production_requires_https_flag() {
    let mut env_guard = EnvGuard::default();
    env_guard.set("ENVIRONMENT", "production");
    env_guard.remove("FORCE_HTTPS");
    env_guard.set("SESSION_SECRET", "a".repeat(64));

    let result = std::panic::catch_unwind(|| validate_production_config());
    assert!(result.is_err(), "FORCE_HTTPS must be enforced in production");
}

#[test]
#[serial]
fn production_rejects_weak_secrets() {
    let mut env_guard = EnvGuard::default();
    env_guard.set("ENVIRONMENT", "production");
    env_guard.set("FORCE_HTTPS", "true");
    env_guard.set("SESSION_SECRET", "changeme");

    let result = std::panic::catch_unwind(|| validate_production_config());
    assert!(
        result.is_err(),
        "Weak or default session secrets must panic in production"
    );
}
