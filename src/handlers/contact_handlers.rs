use crate::AppState;
use askama::Template;
use askama_web::WebTemplate;
use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Json, Redirect, Response},
    Form,
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

#[derive(Template, WebTemplate)]
#[template(path = "contact.html")]
struct ContactTemplate {
    success_message: String,
    error_message: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "contact_authenticated.html")]
struct ContactAuthenticatedTemplate {
    user_email: String,
    success_message: String,
    error_message: String,
}

#[derive(Deserialize)]
pub struct ContactQuery {
    success: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
pub struct ContactForm {
    pub email: String,
    pub name: Option<String>,
    pub message: String,
}

#[derive(Serialize)]
struct JsonResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn is_valid_email(email: &str) -> bool {
    email.contains('@') && email.len() > 3
}

pub async fn show_contact_form(
    session: Session,
    Query(query): Query<ContactQuery>,
) -> Result<Html<String>, Response> {
    let user_id = session.get::<i64>("user_id").await.ok().flatten();
    let user_email = session.get::<String>("email").await.ok().flatten();

    let success_message = query.success.unwrap_or_default();
    let error_message = query.error.unwrap_or_default();

    if user_id.is_some() {
        let template = ContactAuthenticatedTemplate {
            user_email: user_email.unwrap_or_default(),
            success_message,
            error_message,
        };
        Ok(Html(
            template
                .render()
                .unwrap_or_else(|_| "Template error".to_string()),
        ))
    } else {
        let template = ContactTemplate {
            success_message,
            error_message,
        };
        Ok(Html(
            template
                .render()
                .unwrap_or_else(|_| "Template error".to_string()),
        ))
    }
}

pub async fn submit_contact_form(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ContactForm>,
) -> Response {
    // Check if this is an AJAX request
    let is_ajax = headers
        .get("X-Requested-With")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "XMLHttpRequest")
        .unwrap_or(false);

    // Validate email
    if form.email.trim().is_empty() {
        return if is_ajax {
            Json(JsonResponse {
                success: false,
                message: None,
                error: Some("Email is required".to_string()),
            })
            .into_response()
        } else {
            Redirect::to("/contact?error=Email is required").into_response()
        };
    }

    if !is_valid_email(&form.email) {
        return if is_ajax {
            Json(JsonResponse {
                success: false,
                message: None,
                error: Some("Invalid email address".to_string()),
            })
            .into_response()
        } else {
            Redirect::to("/contact?error=Invalid email address").into_response()
        };
    }

    if form.message.trim().is_empty() {
        return if is_ajax {
            Json(JsonResponse {
                success: false,
                message: None,
                error: Some("Message is required".to_string()),
            })
            .into_response()
        } else {
            Redirect::to("/contact?error=Message is required").into_response()
        };
    }

    let email_service = app_state.auth_token_service.email_service();
    let name = form.name.as_deref().filter(|n| !n.trim().is_empty());

    match email_service
        .send_contact_form(&form.email, name, &form.message)
        .await
    {
        Ok(()) => {
            if is_ajax {
                Json(JsonResponse {
                    success: true,
                    message: Some(
                        "Thank you for contacting us! We will get back to you soon.".to_string(),
                    ),
                    error: None,
                })
                .into_response()
            } else {
                Redirect::to(
                    "/contact?success=Thank you for contacting us! We will get back to you soon.",
                )
                .into_response()
            }
        }
        Err(_) => {
            if is_ajax {
                Json(JsonResponse {
                    success: false,
                    message: None,
                    error: Some("Failed to send message. Please try again later.".to_string()),
                })
                .into_response()
            } else {
                Redirect::to("/contact?error=Failed to send message. Please try again later.")
                    .into_response()
            }
        }
    }
}
