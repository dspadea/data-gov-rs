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

use serde_json::{Value, json};
use wiremock::matchers::{method as wm_method, path as wm_path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::test_support::{search_body, test_server};
use crate::types::{SUPPORTED_PROTOCOL_VERSIONS, ServerError};

/// Revisions the MCP specification has actually published, oldest first.
/// Literal on purpose: deriving it from `SUPPORTED_PROTOCOL_VERSIONS` would
/// make these assertions agree with whatever that constant says.
const PUBLISHED_MCP_REVISIONS: [&str; 4] = ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

/// The revision marked *current* at modelcontextprotocol.io/specification/versioning.
const CURRENT_MCP_REVISION: &str = "2025-11-25";

/// Extract the inner JSON payload from a `ToolResponse`-shaped value.
fn tool_response_json(value: &Value) -> &Value {
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .expect("ToolResponse must have content array");
    // `content` is a closed union in MCP; a structured payload rides alongside
    // it in `structuredContent`, never as a content block.
    for block in content {
        let ty = block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            matches!(
                ty,
                "text" | "image" | "audio" | "resource_link" | "resource"
            ),
            "`{ty}` is not an MCP content type"
        );
    }
    value
        .get("structuredContent")
        .expect("tool result must carry structuredContent")
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
    // dataset_by_slug resolves through the exact-lookup endpoint
    // GET /api/dataset/{slug_or_id}, not a full-text search.
    Mock::given(wm_method("GET"))
        .and(wm_path("/api/dataset/my-dataset"))
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
        result.get("serverInfo").is_some(),
        "initialize result should carry serverInfo, got: {result}"
    );
    assert!(
        result.get("protocolVersion").is_some(),
        "initialize result must carry protocolVersion (MCP requires it), got: {result}"
    );
}

/// MCP 2025-11-25, Lifecycle / Version Negotiation: "If the server supports the
/// requested protocol version, it MUST respond with the same version."
#[tokio::test]
async fn initialize_echoes_every_published_revision_verbatim() {
    let server = test_server("http://127.0.0.1:1");

    assert!(
        SUPPORTED_PROTOCOL_VERSIONS.contains(&CURRENT_MCP_REVISION),
        "server must speak the current MCP revision; advertises {SUPPORTED_PROTOCOL_VERSIONS:?}"
    );

    for requested in PUBLISHED_MCP_REVISIONS {
        let result = server
            .dispatch(
                "initialize",
                Some(json!({
                    "protocolVersion": requested,
                    "capabilities": {},
                    "clientInfo": { "name": "test-client", "version": "0.0.0" }
                })),
            )
            .await
            .unwrap_or_else(|err| panic!("initialize({requested}) must succeed: {err}"));

        assert_eq!(
            result.get("protocolVersion").and_then(Value::as_str),
            Some(requested),
            "{requested} is a published revision this server advertises, so the spec \
             requires echoing it verbatim; got: {result}"
        );
    }

    for advertised in SUPPORTED_PROTOCOL_VERSIONS {
        assert!(
            PUBLISHED_MCP_REVISIONS.contains(advertised),
            "{advertised} is not a published MCP revision"
        );
    }
}

/// "Otherwise, the server MUST respond with another protocol version it
/// supports. This SHOULD be the latest version supported by the server."
/// The MUST is membership; the SHOULD is recency. Neither is asserted against
/// `LATEST_PROTOCOL_VERSION`, which would compare the code with itself.
#[tokio::test]
async fn initialize_negotiates_unknown_or_absent_versions_to_the_current_revision() {
    let server = test_server("http://127.0.0.1:1");

    for requested in [
        "1999-01-01",
        "2025-06-19",
        "2026-11-25",
        "1.0.0",
        "2025-11-25 ",
        "",
    ] {
        let result = server
            .dispatch("initialize", Some(json!({ "protocolVersion": requested })))
            .await
            .unwrap_or_else(|err| panic!("initialize({requested:?}) must not error: {err}"));

        let answered = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("protocolVersion must be a string, got: {result}"));

        assert_ne!(
            answered, requested,
            "an unsupported version must never be echoed back"
        );
        assert!(
            SUPPORTED_PROTOCOL_VERSIONS.contains(&answered),
            "answered {answered:?}, which this server does not support"
        );
        assert_eq!(
            answered, CURRENT_MCP_REVISION,
            "fallback SHOULD be the latest we speak"
        );
    }

    for params in [
        None,
        Some(json!({})),
        Some(json!({ "protocolVersion": null })),
    ] {
        let shown = format!("{params:?}");
        let result = server
            .dispatch("initialize", params)
            .await
            .unwrap_or_else(|err| panic!("initialize({shown}) must not error: {err}"));
        assert_eq!(
            result.get("protocolVersion").and_then(Value::as_str),
            Some(CURRENT_MCP_REVISION),
            "omitted version must default to the current revision; params {shown} got: {result}"
        );
    }
}

