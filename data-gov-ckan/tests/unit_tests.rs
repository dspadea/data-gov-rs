//! Unit tests for the CKAN client using mock HTTP responses.
//!
//! These tests validate URL construction, response parsing, and error handling
//! without requiring network access. Run with:
//!
//! ```bash
//! cargo test -p data-gov-ckan --test unit_tests
//! ```

use data_gov_ckan::{CkanClient, CkanError, Configuration};
use serde_json::json;
use std::sync::Arc;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Create a test client pointed at the given mock server.
fn test_client(base_url: &str) -> CkanClient {
    let config = Arc::new(Configuration {
        base_path: base_url.to_string(),
        user_agent: Some("test/1.0".to_string()),
        client: reqwest::Client::new(),
        basic_auth: None,
        oauth_access_token: None,
        bearer_access_token: None,
        api_key: None,
    });
    CkanClient::new(config)
}

// ---------------------------------------------------------------------------
// package_search
// ---------------------------------------------------------------------------

#[tokio::test]
async fn package_search_builds_correct_url_and_parses_response() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .and(query_param("q", "climate"))
        .and(query_param("rows", "5"))
        .and(query_param("start", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": {
                "count": 42,
                "results": [
                    {
                        "name": "climate-dataset-1",
                        "title": "Climate Dataset 1",
                        "id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
                    }
                ]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let result = client
        .package_search(Some("climate"), Some(5), Some(0), None)
        .await
        .expect("should succeed");

    assert_eq!(result.count, Some(42));
    let results = result.results.expect("should have results");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "climate-dataset-1");
}

#[tokio::test]
async fn package_search_with_fq_passes_filter_query() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .and(query_param("fq", "organization:epa-gov AND res_format:CSV"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": { "count": 10, "results": [] }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let result = client
        .package_search(
            None,
            None,
            None,
            Some("organization:epa-gov AND res_format:CSV"),
        )
        .await
        .expect("should succeed");

    assert_eq!(result.count, Some(10));
}

#[tokio::test]
async fn package_search_with_no_params() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": { "count": 0, "results": [] }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let result = client
        .package_search(None, None, None, None)
        .await
        .expect("should succeed");

    assert_eq!(result.count, Some(0));
}

// ---------------------------------------------------------------------------
// package_show
// ---------------------------------------------------------------------------

#[tokio::test]
async fn package_show_returns_full_dataset() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_show"))
        .and(query_param("id", "my-dataset"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": {
                "name": "my-dataset",
                "title": "My Dataset",
                "id": "11111111-2222-3333-4444-555555555555",
                "notes": "A description",
                "resources": [
                    {
                        "name": "data.csv",
                        "format": "CSV",
                        "url": "https://example.com/data.csv"
                    }
                ]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let pkg = client
        .package_show("my-dataset")
        .await
        .expect("should succeed");

    assert_eq!(pkg.name, "my-dataset");
    assert_eq!(pkg.title.as_deref(), Some("My Dataset"));
    assert_eq!(pkg.notes.as_deref(), Some("A description"));

    let resources = pkg.resources.expect("should have resources");
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].format.as_deref(), Some("CSV"));
}

