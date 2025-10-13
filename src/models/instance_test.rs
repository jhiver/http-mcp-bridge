#[cfg(test)]
mod tests {
    use super::super::instance::{InstanceParam, ToolInstance};
    use crate::test_utils::test_helpers;

    /// Test that get_signature shows ALL exposed parameters from the tool template,
    /// not just those configured in instance_params
    #[tokio::test]
    async fn test_get_signature_shows_all_exposed_params_from_tool() {
        let pool = test_helpers::create_test_db().await.unwrap();

        // Create test data
        let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
            .await
            .unwrap();

        let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
            .await
            .unwrap();

        // Create a tool with 2 parameters in the URL template
        let tool_url = "https://api.nbp.pl/api/exchangerates/rates/c/{{three_letter_iso_currency_code_lowercase}}/{{UTC_ISO_8601_date}}/?format=json";
        let tool_id = test_helpers::create_test_tool(
            &pool,
            toolkit_id,
            "Rate Lookup",
            "GET",
            Some(tool_url),
            Some("{}"),
            None,
            30000,
        )
        .await
        .unwrap();

        let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
            .await
            .unwrap();

        // Create instance with only 1 parameter configured (incomplete configuration)
        let instance_id = sqlx::query!(
            "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
             VALUES (?, ?, 'rate_lookup', 'Currency rate lookup')",
            server_id,
            tool_id
        )
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_rowid();

        // Only configure one parameter as exposed
        sqlx::query!(
            "INSERT INTO instance_params (instance_id, param_name, source, value)
             VALUES (?, ?, 'exposed', NULL)",
            instance_id,
            "three_letter_iso_currency_code_lowercase"
        )
        .execute(&pool)
        .await
        .unwrap();

        // Load instance, tool, and params
        let instance = ToolInstance::get_by_id(&pool, instance_id)
            .await
            .unwrap()
            .unwrap();
        let tool = crate::models::tool::Tool::get_by_id(&pool, tool_id)
            .await
            .unwrap()
            .unwrap();
        let params = InstanceParam::list_by_instance(&pool, instance_id)
            .await
            .unwrap();

        // Get signature
        let signature = instance.get_signature(&tool, &params);

        // EXPECTED: Both parameters should appear in signature because both are in the tool template
        // and neither is bound to instance/server source
        // ACTUAL (before fix): Only shows "rate_lookup(three_letter_iso_currency_code_lowercase)"
        assert!(
            signature.contains("three_letter_iso_currency_code_lowercase"),
            "Signature should include first parameter. Got: {}",
            signature
        );
        assert!(
            signature.contains("UTC_ISO_8601_date"),
            "Signature should include second parameter that's in tool template but not in instance_params. Got: {}",
            signature
        );

        // Verify it's formatted correctly
        assert!(
            signature.starts_with("rate_lookup("),
            "Signature should start with instance name. Got: {}",
            signature
        );
        assert!(
            signature.ends_with(")"),
            "Signature should end with closing paren. Got: {}",
            signature
        );
    }

    /// Test that get_signature excludes parameters bound to instance or server sources
    #[tokio::test]
    async fn test_get_signature_excludes_non_exposed_params() {
        let pool = test_helpers::create_test_db().await.unwrap();

        // Create test data
        let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
            .await
            .unwrap();

        let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
            .await
            .unwrap();

        // Create a tool with 3 parameters
        let tool_url =
            "https://api.example.com/{{string:resource}}/{{integer:id}}?api_key={{string:api_key}}";
        let tool_id = test_helpers::create_test_tool(
            &pool,
            toolkit_id,
            "Get Resource",
            "GET",
            Some(tool_url),
            Some("{}"),
            None,
            30000,
        )
        .await
        .unwrap();

        let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
            .await
            .unwrap();

        // Create instance
        let instance_id = sqlx::query!(
            "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
             VALUES (?, ?, 'get_resource', 'Get a resource')",
            server_id,
            tool_id
        )
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_rowid();

        // Configure parameters:
        // - resource: instance-bound (should NOT appear in signature)
        // - id: exposed (should appear in signature)
        // - api_key: server-bound (should NOT appear in signature)
        sqlx::query!(
            "INSERT INTO instance_params (instance_id, param_name, source, value)
             VALUES (?, 'resource', 'instance', 'users')",
            instance_id
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query!(
            "INSERT INTO instance_params (instance_id, param_name, source, value)
             VALUES (?, 'id', 'exposed', NULL)",
            instance_id
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query!(
            "INSERT INTO instance_params (instance_id, param_name, source, value)
             VALUES (?, 'api_key', 'server', NULL)",
            instance_id
        )
        .execute(&pool)
        .await
        .unwrap();

        // Load instance, tool, and params
        let instance = ToolInstance::get_by_id(&pool, instance_id)
            .await
            .unwrap()
            .unwrap();
        let tool = crate::models::tool::Tool::get_by_id(&pool, tool_id)
            .await
            .unwrap()
            .unwrap();
        let params = InstanceParam::list_by_instance(&pool, instance_id)
            .await
            .unwrap();

        // Get signature
        let signature = instance.get_signature(&tool, &params);

        // Should only show 'id' parameter
        assert_eq!(signature, "get_resource(id)");
    }

    /// Test signature with no exposed parameters
    #[tokio::test]
    async fn test_get_signature_no_exposed_params() {
        let pool = test_helpers::create_test_db().await.unwrap();

        // Create test data
        let user_id = test_helpers::insert_test_user(&pool, "test@example.com", "password", true)
            .await
            .unwrap();

        let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit")
            .await
            .unwrap();

        // Create a tool with 2 parameters
        let tool_url = "https://api.example.com/{{string:resource}}?api_key={{string:api_key}}";
        let tool_id = test_helpers::create_test_tool(
            &pool,
            toolkit_id,
            "Get Resource",
            "GET",
            Some(tool_url),
            Some("{}"),
            None,
            30000,
        )
        .await
        .unwrap();

        let (server_id, _) = test_helpers::create_test_server(&pool, user_id, "Test Server", None)
            .await
            .unwrap();

        // Create instance
        let instance_id = sqlx::query!(
            "INSERT INTO tool_instances (server_id, tool_id, instance_name, description)
             VALUES (?, ?, 'get_resource', 'Get a resource')",
            server_id,
            tool_id
        )
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_rowid();

        // Configure both parameters as non-exposed (instance-bound and server-bound)
        sqlx::query!(
            "INSERT INTO instance_params (instance_id, param_name, source, value)
             VALUES (?, 'resource', 'instance', 'users')",
            instance_id
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query!(
            "INSERT INTO instance_params (instance_id, param_name, source, value)
             VALUES (?, 'api_key', 'server', NULL)",
            instance_id
        )
        .execute(&pool)
        .await
        .unwrap();

        // Load instance, tool, and params
        let instance = ToolInstance::get_by_id(&pool, instance_id)
            .await
            .unwrap()
            .unwrap();
        let tool = crate::models::tool::Tool::get_by_id(&pool, tool_id)
            .await
            .unwrap()
            .unwrap();
        let params = InstanceParam::list_by_instance(&pool, instance_id)
            .await
            .unwrap();

        // Get signature
        let signature = instance.get_signature(&tool, &params);

        // Should have empty parameter list
        assert_eq!(signature, "get_resource()");
    }
}
