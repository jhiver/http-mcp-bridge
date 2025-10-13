use saramcp::{
    models::{ConfigureInstanceForm, GlobalsForm},
    repositories::{
        tool_repository::SqliteToolRepository, toolkit_repository::SqliteToolkitRepository,
        user_repository::SqliteUserRepository,
    },
    services::{
        auth_service::AuthService, auth_token_service::AuthTokenService,
        instance_service::InstanceService, server_service::ServerService,
        tool_service::ToolService, toolkit_service::ToolkitService, user_service::UserService,
        OAuthService, SecretsManager,
    },
    test_utils::test_helpers,
    AppState,
};
use std::sync::Arc;

/// Helper to setup complete test environment with all services
async fn setup_full_test_environment() -> anyhow::Result<(AppState, i64, i64, i64)> {
    let pool = test_helpers::create_test_db().await?;

    // Create user
    let user_id =
        test_helpers::insert_test_user(&pool, "test@example.com", "password", true).await?;

    // Create toolkit
    let toolkit_id = test_helpers::create_test_toolkit(&pool, user_id, "Test Toolkit").await?;

    // Create server
    let server_id = sqlx::query!(
        "INSERT INTO servers (user_id, name, description) VALUES (?, ?, ?)",
        user_id,
        "Test Server",
        "Test Description"
    )
    .execute(&pool)
    .await?
    .last_insert_rowid();

    // Setup services
    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let user_service = Arc::new(UserService::new(user_repository.clone()));
    let auth_service = Arc::new(AuthService::new(user_repository.clone()));

    let email_service = saramcp::services::create_email_service();
    let auth_token_service = Arc::new(AuthTokenService::new(
        pool.clone(),
        email_service,
        user_repository.clone(),
        user_service.clone(),
    ));

    let toolkit_repository = Arc::new(SqliteToolkitRepository::new(pool.clone()));
    let tool_repository = Arc::new(SqliteToolRepository::new(pool.clone()));

    let toolkit_service = Some(Arc::new(ToolkitService::new(
        toolkit_repository.clone() as Arc<dyn saramcp::repositories::ToolkitRepository>,
        tool_repository.clone() as Arc<dyn saramcp::repositories::ToolRepository>,
    )));

    let tool_service = Some(Arc::new(ToolService::new(
        tool_repository.clone() as Arc<dyn saramcp::repositories::ToolRepository>,
        toolkit_repository.clone() as Arc<dyn saramcp::repositories::ToolkitRepository>,
    )));

    let secrets = SecretsManager::new()?;
    let server_service = Some(Arc::new(ServerService::new(pool.clone(), secrets.clone())));
    let instance_service = Some(Arc::new(InstanceService::new(pool.clone(), secrets)));
    let oauth_service = Arc::new(OAuthService::new(pool.clone()));

    let state = AppState {
        user_service,
        auth_service,
        auth_token_service,
        toolkit_service,
        tool_service,
        server_service,
        instance_service,
        oauth_service,
        toolkit_repository: Some(
            toolkit_repository as Arc<dyn saramcp::repositories::ToolkitRepository>,
        ),
        tool_repository: Some(tool_repository as Arc<dyn saramcp::repositories::ToolRepository>),
        mcp_registry: None,
        pool: pool.clone(),
    };

    Ok((state, user_id, toolkit_id, server_id))
}

async fn create_test_tool(
    pool: &sqlx::SqlitePool,
    toolkit_id: i64,
    name: &str,
    url: &str,
) -> anyhow::Result<i64> {
    let tool_id = sqlx::query!(
        r#"
        INSERT INTO tools (toolkit_id, name, description, method, url, headers, timeout_ms)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
        toolkit_id,
        name,
        "Test tool",
        "POST",
        url,
        "{}",
        30000
    )
    .execute(pool)
    .await?
    .last_insert_rowid();

    Ok(tool_id)
}

#[tokio::test]
async fn test_view_bindings_tab_with_discovered_parameters() -> anyhow::Result<()> {
    let (state, _user_id, toolkit_id, server_id) = setup_full_test_environment().await?;

    // Create tools with parameters
    let tool_id_1 = create_test_tool(
        &state.pool,
        toolkit_id,
        "tool_1",
        "https://api.example.com/{{string:api_key}}",
    )
    .await?;

    let tool_id_2 = create_test_tool(
        &state.pool,
        toolkit_id,
        "tool_2",
        "https://api.example.com/user/{{string:api_key}}/{{string:user_id}}",
    )
    .await?;

    // Create instances
    let instance_service = state.instance_service.as_ref().unwrap();

    let form1 = ConfigureInstanceForm {
        instance_name: "instance_1".to_string(),
        description: None,
        tool_id: tool_id_1,
        param_configs: vec![],
        csrf_token: "test".to_string(),
    };
    instance_service
        .create_instance_with_config(server_id, form1)
        .await?;

    let form2 = ConfigureInstanceForm {
        instance_name: "instance_2".to_string(),
        description: None,
        tool_id: tool_id_2,
        param_configs: vec![],
        csrf_token: "test".to_string(),
    };
    instance_service
        .create_instance_with_config(server_id, form2)
        .await?;

    // Get discovered parameters
    let discovered = instance_service
        .discover_parameters_with_usage(server_id)
        .await?;

    assert_eq!(discovered.len(), 2, "Should discover 2 unique parameters");

    let api_key_param = discovered
        .iter()
        .find(|p| p.param_name == "api_key")
        .expect("api_key should be discovered");
    assert_eq!(api_key_param.usage_count, 2, "api_key used in 2 instances");

    let user_id_param = discovered
        .iter()
        .find(|p| p.param_name == "user_id")
        .expect("user_id should be discovered");
    assert_eq!(user_id_param.usage_count, 1, "user_id used in 1 instance");

    Ok(())
}

