use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use tower_sessions::Session;

pub async fn require_auth(session: Session, request: Request, next: Next) -> Response {
    // Check if user is logged in
    if let Ok(Some(_user_id)) = session.get::<i64>("user_id").await {
        // User is authenticated, proceed with the request
        next.run(request).await
    } else {
        // User is not authenticated, redirect to login
        Redirect::to("/login").into_response()
    }
}

pub async fn redirect_if_authenticated(session: Session, request: Request, next: Next) -> Response {
    // Check if user is logged in
    if let Ok(Some(_user_id)) = session.get::<i64>("user_id").await {
        // User is already authenticated, redirect to dashboard
        Redirect::to("/dashboard").into_response()
    } else {
        // User is not authenticated, proceed with the request
        next.run(request).await
    }
}
