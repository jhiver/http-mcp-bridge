use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Toolkit {
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub description: Option<String>,
    pub visibility: String,
    pub parent_toolkit_id: Option<i64>,
    pub clone_count: i32,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateToolkitForm {
    pub title: String,
    pub description: String,
    pub visibility: String,
    pub csrf_token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateToolkitForm {
    pub title: String,
    pub description: String,
    pub visibility: String,
    pub csrf_token: String,
}

// View model for dashboard display
#[derive(Debug, Clone, Serialize)]
pub struct ToolkitSummary {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub tools_count: i32,
}

impl From<CreateToolkitForm> for CreateToolkitRequest {
    fn from(form: CreateToolkitForm) -> Self {
        CreateToolkitRequest {
            title: form.title.trim().to_string(),
            description: if form.description.trim().is_empty() {
                None
            } else {
                Some(form.description.trim().to_string())
            },
            visibility: form.visibility,
        }
    }
}

impl From<UpdateToolkitForm> for UpdateToolkitRequest {
    fn from(form: UpdateToolkitForm) -> Self {
        UpdateToolkitRequest {
            title: form.title.trim().to_string(),
            description: if form.description.trim().is_empty() {
                None
            } else {
                Some(form.description.trim().to_string())
            },
            visibility: form.visibility,
        }
    }
}

// Service request models
#[derive(Debug, Clone)]
pub struct CreateToolkitRequest {
    pub title: String,
    pub description: Option<String>,
    pub visibility: String,
}

#[derive(Debug, Clone)]
pub struct UpdateToolkitRequest {
    pub title: String,
    pub description: Option<String>,
    pub visibility: String,
}

// Model for displaying public toolkits with statistics
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ToolkitWithStats {
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub description: Option<String>,
    pub visibility: String,
    pub parent_toolkit_id: Option<i64>,
    pub clone_count: i32,
    pub tools_count: i32,
    pub owner_email: String,
    pub created_at: chrono::NaiveDateTime,
}

// Model for public toolkit details page
#[derive(Debug, Clone, Serialize)]
pub struct PublicToolkitDetails {
    pub toolkit: Toolkit,
    pub tools: Vec<crate::models::tool::Tool>,
    pub owner_email: String,
    pub parent_toolkit: Option<ToolkitSummary>,
}

// Request model for cloning a toolkit
#[derive(Debug, Clone, Deserialize)]
pub struct CloneToolkitRequest {
    pub new_title: Option<String>,
    pub csrf_token: String,
}