#[tokio::test]
async fn test_save_bindings_with_variables() -> anyhow::Result<()> {
    let (state, _user_id, toolkit_id, server_id) = setup_full_test_environment().await?;

    // Create tool with parameter
    let tool_id = create_test_tool(
        &state.pool,
        toolkit_id,
        "test_tool",
        "https://api.example.com/{{string:api_url}}",
    )
    .await?;

    // Create instance
    let instance_service = state.instance_service.as_ref().unwrap();
    let form = ConfigureInstanceForm {
        instance_name: "test_instance".to_string(),
        description: None,
        tool_id,
        param_configs: vec![],
        csrf_token: "test".to_string(),
    };
    instance_service
        .create_instance_with_config(server_id, form)
        .await?;

    // Save bindings with variable
    let server_service = state.server_service.as_ref().unwrap();
    let globals_form = GlobalsForm {
        var_keys: vec!["api_url".to_string()],
        var_values: vec!["https://production.example.com".to_string()],
        secret_keys: vec![],
        secret_values: vec![],
        csrf_token: "test".to_string(),
    };

    server_service
        .save_server_globals(server_id, _user_id, globals_form)
        .await?;

    // Verify the binding was saved
    let globals = server_service.get_server_globals(server_id).await?;

    assert_eq!(globals.len(), 1, "Should have one global");
    assert_eq!(globals[0].key, "api_url");
    assert_eq!(globals[0].value, "https://production.example.com");
    assert!(
        !globals[0].is_secret.unwrap_or(false),
        "Should not be a secret"
    );

    Ok(())
}

#[tokio::test]
async fn test_save_bindings_with_secrets() -> anyhow::Result<()> {
    let (state, _user_id, toolkit_id, server_id) = setup_full_test_environment().await?;

    // Create tool with parameter
    let tool_id = create_test_tool(
        &state.pool,
        toolkit_id,
        "test_tool",
        "https://api.example.com/{{string:api_key}}",
    )
    .await?;

    // Create instance
    let instance_service = state.instance_service.as_ref().unwrap();
    let form = ConfigureInstanceForm {
        instance_name: "test_instance".to_string(),
        description: None,
        tool_id,
        param_configs: vec![],
        csrf_token: "test".to_string(),
    };
    instance_service
        .create_instance_with_config(server_id, form)
        .await?;

    // Save bindings with secret
    let server_service = state.server_service.as_ref().unwrap();
    let globals_form = GlobalsForm {
        var_keys: vec![],
        var_values: vec![],
        secret_keys: vec!["api_key".to_string()],
        secret_values: vec!["super-secret-key-12345".to_string()],
        csrf_token: "test".to_string(),
    };

    server_service
        .save_server_globals(server_id, _user_id, globals_form)
        .await?;

    // Verify the binding was saved as a secret
    let globals = server_service.get_server_globals(server_id).await?;

    assert_eq!(globals.len(), 1, "Should have one global");
    assert_eq!(globals[0].key, "api_key");
    assert!(globals[0].is_secret.unwrap_or(false), "Should be a secret");
    // Note: value is encrypted, so we can't check the exact value directly

    Ok(())
}

#[tokio::test]
async fn test_parameters_matched_by_name_only() -> anyhow::Result<()> {
    let (state, _user_id, toolkit_id, server_id) = setup_full_test_environment().await?;

    // Create tools with same parameter name but different types
    let tool_id_1 = create_test_tool(
        &state.pool,
        toolkit_id,
        "tool_1",
        "https://api.example.com/{{string:user_id}}",
    )
    .await?;

    let tool_id_2 = create_test_tool(
        &state.pool,
        toolkit_id,
        "tool_2",
        "https://api.example.com/{{number:user_id}}",
    )
    .await?;

    // Create instances
    let instance_service = state.instance_service.as_ref().unwrap();

    let form1 = ConfigureInstanceForm {
        instance_name: "instance_1".to_string(),
        description: None,
        tool_id: tool_id_1,
        param_configs: vec![],
        csrf_token: "test".to_string(),
    };
    instance_service
        .create_instance_with_config(server_id, form1)
        .await?;

    let form2 = ConfigureInstanceForm {
        instance_name: "instance_2".to_string(),
        description: None,
        tool_id: tool_id_2,
        param_configs: vec![],
        csrf_token: "test".to_string(),
    };
    instance_service
        .create_instance_with_config(server_id, form2)
        .await?;

    // Save binding for user_id (should work for both types)
    let server_service = state.server_service.as_ref().unwrap();
    let globals_form = GlobalsForm {
        var_keys: vec!["user_id".to_string()],
        var_values: vec!["12345".to_string()],
        secret_keys: vec![],
        secret_values: vec![],
        csrf_token: "test".to_string(),
    };

    server_service
        .save_server_globals(server_id, _user_id, globals_form)
        .await?;

    // Verify the binding was saved
    let globals = server_service.get_server_globals(server_id).await?;

    assert_eq!(globals.len(), 1, "Should have one global for user_id");
    assert_eq!(globals[0].key, "user_id");
    assert_eq!(globals[0].value, "12345");

    // Note: The implementation should match by name only, not by type
    // So this single binding should work for both string:user_id and number:user_id

    Ok(())
}

