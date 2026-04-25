//! Routing tests for [`DataGovMcpServer::dispatch`].
//!
//! Verifies the routing contracts:
//!
//! 1. `tools/call` unwraps its nested method name and wraps the result in a
//!    `ToolResponse` envelope.
//! 2. A direct call to a registered tool method is also wrapped in a
//!    `ToolResponse`.
//! 3. Non-tool methods (`initialize`, `tools/list`) return raw JSON with no
//!    envelope.
//!
//! Plus error-variant contracts for unknown methods and missing params.

use data_gov::{DataGovClient, DataGovConfig, OperatingMode};
use serde_json::{Value, json};
use wiremock::matchers::{method as wm_method, path as wm_path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::server::DataGovMcpServer;
use crate::types::ServerError;

/// Build a `DataGovMcpServer` whose internal client points at the given mock
/// URL. Callers mount `Mock`s on the same server before exercising a dispatch
/// path.
fn test_server(mock_uri: &str) -> DataGovMcpServer {
    let config = DataGovConfig::default()
        .with_base_url(mock_uri)
        .with_mode(OperatingMode::CommandLine)
        .with_user_agent("test/1.0");
    let data_gov = DataGovClient::with_config(config).expect("build data_gov");

    DataGovMcpServer {
        data_gov,
        portal_base_url: mock_uri.to_string(),
    }
}

/// Extract the inner JSON payload from a `ToolResponse`-shaped value.
fn tool_response_json(value: &Value) -> &Value {
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .expect("ToolResponse must have content array");
    let json_item = content
        .iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("json"))
        .expect("ToolResponse must contain a json item");
    json_item
        .get("json")
        .expect("json item must have inner 'json' field")
}

/// Minimal search response body matching the Catalog API shape.
fn search_body(slug: &str, title: &str) -> Value {
    json!({
        "results": [{
            "identifier": format!("id:{slug}"),
            "slug": slug,
            "title": title,
            "description": "mock",
            "publisher": "mock",
            "organization": {
                "id": "00000000-0000-0000-0000-000000000000",
                "name": "Mock Org",
                "slug": "mock-org",
                "organization_type": "Federal Government"
            },
            "keyword": [],
            "theme": [],
            "has_spatial": false,
            "dcat": {
                "@type": "dcat:Dataset",
                "title": title,
                "description": "mock",
                "identifier": format!("id:{slug}"),
                "distribution": []
            }
        }],
        "sort": "relevance"
    })
}

#[tokio::test]
async fn dispatch_tools_list_returns_raw_descriptor_array() {
    let mock = MockServer::start().await;
    let server = test_server(&mock.uri());

    let result = server
        .dispatch("tools/list", None)
        .await
        .expect("tools/list should succeed with no params");

    assert!(
        result.get("content").is_none(),
        "tools/list result must not be wrapped in a ToolResponse"
    );

    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .expect("tools/list must return a `tools` array");
    assert!(
        !tools.is_empty(),
        "tools/list must return at least one tool"
    );

    for tool in tools {
        assert!(tool.get("name").is_some(), "tool missing name: {tool}");
        assert!(
            tool.get("description").is_some(),
            "tool missing description"
        );
        assert!(
            tool.get("inputSchema").is_some(),
            "tool missing inputSchema"
        );
    }
}

#[tokio::test]
async fn dispatch_tools_call_unwraps_and_wraps_response() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/search"))
        .and(query_param("q", "climate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_body("ds-1", "DS1")))
        .expect(1)
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());

    let result = server
        .dispatch(
            "tools/call",
            Some(json!({
                "name": "data_gov_search",
                "arguments": { "query": "climate" }
            })),
        )
        .await
        .expect("tools/call should succeed");

    let inner = tool_response_json(&result);
    let results = inner
        .get("results")
        .and_then(Value::as_array)
        .expect("inner payload must carry results array");
    assert_eq!(results.len(), 1);
    let summaries = inner
        .get("summaries")
        .and_then(Value::as_array)
        .expect("handler must attach summaries");
    assert_eq!(summaries.len(), 1);
    assert_eq!(
        summaries[0].get("slug").and_then(Value::as_str),
        Some("ds-1")
    );
}