#[tokio::test]
async fn package_show_url_encodes_special_characters() {
    let server = MockServer::start().await;

    // The id has spaces/special chars — reqwest should URL-encode them
    Mock::given(method("GET"))
        .and(path("/action/package_show"))
        .and(query_param("id", "my dataset/test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": { "name": "my-dataset-test" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let pkg = client
        .package_show("my dataset/test")
        .await
        .expect("should succeed");

    assert_eq!(pkg.name, "my-dataset-test");
}

// ---------------------------------------------------------------------------
// organization_list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn organization_list_with_sort_and_limit() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/organization_list"))
        .and(query_param("sort", "name"))
        .and(query_param("limit", "3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": ["epa-gov", "nasa-gov", "usda-gov"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let orgs = client
        .organization_list(Some("name"), Some(3), None)
        .await
        .expect("should succeed");

    assert_eq!(orgs, vec!["epa-gov", "nasa-gov", "usda-gov"]);
}

// ---------------------------------------------------------------------------
// group_list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn group_list_returns_names() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/group_list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": ["agriculture", "science"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let groups = client
        .group_list(None, None, None)
        .await
        .expect("should succeed");

    assert_eq!(groups, vec!["agriculture", "science"]);
}

// ---------------------------------------------------------------------------
// dataset_autocomplete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dataset_autocomplete_sends_q_and_limit() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_autocomplete"))
        .and(query_param("q", "elect"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": [
                { "name": "electric-vehicles", "title": "Electric Vehicles" },
                { "name": "election-data", "title": "Election Data" }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let results = client
        .dataset_autocomplete(Some("elect"), Some(5))
        .await
        .expect("should succeed");

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].name.as_deref(), Some("electric-vehicles"));
}

// ---------------------------------------------------------------------------
// tag_autocomplete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tag_autocomplete_returns_strings() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/tag_autocomplete"))
        .and(query_param("q", "health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": ["health", "healthcare", "health-data"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let tags = client
        .tag_autocomplete(Some("health"), None, None)
        .await
        .expect("should succeed");

    assert_eq!(tags, vec!["health", "healthcare", "health-data"]);
}

// ---------------------------------------------------------------------------
// organization_autocomplete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn organization_autocomplete_parses_response() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/organization_autocomplete"))
        .and(query_param("q", "dep"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": [
                { "name": "department-of-energy", "title": "Department of Energy" }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let orgs = client
        .organization_autocomplete(Some("dep"), None)
        .await
        .expect("should succeed");

    assert_eq!(orgs.len(), 1);
    assert_eq!(orgs[0].name.as_deref(), Some("department-of-energy"));
}

// ---------------------------------------------------------------------------
// resource_format_autocomplete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resource_format_autocomplete_returns_formats() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/format_autocomplete"))
        .and(query_param("q", "csv"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": ["CSV", "CSV/XLS"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let formats = client
        .resource_format_autocomplete(Some("csv"), None)
        .await
        .expect("should succeed");

    assert_eq!(formats, vec!["CSV", "CSV/XLS"]);
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

#[tokio::test]
async fn http_404_returns_api_error_with_status() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_show"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let err = client
        .package_show("nonexistent")
        .await
        .expect_err("should fail");

    match err {
        CkanError::ApiError { status, message } => {
            assert_eq!(status, 404);
            assert!(message.contains("Not Found"));
        }
        other => panic!("expected ApiError, got: {:?}", other),
    }
}

#[tokio::test]
async fn http_500_returns_api_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let err = client
        .package_search(Some("test"), None, None, None)
        .await
        .expect_err("should fail");

    match err {
        CkanError::ApiError { status, .. } => assert_eq!(status, 500),
        other => panic!("expected ApiError, got: {:?}", other),
    }
}

#[tokio::test]
async fn success_false_returns_api_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": false, "result": null,
            "error": { "message": "something went wrong" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let err = client
        .package_search(Some("test"), None, None, None)
        .await
        .expect_err("should fail");

    match err {
        CkanError::ApiError { status: 400, .. } => {}
        other => panic!("expected ApiError with status 400, got: {:?}", other),
    }
}

#[tokio::test]
async fn missing_result_field_returns_api_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_show"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true, "result": null
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let err = client
        .package_show("test")
        .await
        .expect_err("should fail");

    match err {
        CkanError::ApiError {
            status: 500,
            ref message,
        } => {
            assert!(message.contains("No result data"));
        }
        other => panic!("expected ApiError with 'No result data', got: {:?}", other),
    }
}

#[tokio::test]
async fn malformed_result_returns_parse_error() {
    let server = MockServer::start().await;

    // Return a result that's a string instead of a Package object
    Mock::given(method("GET"))
        .and(path("/action/package_show"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": "not a package object"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let err = client
        .package_show("test")
        .await
        .expect_err("should fail");

    assert!(
        matches!(err, CkanError::ParseError(_)),
        "expected ParseError, got: {:?}",
        err
    );
}

#[tokio::test]
async fn malformed_json_body_returns_request_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let err = client
        .package_search(Some("test"), None, None, None)
        .await
        .expect_err("should fail");

    assert!(
        matches!(err, CkanError::RequestError(_)),
        "expected RequestError, got: {:?}",
        err
    );
}

// ---------------------------------------------------------------------------
// Error trait and Display
// ---------------------------------------------------------------------------

#[test]
fn error_display_formats() {
    let api_err = CkanError::ApiError {
        status: 404,
        message: "Not Found".to_string(),
    };
    let display = format!("{}", api_err);
    assert!(display.contains("404"));
    assert!(display.contains("Not Found"));

    let parse_err = CkanError::ParseError(
        serde_json::from_str::<serde_json::Value>("invalid").unwrap_err(),
    );
    let display = format!("{}", parse_err);
    assert!(display.contains("Parse error"));

    let req_err = CkanError::RequestError(Box::new(std::io::Error::other("connection refused")));
    let display = format!("{}", req_err);
    assert!(display.contains("Request error"));
    assert!(display.contains("connection refused"));
}

#[test]
fn ckan_error_implements_std_error() {
    fn assert_error<T: std::error::Error>() {}
    assert_error::<CkanError>();
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[test]
fn default_configuration_has_expected_values() {
    let config = Configuration::default();
    assert_eq!(config.base_path, "https://catalog.data.gov/api/3");
    let expected_ua = concat!("data-gov-rs/", env!("CARGO_PKG_VERSION"));
    assert_eq!(config.user_agent.as_deref(), Some(expected_ua));
    assert!(config.api_key.is_none());
    assert!(config.basic_auth.is_none());
    assert!(config.oauth_access_token.is_none());
    assert!(config.bearer_access_token.is_none());
}

#[test]
fn client_debug_shows_base_path() {
    let config = Arc::new(Configuration {
        base_path: "https://example.com/api/3".to_string(),
        ..Configuration::default()
    });
    let client = CkanClient::new(config);
    let debug = format!("{:?}", client);
    assert!(debug.contains("example.com"));
}
