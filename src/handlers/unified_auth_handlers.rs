use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Form, Path, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use tower_sessions::Session;

#[derive(Template, WebTemplate)]
#[template(path = "auth/unified_auth.html")]
struct UnifiedAuthTemplate {
    error: Option<String>,
    csrf_token: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "auth/check_email.html")]
struct CheckEmailTemplate {
    email: String,
}

#[allow(dead_code)]
#[derive(Template, WebTemplate)]
#[template(path = "auth/verify_success.html")]
struct VerifySuccessTemplate {}

#[derive(Template, WebTemplate)]
#[template(path = "auth/magic_error.html")]
struct MagicErrorTemplate {
    error: String,
}

#[derive(Deserialize)]
pub struct UnifiedAuthForm {
    email: String,
    password: Option<String>,
    #[allow(dead_code)]
    csrf_token: String,
}

fn generate_csrf_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    hex::encode(bytes)
}

fn auth_error(msg: &str) -> Response {
    let template = UnifiedAuthTemplate {
        error: Some(msg.to_string()),
        csrf_token: generate_csrf_token(),
    };
    Html(
        template
            .render()
            .unwrap_or_else(|_| format!("<html><body><h1>Error: {}</h1></body></html>", msg)),
    )
    .into_response()
}

pub async fn unified_auth_handler(
    State(app_state): State<AppState>,
    session: Session,
    Form(form): Form<UnifiedAuthForm>,
) -> Response {
    let email = form.email.trim();

    if email.is_empty() {
        return auth_error("Please enter your email address");
    }

    let user_opt = match app_state.user_service.find_user_by_email(email).await {
        Ok(user) => user,
        Err(_) => return auth_error("An error occurred. Please try again."),
    };

    let password_is_empty = form.password.as_ref().is_none_or(|p| p.is_empty());

    match (user_opt, password_is_empty) {
        // User exists + password provided: Login
        (Some(_user), false) => {
            // password_is_empty == false guarantees password is Some and non-empty
            let password = match form.password.as_ref() {
                Some(p) => p,
                None => return auth_error("Password is required for login"),
            };
            let request = crate::services::auth_service::LoginRequest {
                email: email.to_string(),
                password: password.clone(),
            };

            match app_state.auth_service.authenticate(request).await {
                Ok(user) => {
                    if session.insert("user_id", user.id).await.is_err()
                        || session.insert("email", user.email).await.is_err()
                        || session
                            .insert("auth_timestamp", chrono::Utc::now().timestamp())
                            .await
                            .is_err()
                    {
                        return auth_error("Failed to create session");
                    }

                    // Check for OAuth return_to URL
                    let redirect_url = match session.get::<String>("oauth_return_to").await {
                        Ok(Some(return_to)) => {
                            // Clear the return_to from session
                            let _ = session.remove::<String>("oauth_return_to").await;
                            return_to
                        }
                        _ => "/dashboard".to_string(),
                    };

                    Redirect::to(&redirect_url).into_response()
                }
                Err(_) => auth_error("Invalid email or password"),
            }
        }

        // User exists + no password: Magic link
        (Some(user), true) => {
            match app_state
                .auth_token_service
                .create_magic_login_token(user.id)
                .await
            {
                Ok(_) => {
                    let template = CheckEmailTemplate {
                        email: email.to_string(),
                    };
                    Html(template.render().unwrap_or_else(|_| {
                        "<html><body><h1>Check your email</h1></body></html>".to_string()
                    }))
                    .into_response()
                }
                Err(_) => auth_error("Failed to send login link. Please try again."),
            }
        }

        // User doesn't exist + password provided: Create account with password
        (None, false) => {
            let password = match form.password.as_ref() {
                Some(p) => p,
                None => return auth_error("Password is required for account creation"),
            };
            match app_state
                .auth_token_service
                .create_pending_registration(email, Some(password))
                .await
            {
                Ok(_) => {
                    let template = CheckEmailTemplate {
                        email: email.to_string(),
                    };
                    Html(template.render().unwrap_or_else(|_| {
                        "<html><body><h1>Check your email</h1></body></html>".to_string()
                    }))
                    .into_response()
                }
                Err(_) => auth_error("Failed to create account. Please try again."),
            }
        }

        // User doesn't exist + no password: Create account without password
        (None, true) => {
            match app_state
                .auth_token_service
                .create_pending_registration(email, None)
                .await
            {
                Ok(_) => {
                    let template = CheckEmailTemplate {
                        email: email.to_string(),
                    };
                    Html(template.render().unwrap_or_else(|_| {
                        "<html><body><h1>Check your email</h1></body></html>".to_string()
                    }))
                    .into_response()
                }
                Err(_) => auth_error("Failed to create account. Please try again."),
            }
        }
    }
}

pub async fn verify_token_handler(
    State(app_state): State<AppState>,
    session: Session,
    Path(token): Path<String>,
) -> Response {
    match app_state
        .auth_token_service
        .verify_registration_token(&token)
        .await
    {
        Ok(user) => {
            if session.insert("user_id", user.id).await.is_err()
                || session.insert("email", user.email).await.is_err()
                || session
                    .insert("auth_timestamp", chrono::Utc::now().timestamp())
                    .await
                    .is_err()
            {
                return auth_error("Failed to create session");
            }

            // Check for OAuth return_to URL
            let redirect_url = match session.get::<String>("oauth_return_to").await {
                Ok(Some(return_to)) => {
                    // Clear the return_to from session
                    let _ = session.remove::<String>("oauth_return_to").await;
                    return_to
                }
                _ => "/dashboard".to_string(),
            };

            Redirect::to(&redirect_url).into_response()
        }
        Err(_) => {
            let template = MagicErrorTemplate {
                error: "Invalid or expired verification link".to_string(),
            };
            Html(
                template.render().unwrap_or_else(|_| {
                    "<html><body><h1>Invalid Link</h1></body></html>".to_string()
                }),
            )
            .into_response()
        }
    }
}

pub async fn magic_login_handler(
    State(app_state): State<AppState>,
    session: Session,
    Path(token): Path<String>,
) -> Response {
    match app_state
        .auth_token_service
        .verify_magic_login_token(&token)
        .await
    {
        Ok(user) => {
            if session.insert("user_id", user.id).await.is_err()
                || session.insert("email", user.email).await.is_err()
                || session
                    .insert("auth_timestamp", chrono::Utc::now().timestamp())
                    .await
                    .is_err()
            {
                return auth_error("Failed to create session");
            }

            // Check for OAuth return_to URL
            let redirect_url = match session.get::<String>("oauth_return_to").await {
                Ok(Some(return_to)) => {
                    // Clear the return_to from session
                    let _ = session.remove::<String>("oauth_return_to").await;
                    return_to
                }
                _ => "/dashboard".to_string(),
            };

            Redirect::to(&redirect_url).into_response()
        }
        Err(_) => {
            let template = MagicErrorTemplate {
                error: "Invalid or expired login link".to_string(),
            };
            Html(
                template.render().unwrap_or_else(|_| {
                    "<html><body><h1>Invalid Link</h1></body></html>".to_string()
                }),
            )
            .into_response()
        }
    }
}

pub async fn logout_handler(session: Session) -> impl IntoResponse {
    let _ = session.flush().await;
    Redirect::to("/")
}