#[tokio::test]
async fn test_save_bindings_updates_existing_values() -> anyhow::Result<()> {
    let (state, _user_id, toolkit_id, server_id) = setup_full_test_environment().await?;

    // Create tool with parameter
    let tool_id = create_test_tool(
        &state.pool,
        toolkit_id,
        "test_tool",
        "https://api.example.com/{{string:api_key}}",
    )
    .await?;

    // Create instance
    let instance_service = state.instance_service.as_ref().unwrap();
    let form = ConfigureInstanceForm {
        instance_name: "test_instance".to_string(),
        description: None,
        tool_id,
        param_configs: vec![],
        csrf_token: "test".to_string(),
    };
    instance_service
        .create_instance_with_config(server_id, form)
        .await?;

    let server_service = state.server_service.as_ref().unwrap();

    // Save initial binding
    let globals_form_1 = GlobalsForm {
        var_keys: vec!["api_key".to_string()],
        var_values: vec!["initial-value".to_string()],
        secret_keys: vec![],
        secret_values: vec![],
        csrf_token: "test".to_string(),
    };

    server_service
        .save_server_globals(server_id, _user_id, globals_form_1)
        .await?;

    // Verify initial value
    let globals = server_service.get_server_globals(server_id).await?;
    assert_eq!(globals[0].value, "initial-value");

    // Update the binding
    let globals_form_2 = GlobalsForm {
        var_keys: vec!["api_key".to_string()],
        var_values: vec!["updated-value".to_string()],
        secret_keys: vec![],
        secret_values: vec![],
        csrf_token: "test".to_string(),
    };

    server_service
        .save_server_globals(server_id, _user_id, globals_form_2)
        .await?;

    // Verify updated value
    let globals = server_service.get_server_globals(server_id).await?;
    assert_eq!(globals.len(), 1, "Should still have one global");
    assert_eq!(globals[0].value, "updated-value", "Value should be updated");

    Ok(())
}

#[tokio::test]
async fn test_save_bindings_with_mixed_variables_and_secrets() -> anyhow::Result<()> {
    let (state, _user_id, toolkit_id, server_id) = setup_full_test_environment().await?;

    // Create tool with multiple parameters
    let tool_id = create_test_tool(
        &state.pool,
        toolkit_id,
        "test_tool",
        "https://api.example.com/{{string:api_url}}/{{string:api_key}}",
    )
    .await?;

    // Create instance
    let instance_service = state.instance_service.as_ref().unwrap();
    let form = ConfigureInstanceForm {
        instance_name: "test_instance".to_string(),
        description: None,
        tool_id,
        param_configs: vec![],
        csrf_token: "test".to_string(),
    };
    instance_service
        .create_instance_with_config(server_id, form)
        .await?;

    // Save bindings with both variables and secrets
    let server_service = state.server_service.as_ref().unwrap();
    let globals_form = GlobalsForm {
        var_keys: vec!["api_url".to_string()],
        var_values: vec!["https://api.example.com".to_string()],
        secret_keys: vec!["api_key".to_string()],
        secret_values: vec!["secret-key-value".to_string()],
        csrf_token: "test".to_string(),
    };

    server_service
        .save_server_globals(server_id, _user_id, globals_form)
        .await?;

    // Verify both bindings were saved
    let globals = server_service.get_server_globals(server_id).await?;

    assert_eq!(globals.len(), 2, "Should have two globals");

    let api_url = globals.iter().find(|g| g.key == "api_url").unwrap();
    assert_eq!(api_url.value, "https://api.example.com");
    assert!(!api_url.is_secret.unwrap_or(false));

    let api_key = globals.iter().find(|g| g.key == "api_key").unwrap();
    assert!(api_key.is_secret.unwrap_or(false));

    Ok(())
}

#[tokio::test]
async fn test_bindings_empty_when_no_instances() -> anyhow::Result<()> {
    let (state, _user_id, _toolkit_id, server_id) = setup_full_test_environment().await?;

    // No instances created

    // Get discovered parameters
    let instance_service = state.instance_service.as_ref().unwrap();
    let discovered = instance_service
        .discover_parameters_with_usage(server_id)
        .await?;

    assert_eq!(
        discovered.len(),
        0,
        "Should have no discovered parameters when no instances"
    );

    Ok(())
}