/// MCP 2025-11-25, server/tools: "Servers that support tools MUST declare the
/// `tools` capability". `listChanged` is optional in the schema but typed
/// boolean, and it is a behavioural promise: this server never emits
/// `notifications/tools/list_changed`, so the honest value is false. Declaring
/// a capability the dispatcher cannot serve breaks the lifecycle rule that both
/// parties "only use capabilities that were successfully negotiated".
#[tokio::test]
async fn initialize_declares_only_capabilities_the_server_can_serve() {
    let server = test_server("http://127.0.0.1:1");
    let result = server
        .dispatch("initialize", Some(json!({})))
        .await
        .expect("initialize should succeed");

    let caps = result
        .get("capabilities")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("capabilities is required and must be an object: {result}"));

    let tools = caps
        .get("tools")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("a tool server MUST declare capabilities.tools: {result}"));

    assert_eq!(
        tools.get("listChanged"),
        Some(&json!(false)),
        "listChanged is typed boolean, and this server never emits \
         notifications/tools/list_changed, so it must be false; got: {tools:?}"
    );

    // Whitelist, not a blacklist of the one historical bug: the next invented
    // key ("listChange", "list_changed") must fail too.
    let unknown: Vec<&str> = tools
        .keys()
        .map(String::as_str)
        .filter(|key| *key != "listChanged")
        .collect();
    assert!(
        unknown.is_empty(),
        "not MCP tools-capability keys: {unknown:?}"
    );

    for declared in caps.keys() {
        assert!(
            matches!(
                declared.as_str(),
                "tools" | "prompts" | "resources" | "logging" | "completions" | "experimental"
            ),
            "`{declared}` is not a server capability in the 2025-11-25 schema"
        );
    }

    // Anything advertised must actually dispatch. A capability answered with
    // -32601 is worse than an undeclared one: the client finds out mid-session.
    for (capability, probe) in [
        ("prompts", "prompts/list"),
        ("resources", "resources/list"),
        ("completions", "completion/complete"),
        ("logging", "logging/setLevel"),
    ] {
        if caps.contains_key(capability) {
            assert!(
                !matches!(
                    server.dispatch(probe, Some(json!({}))).await,
                    Err(ServerError::InvalidMethod(_))
                ),
                "initialize declares `{capability}` but `{probe}` answers -32601"
            );
        }
    }
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
        .and(wm_path("/api/dataset/some-dataset"))
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

/// MCP 2025-11-25, server/tools: structured content "is returned as a JSON
/// object in the `structuredContent` field", and `content` is a closed union.
///
/// Driven off `TOOL_SPECS` rather than a hand-picked tool. Exercising one tool
/// is what let two of five ship a bare JSON array in `structuredContent`:
/// `data_gov.listOrganizations` and `data_gov.autocompleteDatasets` both return
/// `Vec<String>`, and a single-tool check on `data_gov_search` — which happens
/// to return an object — reported the shape as conformant.
///
/// A new tool added without a fixture here fails rather than being skipped.
#[tokio::test]
async fn every_tool_returns_object_shaped_structured_content() {
    fn arguments_for(tool: &str) -> Value {
        match tool {
            "data_gov_search" => json!({"query": "climate", "limit": 1}),
            "data_gov_dataset" => json!({"slug": "probe-dataset"}),
            "data_gov_autocomplete_datasets" => json!({"partial": "clim", "limit": 2}),
            "data_gov_list_organizations" => json!({"limit": 2}),
            "data_gov_download_resources" => json!({"datasetId": "probe-dataset"}),
            other => panic!("no fixture arguments for `{other}`; add them with the tool"),
        }
    }

    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/search"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(search_body("probe-dataset", "Probe Dataset")),
        )
        .mount(&mock)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/api/organizations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "organizations": [
                {"id": "1", "name": "NASA", "slug": "nasa"},
                {"id": "2", "name": "NOAA", "slug": "noaa"}
            ],
            "total": 2
        })))
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());

    for spec in crate::tools::TOOL_SPECS.iter() {
        let args = arguments_for(spec.tool_name);
        let outcome = server
            .dispatch(
                "tools/call",
                Some(json!({ "name": spec.tool_name, "arguments": args })),
            )
            .await;

        // A tool may legitimately fail against this mock (the download tool
        // reaches for distributions the fixture has none of). What must never
        // happen is a *successful* result in a non-conformant shape.
        let Ok(value) = outcome else { continue };

        for block in value["content"].as_array().expect("content array") {
            let ty = block["type"].as_str().expect("every block has a type");
            assert!(
                matches!(
                    ty,
                    "text" | "image" | "audio" | "resource_link" | "resource"
                ),
                "{}: `{ty}` is not an MCP content type",
                spec.tool_name
            );
        }

        // Presence is required, not merely checked-if-present. `from_value`
        // drops a non-object defensively, so `if let Some(..)` here would pass
        // for a handler that emitted a bare array: the guard would hide exactly
        // the bug this test exists to catch.
        let sc = value.get("structuredContent").unwrap_or_else(|| {
            panic!(
                "{}: every tool returns machine-readable data, so structuredContent must be \
                 present. Absent means the handler passed a non-object and the guard in \
                 ToolResponse::from_value dropped it: {value}",
                spec.tool_name
            )
        });
        assert!(
            sc.is_object(),
            "{}: structuredContent must be a JSON object, got {}: {sc}",
            spec.tool_name,
            if sc.is_array() {
                "an array"
            } else {
                "a scalar"
            }
        );
    }
}
