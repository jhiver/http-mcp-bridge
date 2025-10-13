use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

// Hardcoded regex pattern - guaranteed to be valid at compile time
// Using unwrap here is safe because the pattern is a compile-time constant
#[allow(clippy::unwrap_used)]
static TYPED_VARIABLE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\{\{(?:([a-z]+):)?([a-zA-Z_][a-zA-Z0-9_]*)\}\}").unwrap());

#[derive(Debug, Clone)]
pub enum VariableType {
    String,
    Number,
    Integer,
    Boolean,
    Json,
    Url,
}

impl VariableType {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "number" => Self::Number,
            "integer" => Self::Integer,
            "boolean" | "bool" => Self::Boolean,
            "json" => Self::Json,
            "url" => Self::Url,
            _ => Self::String,
        }
    }

    pub fn cast(&self, value: &str) -> Result<Value> {
        match self {
            Self::String => Ok(Value::String(value.to_string())),

            Self::Number => {
                let n = value
                    .parse::<f64>()
                    .map_err(|_| anyhow!("Cannot parse '{}' as number", value))?;
                let num = serde_json::Number::from_f64(n)
                    .ok_or_else(|| anyhow!("Number '{}' is not finite (NaN or Infinite)", n))?;
                Ok(Value::Number(num))
            }

            Self::Integer => value
                .parse::<i64>()
                .map(|i| Value::Number(i.into()))
                .map_err(|_| anyhow!("Cannot parse '{}' as integer", value)),

            Self::Boolean => match value.to_lowercase().as_str() {
                "true" | "1" | "yes" => Ok(Value::Bool(true)),
                "false" | "0" | "no" => Ok(Value::Bool(false)),
                _ => Err(anyhow!("Cannot parse '{}' as boolean", value)),
            },

            Self::Json => {
                serde_json::from_str(value).map_err(|e| anyhow!("Cannot parse as JSON: {}", e))
            }

            Self::Url => {
                // Basic URL validation
                if value.starts_with("http://") || value.starts_with("https://") {
                    Ok(Value::String(value.to_string()))
                } else {
                    Err(anyhow!("Invalid URL: {}", value))
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct TypedVariableEngine {
    pattern: &'static Regex,
}

impl Default for TypedVariableEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TypedVariableEngine {
    pub fn new() -> Self {
        Self {
            // Match {{type:name}} or {{name}}
            pattern: &TYPED_VARIABLE_PATTERN,
        }
    }

    pub fn substitute(&self, template: &str, context: &HashMap<String, String>) -> Result<String> {
        let mut result = template.to_string();
        let mut errors = Vec::new();

        // Find all variables and replace them
        for cap in self.pattern.captures_iter(template) {
            let full_match = &cap[0];
            let type_str = cap.get(1).map(|m| m.as_str());
            let var_name = &cap[2];

            let var_type = type_str
                .map(VariableType::from_str)
                .unwrap_or(VariableType::String);

            match context.get(var_name) {
                Some(value) => match var_type.cast(value) {
                    Ok(casted) => {
                        let replacement = match casted {
                            Value::String(s) => s,
                            Value::Number(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            v => v.to_string(),
                        };
                        result = result.replace(full_match, &replacement);
                    }
                    Err(e) => {
                        errors.push(format!("Variable '{}': {}", var_name, e));
                    }
                },
                None => {
                    errors.push(format!("Variable '{}' not found", var_name));
                }
            }
        }

        if errors.is_empty() {
            Ok(result)
        } else {
            Err(anyhow!(
                "Variable substitution errors: {}",
                errors.join(", ")
            ))
        }
    }

    pub fn substitute_json(
        &self,
        template: &str,
        context: &HashMap<String, String>,
    ) -> Result<Value> {
        // First do string substitution
        let substituted = self.substitute(template, context)?;

        // Then parse as JSON if it looks like JSON
        if substituted.starts_with('{') || substituted.starts_with('[') {
            serde_json::from_str(&substituted)
                .map_err(|e| anyhow!("Failed to parse as JSON after substitution: {}", e))
        } else {
            Ok(Value::String(substituted))
        }
    }

    pub fn find_variables(&self, template: &str) -> Vec<(Option<String>, String)> {
        let mut vars = Vec::new();
        for cap in self.pattern.captures_iter(template) {
            let type_str = cap.get(1).map(|m| m.as_str().to_string());
            let var_name = cap[2].to_string();
            vars.push((type_str, var_name));
        }
        vars
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_substitution() {
        let engine = TypedVariableEngine::new();
        let mut context = HashMap::new();
        context.insert("name".to_string(), "Sarah".to_string());
        context.insert("city".to_string(), "New York".to_string());

        let template = "Hello {{name}}, welcome to {{city}}!";
        let result = engine.substitute(template, &context).unwrap();
        assert_eq!(result, "Hello Sarah, welcome to New York!");
    }

    #[test]
    fn test_typed_substitution() {
        let engine = TypedVariableEngine::new();
        let mut context = HashMap::new();
        context.insert("port".to_string(), "8080".to_string());
        context.insert("ssl".to_string(), "true".to_string());
        context.insert("timeout".to_string(), "5.5".to_string());

        let template = "Port: {{integer:port}}, SSL: {{boolean:ssl}}, Timeout: {{number:timeout}}s";
        let result = engine.substitute(template, &context).unwrap();
        assert_eq!(result, "Port: 8080, SSL: true, Timeout: 5.5s");
    }

    #[test]
    fn test_type_casting_error() {
        let engine = TypedVariableEngine::new();
        let mut context = HashMap::new();
        context.insert("port".to_string(), "not_a_number".to_string());

        let template = "Port: {{integer:port}}";
        let result = engine.substitute(template, &context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cannot parse"));
    }

    #[test]
    fn test_missing_variable() {
        let engine = TypedVariableEngine::new();
        let context = HashMap::new();

        let template = "Hello {{name}}!";
        let result = engine.substitute(template, &context);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_json_substitution() {
        let engine = TypedVariableEngine::new();
        let mut context = HashMap::new();
        context.insert("port".to_string(), "8080".to_string());
        context.insert("enabled".to_string(), "true".to_string());

        let template = r#"{"port": {{integer:port}}, "ssl": {{boolean:enabled}}}"#;
        let result = engine.substitute(template, &context).unwrap();
        assert_eq!(result, r#"{"port": 8080, "ssl": true}"#);
    }

    #[test]
    fn test_find_variables() {
        let engine = TypedVariableEngine::new();
        let template = "{{name}} lives at {{url:api_base}}/users/{{integer:id}}";
        let vars = engine.find_variables(template);

        assert_eq!(vars.len(), 3);
        assert_eq!(vars[0], (None, "name".to_string()));
        assert_eq!(vars[1], (Some("url".to_string()), "api_base".to_string()));
        assert_eq!(vars[2], (Some("integer".to_string()), "id".to_string()));
    }
}
