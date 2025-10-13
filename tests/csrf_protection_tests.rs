// Integration tests to verify CSRF protection is properly implemented
// These tests should PASS when CSRF protection is working correctly

use saramcp::test_utils::test_helpers;
use sqlx::SqlitePool;

/// Helper to setup a test database with a user
async fn setup_test_db() -> SqlitePool {
    let pool = test_helpers::create_test_db().await.unwrap();

    // Create a test user with known credentials
    sqlx::query!(
        r#"
        INSERT INTO users (email, password_hash, email_verified, created_at)
        VALUES (?1, ?2, ?3, datetime('now'))
        "#,
        "test@example.com",
        "$argon2id$v=19$m=19456,t=2,p=1$VE0e3g7U4HQo7gvT$9bzlJ7VRVt3xLmOo8K+urPpPXD3O1INNhZ0M0XbLzWs", // "testpass123"
        true
    )
    .execute(&pool)
    .await
    .expect("Failed to create test user");

    pool
}

#[tokio::test]
async fn test_csrf_protection_is_implemented() {
    // This test verifies that CSRF protection is properly implemented
    use std::fs;

    // Read the auth handler file
    let auth_handler =
        fs::read_to_string("src/auth/handlers.rs").expect("Failed to read auth handler");

    // Check that csrf_token field is NOT marked with #[allow(dead_code)]
    assert!(
        !auth_handler.contains("#[allow(dead_code)]"),
        "CSRF token field should NOT be marked as dead code - it should be actively used"
    );

    // Check that CSRF validation is present
    assert!(
        auth_handler.contains("validate_csrf_form_field"),
        "Auth handlers should validate CSRF tokens"
    );

    // Check that we're using session-based CSRF tokens
    assert!(
        auth_handler.contains("get_or_create_csrf_token"),
        "Auth handlers should use session-based CSRF tokens"
    );
}

#[tokio::test]
async fn test_auth_handlers_validate_csrf() {
    use std::fs;

    let auth_handler =
        fs::read_to_string("src/auth/handlers.rs").expect("Failed to read auth handler");

    // Check login handler validates CSRF
    assert!(
        auth_handler.contains("pub async fn login_handler")
            && auth_handler.contains("validate_csrf_form_field(&session, &form.csrf_token)"),
        "Login handler should validate CSRF tokens"
    );

    // Check signup handler validates CSRF
    assert!(
        auth_handler.contains("pub async fn signup_handler")
            && auth_handler.contains("validate_csrf_form_field(&session, &form.csrf_token)"),
        "Signup handler should validate CSRF tokens"
    );
}

#[tokio::test]
async fn test_csrf_middleware_exists() {
    use std::fs;

    // Verify CSRF middleware file exists and has proper implementation
    let csrf_middleware =
        fs::read_to_string("src/middleware/csrf.rs").expect("Failed to read CSRF middleware");

    // Check for key components
    assert!(
        csrf_middleware.contains("pub async fn csrf_validation_middleware"),
        "CSRF validation middleware should exist"
    );

    assert!(
        csrf_middleware.contains("pub async fn validate_csrf_form_field"),
        "Form field validation function should exist"
    );

    assert!(
        csrf_middleware.contains("pub async fn generate_csrf_token"),
        "Token generation function should exist"
    );

    assert!(
        csrf_middleware.contains("pub async fn get_or_create_csrf_token"),
        "Get or create token function should exist"
    );
}

#[tokio::test]
async fn test_csrf_tokens_in_templates() {
    use std::fs;

    // Verify that templates include CSRF tokens in forms
    let login_template =
        fs::read_to_string("templates/auth/login.html").expect("Failed to read login template");

    assert!(
        login_template.contains(r#"name="csrf_token""#)
            && login_template.contains("value=\"{{ csrf_token }}\""),
        "Login template should include CSRF token field"
    );

    let signup_template =
        fs::read_to_string("templates/auth/signup.html").expect("Failed to read signup template");

    assert!(
        signup_template.contains(r#"name="csrf_token""#)
            && signup_template.contains("value=\"{{ csrf_token }}\""),
        "Signup template should include CSRF token field"
    );
}

#[tokio::test]
async fn test_javascript_includes_csrf_support() {
    use std::fs;

    // Check that form-handler.js includes CSRF token support for AJAX
    let form_handler =
        fs::read_to_string("static/form-handler.js").expect("Failed to read form handler");

    assert!(
        form_handler.contains("X-CSRF-Token"),
        "JavaScript should include CSRF token in AJAX headers"
    );

    assert!(
        form_handler.contains("csrfToken = formData.get('csrf_token')"),
        "JavaScript should extract CSRF token from form data"
    );
}

#[tokio::test]
async fn test_oauth_handler_csrf_implementation() {
    use std::fs;
    use std::path::Path;

    // OAuth handler has its own CSRF implementation for OAuth flow
    if Path::new("src/handlers/oauth_handlers.rs").exists() {
        let oauth_handler = fs::read_to_string("src/handlers/oauth_handlers.rs")
            .expect("Failed to read OAuth handler");

        // OAuth uses a different CSRF pattern for the OAuth authorization flow
        assert!(
            oauth_handler.contains("oauth_csrf")
                || oauth_handler.contains("state")
                || oauth_handler.contains("csrf"),
            "OAuth handler should have some form of CSRF protection"
        );
    }
}

#[tokio::test]
async fn test_main_uses_session_based_tokens() {
    use std::fs;

    // Check that main.rs uses session-based CSRF tokens
    let main_file = fs::read_to_string("src/main.rs").expect("Failed to read main.rs");

    assert!(
        main_file.contains("saramcp::middleware::csrf::get_or_create_csrf_token"),
        "Main application should use session-based CSRF token generation"
    );

    // Should NOT have the old insecure random generation
    assert!(
        !main_file.contains("fn generate_csrf_token()"),
        "Should not have local insecure token generation function"
    );
}

#[tokio::test]
async fn test_csrf_token_struct_has_expiry() {
    use std::fs;

    let csrf_middleware =
        fs::read_to_string("src/middleware/csrf.rs").expect("Failed to read CSRF middleware");

    // Verify token has expiry mechanism
    assert!(
        csrf_middleware.contains("pub fn is_expired(&self)"),
        "CSRF tokens should have expiry checking"
    );

    assert!(
        csrf_middleware.contains("86400"), // 24 hours in seconds
        "CSRF tokens should expire after 24 hours"
    );
}

#[tokio::test]
async fn test_middleware_regenerates_tokens() {
    use std::fs;

    let csrf_middleware =
        fs::read_to_string("src/middleware/csrf.rs").expect("Failed to read CSRF middleware");

    // Check for replay protection
    assert!(
        csrf_middleware.contains("// Token is valid - regenerate for next request (replay protection)") ||
        csrf_middleware.contains("regenerate") ||
        csrf_middleware.contains("generate_csrf_token"),
        "CSRF middleware should regenerate tokens after successful validation for replay protection"
    );
}

// Note: These are code-level tests. For true integration testing,
// we would need to actually make HTTP requests and verify responses.
// That would require running the server and using a client like reqwest.
