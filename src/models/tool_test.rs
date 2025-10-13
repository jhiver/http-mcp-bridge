#[cfg(test)]
mod tests {
    use super::super::tool::Tool;

    #[test]
    fn test_extract_parameters_from_url() {
        let tool = Tool {
            id: 1,
            toolkit_id: 1,
            name: "Test Tool".to_string(),
            description: None,
            method: "GET".to_string(),
            url: Some(
                "https://api.example.com/users/{{integer:user_id}}/posts?limit={{integer:limit}}"
                    .to_string(),
            ),
            headers: None,
            body: None,
            timeout_ms: 30000,
            created_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
            updated_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
        };

        let params = tool.extract_parameters();
        assert_eq!(params.len(), 2);

        let user_id_param = params.iter().find(|p| p.name == "user_id").unwrap();
        assert_eq!(user_id_param.param_type, "integer");
        assert_eq!(user_id_param.source, "url");
        assert_eq!(user_id_param.full_pattern, "{{integer:user_id}}");

        let limit_param = params.iter().find(|p| p.name == "limit").unwrap();
        assert_eq!(limit_param.param_type, "integer");
        assert_eq!(limit_param.source, "url");
        assert_eq!(limit_param.full_pattern, "{{integer:limit}}");
    }

    #[test]
    fn test_extract_parameters_from_headers() {
        let tool = Tool {
            id: 1,
            toolkit_id: 1,
            name: "Test Tool".to_string(),
            description: None,
            method: "GET".to_string(),
            url: Some("https://api.example.com/data".to_string()),
            headers: Some(r#"{"Authorization": "Bearer {{string:token}}", "X-API-Version": "{{string:version}}"}"#.to_string()),
            body: None,
            timeout_ms: 30000,
            created_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
            updated_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
        };

        let params = tool.extract_parameters();
        assert_eq!(params.len(), 2);

        let token_param = params.iter().find(|p| p.name == "token").unwrap();
        assert_eq!(token_param.param_type, "string");
        assert_eq!(token_param.source, "headers");

        let version_param = params.iter().find(|p| p.name == "version").unwrap();
        assert_eq!(version_param.param_type, "string");
        assert_eq!(version_param.source, "headers");
    }

    #[test]
    fn test_extract_parameters_from_body() {
        let tool = Tool {
            id: 1,
            toolkit_id: 1,
            name: "Test Tool".to_string(),
            description: None,
            method: "POST".to_string(),
            url: Some("https://api.example.com/users".to_string()),
            headers: Some("{}".to_string()),
            body: Some(r#"{"name": "{{string:username}}", "age": {{integer:age}}, "active": {{boolean:is_active}}}"#.to_string()),
            timeout_ms: 30000,
            created_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
            updated_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
        };

        let params = tool.extract_parameters();
        assert_eq!(params.len(), 3);

        let username_param = params.iter().find(|p| p.name == "username").unwrap();
        assert_eq!(username_param.param_type, "string");
        assert_eq!(username_param.source, "body");

        let age_param = params.iter().find(|p| p.name == "age").unwrap();
        assert_eq!(age_param.param_type, "integer");
        assert_eq!(age_param.source, "body");

        let active_param = params.iter().find(|p| p.name == "is_active").unwrap();
        assert_eq!(active_param.param_type, "boolean");
        assert_eq!(active_param.source, "body");
    }

    #[test]
    fn test_extract_parameters_deduplication() {
        // Test that parameters with same name from different sources are deduplicated
        let tool = Tool {
            id: 1,
            toolkit_id: 1,
            name: "Test Tool".to_string(),
            description: None,
            method: "POST".to_string(),
            url: Some("https://api.example.com/{{string:api_key}}".to_string()),
            headers: Some(r#"{"X-API-Key": "{{string:api_key}}"}"#.to_string()),
            body: Some(r#"{"key": "{{string:api_key}}"}"#.to_string()),
            timeout_ms: 30000,
            created_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
            updated_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
        };

        let params = tool.extract_parameters();
        assert_eq!(params.len(), 1); // Should be deduplicated

        let api_key_param = &params[0];
        assert_eq!(api_key_param.name, "api_key");
        assert_eq!(api_key_param.param_type, "string");
        assert_eq!(api_key_param.source, "url"); // First occurrence wins
    }

    #[test]
    fn test_extract_parameters_with_different_types() {
        let tool = Tool {
            id: 1,
            toolkit_id: 1,
            name: "Test Tool".to_string(),
            description: None,
            method: "POST".to_string(),
            url: Some("https://api.example.com/data".to_string()),
            headers: None,
            body: Some(
                r#"{
                "string_param": "{{string:text}}",
                "integer_param": {{integer:count}},
                "number_param": {{number:price}},
                "boolean_param": {{boolean:enabled}}
            }"#
                .to_string(),
            ),
            timeout_ms: 30000,
            created_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
            updated_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
        };

        let params = tool.extract_parameters();
        assert_eq!(params.len(), 4);

        assert!(params
            .iter()
            .any(|p| p.name == "text" && p.param_type == "string"));
        assert!(params
            .iter()
            .any(|p| p.name == "count" && p.param_type == "integer"));
        assert!(params
            .iter()
            .any(|p| p.name == "price" && p.param_type == "number"));
        assert!(params
            .iter()
            .any(|p| p.name == "enabled" && p.param_type == "boolean"));
    }

    #[test]
    fn test_extract_no_parameters() {
        let tool = Tool {
            id: 1,
            toolkit_id: 1,
            name: "Test Tool".to_string(),
            description: None,
            method: "GET".to_string(),
            url: Some("https://api.example.com/health".to_string()),
            headers: Some(r#"{"Content-Type": "application/json"}"#.to_string()),
            body: None,
            timeout_ms: 30000,
            created_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
            updated_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
        };

        let params = tool.extract_parameters();
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_extract_parameters_without_type_prefix() {
        // Test that {{name}} without type defaults to string type
        let tool = Tool {
            id: 1,
            toolkit_id: 1,
            name: "Test Tool".to_string(),
            description: None,
            method: "GET".to_string(),
            url: Some("https://api.example.com/search?q={{search}}&limit={{limit}}".to_string()),
            headers: Some(r#"{"Authorization": "Bearer {{token}}"}"#.to_string()),
            body: None,
            timeout_ms: 30000,
            created_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
            updated_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
        };

        let params = tool.extract_parameters();
        assert_eq!(params.len(), 3);

        let search_param = params.iter().find(|p| p.name == "search").unwrap();
        assert_eq!(search_param.param_type, "string"); // Should default to string
        assert_eq!(search_param.source, "url");
        assert_eq!(search_param.full_pattern, "{{search}}");

        let limit_param = params.iter().find(|p| p.name == "limit").unwrap();
        assert_eq!(limit_param.param_type, "string"); // Should default to string
        assert_eq!(limit_param.source, "url");
        assert_eq!(limit_param.full_pattern, "{{limit}}");

        let token_param = params.iter().find(|p| p.name == "token").unwrap();
        assert_eq!(token_param.param_type, "string"); // Should default to string
        assert_eq!(token_param.source, "headers");
        assert_eq!(token_param.full_pattern, "{{token}}");
    }

    #[test]
    fn test_extract_parameters_mixed_formats() {
        // Test mixing {{name}} and {{type:name}} formats
        let tool = Tool {
            id: 1,
            toolkit_id: 1,
            name: "Test Tool".to_string(),
            description: None,
            method: "POST".to_string(),
            url: Some("https://api.example.com/users/{{user_id}}/posts".to_string()),
            headers: Some(r#"{"X-User-Id": "{{integer:user_id}}"}"#.to_string()),
            body: Some(r#"{"title": "{{title}}", "content": "{{string:content}}", "published": {{boolean:is_published}}}"#.to_string()),
            timeout_ms: 30000,
            created_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
            updated_at: chrono::DateTime::from_timestamp(0, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_default(),
        };

        let params = tool.extract_parameters();
        // user_id appears in both URL and headers, should be deduplicated
        assert_eq!(params.len(), 4);

        let user_id_param = params.iter().find(|p| p.name == "user_id").unwrap();
        assert_eq!(user_id_param.param_type, "string"); // From URL, no type specified, defaults to string
        assert_eq!(user_id_param.source, "url");

        let title_param = params.iter().find(|p| p.name == "title").unwrap();
        assert_eq!(title_param.param_type, "string"); // No type specified, defaults to string
        assert_eq!(title_param.source, "body");

        let content_param = params.iter().find(|p| p.name == "content").unwrap();
        assert_eq!(content_param.param_type, "string"); // Explicitly specified as string
        assert_eq!(content_param.source, "body");

        let published_param = params.iter().find(|p| p.name == "is_published").unwrap();
        assert_eq!(published_param.param_type, "boolean"); // Explicitly specified as boolean
        assert_eq!(published_param.source, "body");
    }
}