#[tokio::test]
async fn dispatch_tools_call_unknown_tool_returns_invalid_method() {
    let mock = MockServer::start().await;
    let server = test_server(&mock.uri());

    let err = server
        .dispatch(
            "tools/call",
            Some(json!({ "name": "not_a_real_tool", "arguments": {} })),
        )
        .await
        .expect_err("unknown tool must fail");

    match err {
        ServerError::InvalidMethod(name) => assert_eq!(name, "not_a_real_tool"),
        other => panic!("expected InvalidMethod, got {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_tools_call_missing_params_returns_invalid_params() {
    let mock = MockServer::start().await;
    let server = test_server(&mock.uri());

    let err = server
        .dispatch("tools/call", None)
        .await
        .expect_err("tools/call without params must fail");

    assert!(matches!(err, ServerError::InvalidParams(_)));
}

#[tokio::test]
async fn dispatch_direct_tool_method_wraps_response() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/search"))
        .and(query_param("slug", "my-dataset"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(search_body("my-dataset", "My Dataset")),
        )
        .expect(1)
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());

    let result = server
        .dispatch("data_gov.dataset", Some(json!({ "slug": "my-dataset" })))
        .await
        .expect("direct data_gov.dataset call should succeed");

    let inner = tool_response_json(&result);
    assert_eq!(
        inner.get("slug").and_then(Value::as_str),
        Some("my-dataset"),
        "wrapped payload should carry the mocked dataset slug"
    );
}

#[tokio::test]
async fn dispatch_unknown_method_returns_invalid_method() {
    let mock = MockServer::start().await;
    let server = test_server(&mock.uri());

    let err = server
        .dispatch("not.a.real.method", Some(json!({})))
        .await
        .expect_err("unknown method must fail");

    match err {
        ServerError::InvalidMethod(name) => assert_eq!(name, "not.a.real.method"),
        other => panic!("expected InvalidMethod, got {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_initialize_returns_raw_response() {
    let mock = MockServer::start().await;
    let server = test_server(&mock.uri());

    let result = server
        .dispatch(
            "initialize",
            Some(json!({
                "clientInfo": { "name": "test-client", "version": "0.0.0" }
            })),
        )
        .await
        .expect("initialize should succeed");

    assert!(
        result.get("content").is_none(),
        "initialize is not a tool — must not be wrapped"
    );
    assert!(
        result.get("serverInfo").is_some() || result.get("protocolVersion").is_some(),
        "initialize result should carry server metadata, got: {result}"
    );
}

#[tokio::test]
async fn dispatch_data_gov_search_attaches_summaries() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/search"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(search_body("summary-probe", "Summary Probe")),
        )
        .expect(1)
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());

    let result = server
        .dispatch("data_gov.search", Some(json!({ "query": "probe" })))
        .await
        .expect("data_gov.search should succeed");

    let inner = tool_response_json(&result);
    let summaries = inner
        .get("summaries")
        .and_then(Value::as_array)
        .expect("data_gov.search must produce a summaries array");
    assert_eq!(summaries.len(), 1);
    assert_eq!(
        summaries[0].get("slug").and_then(Value::as_str),
        Some("summary-probe")
    );
}

#[tokio::test]
async fn dispatch_download_resources_rejects_parent_traversal_in_output_dir() {
    let mock = MockServer::start().await;
    // The handler validates distributions and output_dir after fetching the
    // dataset. Include at least one downloadable distribution so the traversal
    // check is the one that fires.
    Mock::given(wm_method("GET"))
        .and(wm_path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "slug": "some-dataset",
                "title": "Some Dataset",
                "dcat": {
                    "@type": "dcat:Dataset",
                    "title": "Some Dataset",
                    "distribution": [{
                        "@type": "dcat:Distribution",
                        "downloadURL": "http://localhost:1/file.csv",
                        "mediaType": "text/csv"
                    }]
                }
            }],
            "sort": "relevance"
        })))
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());

    let err = server
        .dispatch(
            "data_gov.downloadResources",
            Some(json!({
                "datasetId": "some-dataset",
                "outputDir": "../../etc"
            })),
        )
        .await
        .expect_err("output_dir with '..' must be rejected");

    match err {
        ServerError::InvalidParams(msg) => assert!(msg.contains("..")),
        other => panic!("expected InvalidParams, got {other:?}"),
    }
}
