use data_gov_ckan::{ApiKey, CkanClient, CkanError, Configuration};
use std::sync::Arc;

/// Test that we can create a client and it has expected debug output
#[test]
fn test_client_creation() {
    let config = Arc::new(Configuration {
        base_path: "https://catalog.data.gov/api/3".to_string(),
        user_agent: Some("test-client/1.0".to_string()),
        client: reqwest::Client::new(),
        basic_auth: None,
        oauth_access_token: None,
        bearer_access_token: None,
        api_key: None,
    });

    let client = CkanClient::new(config);

    // Test debug formatting
    let debug_str = format!("{:?}", client);
    assert!(debug_str.contains("CkanClient"));
    assert!(debug_str.contains("catalog.data.gov"));
}

/// Test that we can create a client with authentication
#[test]
fn test_authenticated_client_creation() {
    let config = Arc::new(Configuration {
        base_path: "https://catalog.data.gov/api/3".to_string(),
        user_agent: Some("test-client/1.0".to_string()),
        client: reqwest::Client::new(),
        basic_auth: None,
        oauth_access_token: None,
        bearer_access_token: None,
        api_key: Some(ApiKey {
            prefix: None,
            key: "test-api-key".to_string(),
        }),
    });

    let client = CkanClient::new(config);

    // Test debug formatting
    let debug_str = format!("{:?}", client);
    assert!(debug_str.contains("CkanClient"));
}

/// Test error types implement expected traits
#[test]
fn test_error_types() {
    // Test RequestError
    let req_error = CkanError::RequestError(Box::new(std::io::Error::other(
        "test error",
    )));

    // Should be able to display and debug
    let _display = format!("{}", req_error);
    let _debug = format!("{:?}", req_error);

    // Test ParseError
    let parse_error = CkanError::ParseError(
        serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err(),
    );
    let _display = format!("{}", parse_error);
    let _debug = format!("{:?}", parse_error);

    // Test ApiError
    let api_error = CkanError::ApiError {
        status: 404,
        message: "Not Found".to_string(),
    };
    let _display = format!("{}", api_error);
    let _debug = format!("{:?}", api_error);

    // Test that it implements Error trait
    fn check_error_trait<T: std::error::Error>(_: T) {}
    check_error_trait(req_error);
}

/// Test that error messages are meaningful  
#[test]
fn test_error_messages() {
    let api_error = CkanError::ApiError {
        status: 404,
        message: "Dataset not found".to_string(),
    };

    let message = format!("{}", api_error);
    assert!(message.contains("404"));
    assert!(message.contains("Dataset not found"));
}
