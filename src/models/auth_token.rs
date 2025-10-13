use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PendingRegistration {
    pub id: i64,
    pub email: String,
    pub password_hash: Option<String>,
    pub token: String,
    pub expires_at: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MagicLoginToken {
    pub id: i64,
    pub user_id: i64,
    pub token: String,
    pub expires_at: String,
    pub created_at: Option<String>,
    pub used_at: Option<String>,
}
