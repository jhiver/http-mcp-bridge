use axum::response::{Html, IntoResponse, Redirect};

/// Handler for the tutorial page
/// Redirects to the static tutorial HTML file
pub async fn tutorial_handler() -> impl IntoResponse {
    // Redirect to the static tutorial HTML
    Redirect::permanent("/static/tutorial/index.html")
}

/// Alternative: Serve the HTML directly (optional implementation)
pub async fn tutorial_html_handler() -> Html<String> {
    // Read the tutorial HTML file
    let html_content = include_str!("../../static/tutorial/index.html");
    Html(html_content.to_string())
}
