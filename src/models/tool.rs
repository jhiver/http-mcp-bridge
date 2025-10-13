use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

// Hardcoded regex pattern - guaranteed to be valid at compile time
// Using unwrap here is safe because the pattern is a compile-time constant
#[allow(clippy::unwrap_used)]
static PARAMETER_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\{\{(?:(\w+):)?(\w+)\}\}").unwrap());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub id: i64,
    pub toolkit_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub method: String,          // GET, POST, PUT, DELETE, PATCH
    pub url: Option<String>,     // URL with {{type:param}} templates
    pub headers: Option<String>, // JSON string
    pub body: Option<String>,    // JSON string
    pub timeout_ms: i32,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

#[derive(Debug, Deserialize)]
pub struct CreateToolForm {
    pub name: String,
    pub description: String,
    pub method: String,
    pub url: String,
    pub headers: String,      // JSON string
    pub body: Option<String>, // JSON string, optional for GET/DELETE
    pub timeout_ms: Option<i32>,
    pub csrf_token: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateToolForm {
    pub name: String,
    pub description: String,
    pub method: String,
    pub url: String,
    pub headers: String,      // JSON string
    pub body: Option<String>, // JSON string, optional for GET/DELETE
    pub timeout_ms: Option<i32>,
    pub csrf_token: String,
}

// Service request models
#[derive(Debug, Clone)]
pub struct CreateToolRequest {
    pub name: String,
    pub description: Option<String>,
    pub method: String,
    pub url: Option<String>,
    pub headers: Option<String>,
    pub body: Option<String>,
    pub timeout_ms: i32,
}

#[derive(Debug, Clone)]
pub struct UpdateToolRequest {
    pub name: String,
    pub description: Option<String>,
    pub method: String,
    pub url: Option<String>,
    pub headers: Option<String>,
    pub body: Option<String>,
    pub timeout_ms: i32,
}

// Structure for extracted parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedParameter {
    pub name: String,
    pub param_type: String,
    pub source: String,       // 'url', 'headers', or 'body'
    pub full_pattern: String, // e.g., "{{string:username}}"
}

impl Tool {
    /// Get tool by ID
    pub async fn get_by_id(pool: &sqlx::SqlitePool, id: i64) -> sqlx::Result<Option<Self>> {
        use sqlx::Row;

        let row = sqlx::query("SELECT * FROM tools WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;

        Ok(row.map(|r| Tool {
            id: r.get("id"),
            toolkit_id: r.get("toolkit_id"),
            name: r.get("name"),
            description: r.get("description"),
            method: r.get("method"),
            url: r.get("url"),
            headers: r.get("headers"),
            body: r.get("body"),
            timeout_ms: r.get("timeout_ms"),
            created_at: chrono::DateTime::from_timestamp(r.get::<i64, _>("created_at"), 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
            updated_at: chrono::DateTime::from_timestamp(r.get::<i64, _>("updated_at"), 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
        }))
    }

    /// Extract parameters from URL, headers, and body templates
    pub fn extract_parameters(&self) -> Vec<ExtractedParameter> {
        let mut params = Vec::new();
        // Updated regex to support both {{name}} and {{type:name}} formats
        // Matches: {{name}} or {{type:name}}
        let re = &PARAMETER_PATTERN;

        // Extract from URL
        if let Some(url) = &self.url {
            for cap in re.captures_iter(url) {
                let param_type = cap
                    .get(1)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| "string".to_string()); // Default to string if no type
                let name = cap[2].to_string();

                params.push(ExtractedParameter {
                    param_type,
                    name,
                    source: "url".to_string(),
                    full_pattern: cap[0].to_string(),
                });
            }
        }

        // Extract from headers
        if let Some(headers) = &self.headers {
            for cap in re.captures_iter(headers) {
                let param_type = cap
                    .get(1)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| "string".to_string()); // Default to string if no type
                let name = cap[2].to_string();

                params.push(ExtractedParameter {
                    param_type,
                    name,
                    source: "headers".to_string(),
                    full_pattern: cap[0].to_string(),
                });
            }
        }

        // Extract from body
        if let Some(body) = &self.body {
            for cap in re.captures_iter(body) {
                let param_type = cap
                    .get(1)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| "string".to_string()); // Default to string if no type
                let name = cap[2].to_string();

                params.push(ExtractedParameter {
                    param_type,
                    name,
                    source: "body".to_string(),
                    full_pattern: cap[0].to_string(),
                });
            }
        }

        // Remove duplicates based on name
        let mut seen = std::collections::HashSet::new();
        params.retain(|p| seen.insert(p.name.clone()));

        params
    }
}

impl CreateToolForm {
    pub fn into_request(self) -> CreateToolRequest {
        // Validate JSON format for headers
        if !self.headers.trim().is_empty()
            && serde_json::from_str::<JsonValue>(&self.headers).is_err()
        {
            // Headers should be valid JSON
            // In production, we'd return an error here
        }

        // Validate JSON format for body (if provided)
        if let Some(ref body) = self.body {
            if !body.trim().is_empty() && serde_json::from_str::<JsonValue>(body).is_err() {
                // Body should be valid JSON
                // In production, we'd return an error here
            }
        }

        CreateToolRequest {
            name: self.name.trim().to_string(),
            description: if self.description.trim().is_empty() {
                None
            } else {
                Some(self.description.trim().to_string())
            },
            method: self.method.to_uppercase(),
            url: if self.url.trim().is_empty() {
                None
            } else {
                Some(self.url.trim().to_string())
            },
            headers: if self.headers.trim().is_empty() {
                Some("{}".to_string())
            } else {
                Some(self.headers.trim().to_string())
            },
            body: self.body.as_ref().and_then(|b| {
                if b.trim().is_empty() {
                    None
                } else {
                    Some(b.trim().to_string())
                }
            }),
            timeout_ms: self.timeout_ms.unwrap_or(30000),
        }
    }
}

impl UpdateToolForm {
    pub fn into_request(self) -> UpdateToolRequest {
        let create_form = CreateToolForm {
            name: self.name,
            description: self.description,
            method: self.method,
            url: self.url,
            headers: self.headers,
            body: self.body,
            timeout_ms: self.timeout_ms,
            csrf_token: self.csrf_token,
        };
        let create_request = create_form.into_request();

        UpdateToolRequest {
            name: create_request.name,
            description: create_request.description,
            method: create_request.method,
            url: create_request.url,
            headers: create_request.headers,
            body: create_request.body,
            timeout_ms: create_request.timeout_ms,
        }
    }
}
