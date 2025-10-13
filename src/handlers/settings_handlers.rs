use crate::services::user_service::{UpdateEmailRequest, UpdatePasswordRequest, UserServiceError};
use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use serde::Deserialize;
use tower_sessions::Session;

#[derive(Template, WebTemplate)]
#[template(path = "settings.html")]
struct SettingsTemplate {
    user_email: String,
    requires_password: bool,
    success_message: String,
    error_message: String,
}

#[derive(Deserialize)]
pub struct SettingsQuery {
    success: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdatePasswordForm {
    current_password: Option<String>,
    new_password: String,
    new_password_confirm: String,
}

#[derive(Deserialize)]
pub struct UpdateEmailForm {
    new_email: String,
    current_password: Option<String>,
}

async fn requires_reauthentication(session: &Session) -> bool {
    match session.get::<i64>("auth_timestamp").await {
        Ok(Some(timestamp)) => {
            let now = chrono::Utc::now().timestamp();
            let elapsed_seconds = now - timestamp;
            elapsed_seconds > 300
        }
        _ => true,
    }
}

pub async fn show_settings_page(
    session: Session,
    Query(query): Query<SettingsQuery>,
) -> Result<Html<String>, Response> {
    let user_email = match session.get::<String>("email").await {
        Ok(Some(email)) => email,
        _ => return Err(Redirect::to("/").into_response()),
    };

    let requires_password = requires_reauthentication(&session).await;

    let template = SettingsTemplate {
        user_email,
        requires_password,
        success_message: query.success.unwrap_or_default(),
        error_message: query.error.unwrap_or_default(),
    };

    Ok(Html(
        template
            .render()
            .unwrap_or_else(|_| "Template error".to_string()),
    ))
}

pub async fn update_password_handler(
    State(app_state): State<AppState>,
    session: Session,
    Form(form): Form<UpdatePasswordForm>,
) -> Response {
    let user_id = match session.get::<i64>("user_id").await {
        Ok(Some(id)) => id,
        _ => return Redirect::to("/").into_response(),
    };

    if form.new_password != form.new_password_confirm {
        return Redirect::to("/settings?error=Passwords do not match").into_response();
    }

    if requires_reauthentication(&session).await {
        let current_password = match form.current_password {
            Some(ref pwd) if !pwd.is_empty() => pwd,
            _ => {
                return Redirect::to(
                    "/settings?error=Current password is required for security verification",
                )
                .into_response()
            }
        };

        let user = match app_state.user_service.find_user_by_id(user_id).await {
            Ok(Some(user)) => user,
            _ => {
                return Redirect::to("/settings?error=User not found").into_response();
            }
        };

        if !app_state
            .user_service
            .verify_password(current_password, &user.password_hash)
        {
            return Redirect::to("/settings?error=Current password is incorrect").into_response();
        }
    }

    let request = UpdatePasswordRequest {
        user_id,
        new_password: form.new_password,
        new_password_confirm: Some(form.new_password_confirm),
    };

    match app_state.user_service.update_password(request).await {
        Ok(()) => {
            if session
                .insert("auth_timestamp", chrono::Utc::now().timestamp())
                .await
                .is_err()
            {
                return Redirect::to("/settings?error=Failed to update session").into_response();
            }
            Redirect::to("/settings?success=Password updated successfully").into_response()
        }
        Err(err) => {
            let error_msg = match err {
                UserServiceError::WeakPassword => "Password must be at least 8 characters",
                UserServiceError::PasswordMismatch => "Passwords do not match",
                _ => "Failed to update password",
            };
            Redirect::to(&format!("/settings?error={}", error_msg)).into_response()
        }
    }
}

pub async fn update_email_handler(
    State(app_state): State<AppState>,
    session: Session,
    Form(form): Form<UpdateEmailForm>,
) -> Response {
    let user_id = match session.get::<i64>("user_id").await {
        Ok(Some(id)) => id,
        _ => return Redirect::to("/").into_response(),
    };

    if requires_reauthentication(&session).await {
        let current_password = match form.current_password {
            Some(ref pwd) if !pwd.is_empty() => pwd,
            _ => {
                return Redirect::to(
                    "/settings?error=Current password is required for security verification",
                )
                .into_response()
            }
        };

        let user = match app_state.user_service.find_user_by_id(user_id).await {
            Ok(Some(user)) => user,
            _ => {
                return Redirect::to("/settings?error=User not found").into_response();
            }
        };

        if !app_state
            .user_service
            .verify_password(current_password, &user.password_hash)
        {
            return Redirect::to("/settings?error=Current password is incorrect").into_response();
        }
    }

    let request = UpdateEmailRequest {
        user_id,
        new_email: form.new_email.clone(),
    };

    match app_state.user_service.update_email(request).await {
        Ok(()) => {
            if session.insert("email", form.new_email).await.is_err() {
                return Redirect::to("/settings?error=Failed to update session").into_response();
            }
            Redirect::to("/settings?success=Email updated successfully").into_response()
        }
        Err(err) => {
            let error_msg = match err {
                UserServiceError::InvalidEmail => "Invalid email address",
                UserServiceError::EmailTaken => "Email address already in use",
                _ => "Failed to update email",
            };
            Redirect::to(&format!("/settings?error={}", error_msg)).into_response()
        }
    }
}
