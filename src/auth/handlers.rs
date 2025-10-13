use crate::middleware::csrf::{get_or_create_csrf_token, validate_csrf_form_field};
use crate::services::{
    auth_service::{AuthServiceError, LoginRequest},
    user_service::{CreateUserRequest, UserServiceError},
};
use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Form, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use tower_sessions::Session;

#[derive(Template, WebTemplate)]
#[template(path = "auth/signup.html")]
struct SignupTemplate {
    error: Option<String>,
    csrf_token: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "auth/login.html")]
struct LoginTemplate {
    error: Option<String>,
    signup_success: bool,
    csrf_token: String,
}

#[derive(Deserialize)]
pub struct SignupForm {
    email: String,
    password: String,
    password_confirm: String,
    csrf_token: String,
}

#[derive(Deserialize)]
pub struct LoginForm {
    email: String,
    password: String,
    remember_me: Option<bool>,
    csrf_token: String,
}

#[derive(Deserialize)]
pub struct LoginQuery {
    signup: Option<String>,
    return_to: Option<String>,
}

pub async fn signup_page(session: Session) -> Html<String> {
    let csrf_token = get_or_create_csrf_token(&session)
        .await
        .unwrap_or_else(|_| String::from("error"));

    let template = SignupTemplate {
        error: None,
        csrf_token,
    };
    Html(template.render().unwrap_or_else(|_| {
        "<html><body><h1>Error rendering signup page</h1></body></html>".to_string()
    }))
}

async fn signup_error(msg: &str, session: &Session) -> Response {
    let csrf_token = get_or_create_csrf_token(session)
        .await
        .unwrap_or_else(|_| String::from("error"));

    let template = SignupTemplate {
        error: Some(msg.to_string()),
        csrf_token,
    };
    Html(
        template
            .render()
            .unwrap_or_else(|_| format!("<html><body><h1>Error: {}</h1></body></html>", msg)),
    )
    .into_response()
}

pub async fn signup_handler(
    State(app_state): State<AppState>,
    session: Session,
    Form(form): Form<SignupForm>,
) -> Response {
    // Validate CSRF token
    if validate_csrf_form_field(&session, &form.csrf_token)
        .await
        .is_err()
    {
        return signup_error(
            "Invalid security token. Please refresh the page and try again.",
            &session,
        )
        .await;
    }

    let request = CreateUserRequest {
        email: form.email.clone(),
        password: form.password.clone(),
        password_confirm: Some(form.password_confirm.clone()),
        email_verified: false,
    };

    match app_state.user_service.create_user(request).await {
        Ok(_) => Redirect::to("/login?signup=success").into_response(),
        Err(err) => {
            let error_msg = match err {
                UserServiceError::InvalidEmail => "Please enter a valid email address",
                UserServiceError::WeakPassword => "Password must be at least 8 characters",
                UserServiceError::PasswordMismatch => "Passwords do not match",
                UserServiceError::EmailTaken => "Email already registered",
                _ => "Registration failed. Please try again.",
            };
            signup_error(error_msg, &session).await
        }
    }
}

pub async fn login_page(session: Session, Query(query): Query<LoginQuery>) -> Html<String> {
    // Store return_to in session for OAuth flow
    if let Some(ref return_to) = query.return_to {
        let _ = session.insert("login_return_to", return_to.as_str()).await;
    }

    let csrf_token = get_or_create_csrf_token(&session)
        .await
        .unwrap_or_else(|_| String::from("error"));

    let template = LoginTemplate {
        error: None,
        signup_success: query.signup.as_deref() == Some("success"),
        csrf_token,
    };
    Html(template.render().unwrap_or_else(|_| {
        "<html><body><h1>Error rendering login page</h1></body></html>".to_string()
    }))
}

pub async fn login_handler(
    State(app_state): State<AppState>,
    session: Session,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    // Validate CSRF token
    if validate_csrf_form_field(&session, &form.csrf_token)
        .await
        .is_err()
    {
        return login_error(
            "Invalid security token. Please refresh the page and try again.",
            &session,
        )
        .await
        .into_response();
    }

    let request = LoginRequest {
        email: form.email.clone(),
        password: form.password.clone(),
    };

    match app_state.auth_service.authenticate(request).await {
        Ok(user) => {
            // Set session
            if session.insert("user_id", user.id).await.is_err() {
                return login_error("Failed to create session", &session)
                    .await
                    .into_response();
            }
            if session.insert("email", user.email).await.is_err() {
                return login_error("Failed to create session", &session)
                    .await
                    .into_response();
            }
            if session
                .insert("auth_timestamp", chrono::Utc::now().timestamp())
                .await
                .is_err()
            {
                return login_error("Failed to create session", &session)
                    .await
                    .into_response();
            }

            // Set session expiry based on remember_me
            if form.remember_me.unwrap_or(false) {
                session.set_expiry(Some(tower_sessions::Expiry::OnInactivity(
                    time::Duration::days(30),
                )));
            }

            // Check for OAuth return_to URL
            let redirect_url = match session.get::<String>("login_return_to").await {
                Ok(Some(return_to)) => {
                    // Clear the return_to from session
                    let _ = session.remove::<String>("login_return_to").await;
                    return_to
                }
                _ => "/dashboard".to_string(),
            };

            Redirect::to(&redirect_url).into_response()
        }
        Err(err) => {
            let error_msg = match err {
                AuthServiceError::InvalidCredentials => "Invalid email or password",
                AuthServiceError::EmailNotVerified => "Please verify your email before logging in",
                _ => "An error occurred. Please try again.",
            };
            login_error(error_msg, &session).await.into_response()
        }
    }
}

async fn login_error(msg: &str, session: &Session) -> impl IntoResponse {
    let csrf_token = get_or_create_csrf_token(session)
        .await
        .unwrap_or_else(|_| String::from("error"));

    let template = LoginTemplate {
        error: Some(msg.to_string()),
        signup_success: false,
        csrf_token,
    };
    Html(
        template
            .render()
            .unwrap_or_else(|_| format!("<html><body><h1>Error: {}</h1></body></html>", msg)),
    )
    .into_response()
}

pub async fn logout_handler(session: Session) -> impl IntoResponse {
    let _ = session.flush().await;
    Redirect::to("/")
}
