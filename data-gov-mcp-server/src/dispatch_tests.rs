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

use crate::test_support::{
    CURRENT_MCP_REVISION, PUBLISHED_MCP_REVISIONS, search_body, test_server,
};
use crate::types::{SUPPORTED_PROTOCOL_VERSIONS, ServerError};

/// Arguments that satisfy each tool's advertised `inputSchema`.
///
/// A tool added without an entry here panics rather than being skipped, so the
/// registry-driven tests keep covering the whole set.
fn valid_arguments_for(tool: &str) -> Value {
    match tool {
        "data_gov_search" => json!({"query": "climate", "limit": 1}),
        "data_gov_dataset" => json!({"slug": "probe-dataset"}),
        "data_gov_autocomplete_datasets" => json!({"partial": "clim", "limit": 2}),
        "data_gov_list_organizations" => json!({"limit": 2}),
        "data_gov_download_resources" => json!({"datasetId": "probe-dataset"}),
        other => panic!("no fixture arguments for `{other}`; add them with the tool"),
    }
}

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
        valid_arguments_for(tool)
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
        // happen is a *successful* result in a non-conformant shape. A failure
        // arrives either as a JSON-RPC error or as a result flagged
        // `isError: true`, and neither carries machine-readable output.
        let Ok(value) = outcome else { continue };
        if tool_error_flag(&value) {
            continue;
        }

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

// ---------------------------------------------------------------------------
// Tool execution failures against protocol errors (#70.1)
// ---------------------------------------------------------------------------

/// A dataset body with a single CSV distribution and nothing else.
fn csv_only_dataset(slug: &str) -> Value {
    json!({
        "results": [{
            "slug": slug,
            "title": "CSV Only",
            "dcat": {
                "@type": "dcat:Dataset",
                "title": "CSV Only",
                "distribution": [{
                    "@type": "dcat:Distribution",
                    "downloadURL": "http://127.0.0.1:1/file.csv",
                    "mediaType": "text/csv"
                }]
            }
        }],
        "sort": "relevance"
    })
}

/// The `isError: true` flag on a tool result, or a panic naming the value.
fn tool_error_flag(value: &Value) -> bool {
    value
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("a tool result must carry isError: {value}"))
}

/// The concatenated text of a tool result's content blocks.
fn tool_text(value: &Value) -> String {
    value["content"]
        .as_array()
        .expect("content array")
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// MCP, server/tools, Error Handling: "Tool Execution Errors: Reported in tool
/// results with `isError: true`: API failures, ... business logic errors."
///
/// The reason it matters here is stated in AGENTS.md: an agent acts on a tool
/// result without checking it. A JSON-RPC error object may never reach the
/// model at all - "Clients SHOULD provide tool execution errors to language
/// models... MAY provide protocol errors."
#[tokio::test]
async fn an_upstream_failure_is_a_tool_result_with_is_error_true() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/search"))
        .respond_with(ResponseTemplate::new(500).set_body_string("upstream is down"))
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());

    let value = server
        .dispatch(
            "tools/call",
            Some(json!({"name": "data_gov_search", "arguments": {"query": "climate"}})),
        )
        .await
        .expect("an upstream failure is a tool result, not a JSON-RPC error");

    assert!(
        tool_error_flag(&value),
        "the tool ran and failed, so isError must be true: {value}"
    );
    assert!(
        !tool_text(&value).trim().is_empty(),
        "the failure has to be readable in `content`, not just flagged: {value}"
    );
}

/// The same rule for a transport that never answers at all.
#[tokio::test]
async fn an_unreachable_upstream_is_a_tool_result_with_is_error_true() {
    // Port 1 is reserved and refuses; no mock server is involved.
    let server = test_server("http://127.0.0.1:1");

    let value = server
        .dispatch(
            "tools/call",
            Some(json!({"name": "data_gov_search", "arguments": {"query": "climate"}})),
        )
        .await
        .expect("a connection failure is a tool result, not a JSON-RPC error");

    assert!(tool_error_flag(&value), "{value}");
}

/// The tools/call example in the spec carries `"isError": false` on success.
/// Emitting it means a client never has to infer the flag from its absence.
#[tokio::test]
async fn a_successful_tool_result_reports_is_error_false() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_body("ok-1", "OK")))
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());

    let value = server
        .dispatch(
            "tools/call",
            Some(json!({"name": "data_gov_search", "arguments": {"query": "climate"}})),
        )
        .await
        .expect("the call succeeds");

    assert!(
        !tool_error_flag(&value),
        "a successful call must say so: {value}"
    );
}

/// "no matching downloadable distributions" describes the dataset, not the
/// request. Reporting it as -32602 tells the model to fix parameters that were
/// never wrong.
#[tokio::test]
async fn no_matching_distributions_is_a_tool_error_not_a_parameter_error() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/api/dataset/csv-only"))
        .respond_with(ResponseTemplate::new(200).set_body_json(csv_only_dataset("csv-only")))
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());

    let value = server
        .dispatch(
            "tools/call",
            Some(json!({
                "name": "data_gov_download_resources",
                "arguments": {"datasetId": "csv-only", "formats": ["XLSX"]}
            })),
        )
        .await
        .expect("a content-dependent outcome is a tool result, not a JSON-RPC error");

    assert!(tool_error_flag(&value), "{value}");
    assert!(
        tool_text(&value).contains("XLSX"),
        "the model needs to be told which format was unavailable: {value}"
    );
}

/// A dataset the catalog serves without DCAT metadata is also a fact about the
/// data, not a parameter error.
#[tokio::test]
async fn a_dataset_without_dcat_metadata_is_a_tool_error_not_a_parameter_error() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/api/dataset/bare"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{"slug": "bare", "title": "Bare"}],
            "sort": "relevance"
        })))
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());

    let value = server
        .dispatch(
            "tools/call",
            Some(json!({
                "name": "data_gov_download_resources",
                "arguments": {"datasetId": "bare"}
            })),
        )
        .await
        .expect("a dataset with no DCAT metadata is a tool result, not a JSON-RPC error");

    assert!(tool_error_flag(&value), "{value}");
    assert!(
        tool_text(&value).contains("DCAT"),
        "the message must say what is missing: {value}"
    );
}

/// The other side of the line, so the fix cannot become "never fail loudly":
/// a protocol fault the model cannot correct stays a JSON-RPC error.
#[tokio::test]
async fn arguments_that_do_not_match_the_schema_stay_a_json_rpc_error() {
    let server = test_server("http://127.0.0.1:1");

    for arguments in [json!({"slug": 42}), json!({"slug": null}), json!([])] {
        let err = server
            .dispatch(
                "tools/call",
                Some(json!({"name": "data_gov_dataset", "arguments": arguments})),
            )
            .await
            .expect_err("arguments that fail the schema are a protocol error");
        assert!(
            matches!(err, ServerError::InvalidParams(_)),
            "expected InvalidParams for {arguments}, got {err:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// additionalProperties: false (#70.2)
// ---------------------------------------------------------------------------

/// Every tool schema declares `additionalProperties: false`, and a schema is a
/// promise. Dropping an undeclared key means the tool runs on arguments the
/// client did not send and reports success - the "partial result reported as a
/// whole one" failure AGENTS.md rules out.
///
/// Driven off `TOOL_SPECS`, with the undeclared key derived from a declared
/// one, which is the shape a real misspelling takes.
#[tokio::test]
async fn every_tool_rejects_an_argument_its_schema_does_not_declare() {
    let server = test_server("http://127.0.0.1:1");

    for spec in crate::tools::TOOL_SPECS.iter() {
        assert_eq!(
            spec.input_schema.get("additionalProperties"),
            Some(&json!(false)),
            "{}: this test only speaks for schemas that close the object",
            spec.tool_name
        );

        let properties = spec.input_schema["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("{}: schema has no properties", spec.tool_name));
        let declared = properties
            .keys()
            .next()
            .unwrap_or_else(|| panic!("{}: schema declares no property", spec.tool_name));
        let undeclared = format!("{declared}_typo");

        let mut arguments = valid_arguments_for(spec.tool_name);
        arguments[&undeclared] = json!("value the schema never mentioned");

        let err = server
            .dispatch(
                "tools/call",
                Some(json!({"name": spec.tool_name, "arguments": arguments})),
            )
            .await
            .expect_err(&format!(
                "{}: `{undeclared}` is not in the schema and must be refused",
                spec.tool_name
            ));

        match err {
            ServerError::InvalidParams(message) => assert!(
                message.contains(&undeclared),
                "{}: the message must name the offending key, got: {message}",
                spec.tool_name
            ),
            other => panic!("{}: expected InvalidParams, got {other:?}", spec.tool_name),
        }
    }
}

/// The case from the issue: a snake_case rendering of `outputDir` was dropped,
/// the files went to the default directory, and the call reported success.
#[tokio::test]
async fn a_misspelled_output_dir_is_refused_rather_than_silently_defaulted() {
    let server = test_server("http://127.0.0.1:1");

    let err = server
        .dispatch(
            "tools/call",
            Some(json!({
                "name": "data_gov_download_resources",
                "arguments": {"datasetId": "x", "output_dir": "/data"}
            })),
        )
        .await
        .expect_err("`output_dir` is not `outputDir` and must not be dropped");

    assert!(matches!(err, ServerError::InvalidParams(_)), "got {err:?}");
}

/// The other case from the issue: a misspelled filter yielded unfiltered
/// results that the model would then present as filtered.
#[tokio::test]
async fn a_misspelled_organization_filter_is_refused_rather_than_ignored() {
    let server = test_server("http://127.0.0.1:1");

    let err = server
        .dispatch(
            "tools/call",
            Some(json!({
                "name": "data_gov_search",
                "arguments": {"query": "climate", "organizationContain": "NASA"}
            })),
        )
        .await
        .expect_err("a dropped filter would return unfiltered results as filtered");

    assert!(matches!(err, ServerError::InvalidParams(_)), "got {err:?}");
}

// ---------------------------------------------------------------------------
// unavailableFormats (#70.4)
// ---------------------------------------------------------------------------

/// The reproduction from the issue: `{"formats":["  ","XLSX"]}` on a CSV-only
/// dataset. The filtering was already right; the diagnostic named the wrong
/// string, because the raw and normalized vectors stopped index-aligning the
/// moment a blank was dropped.
#[tokio::test]
async fn a_blank_format_filter_does_not_shift_the_unavailable_format_report() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/api/dataset/csv-only"))
        .respond_with(ResponseTemplate::new(200).set_body_json(csv_only_dataset("csv-only")))
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());

    let value = server
        .dispatch(
            "tools/call",
            Some(json!({
                "name": "data_gov_download_resources",
                "arguments": {"datasetId": "csv-only", "formats": ["  ", "XLSX"]}
            })),
        )
        .await
        .expect("nothing matched, which is an outcome rather than a fault");

    let text = tool_text(&value);
    assert!(
        text.contains("XLSX"),
        "XLSX is the format that was actually unavailable: {value}"
    );
}

// ---------------------------------------------------------------------------
// A download that downloaded nothing
// ---------------------------------------------------------------------------

/// A scratch directory under the system temp dir, removed first so a previous
/// run cannot influence this one. The caller removes it when finished.
fn scratch_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "data-gov-mcp-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A tool that downloaded none of the files it was asked for did not succeed.
///
/// The counts are in `structuredContent`, but this change is the one that made
/// `isError` always present, and AGENTS.md's premise is that an agent acts on
/// a result without checking it. `isError: false` on a call where every file
/// failed is the "silence reads as success" failure with a flag attached.
///
/// The machine-readable summary has to survive the flag: the agent needs to
/// know which files failed and why, not just that something did.
#[tokio::test]
async fn a_download_where_every_file_failed_reports_is_error_true() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/api/dataset/all-fail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "slug": "all-fail",
                "title": "All Fail",
                "dcat": {
                    "@type": "dcat:Dataset",
                    "title": "All Fail",
                    "distribution": [{
                        "@type": "dcat:Distribution",
                        "title": "gone",
                        "downloadURL": format!("{}/missing.csv", mock.uri()),
                        "mediaType": "text/csv"
                    }]
                }
            }],
            "sort": "relevance"
        })))
        .mount(&mock)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/missing.csv"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());
    let output_dir = scratch_dir("all-fail");

    let value = server
        .dispatch(
            "tools/call",
            Some(json!({
                "name": "data_gov_download_resources",
                "arguments": {
                    "datasetId": "all-fail",
                    "outputDir": output_dir.to_string_lossy(),
                    "datasetSubdirectory": false
                }
            })),
        )
        .await
        .expect("a failed download is a tool result, not a JSON-RPC error");

    let _ = std::fs::remove_dir_all(&output_dir);

    assert!(
        tool_error_flag(&value),
        "every file failed, so the call did not succeed: {value}"
    );

    let summary = value
        .get("structuredContent")
        .unwrap_or_else(|| panic!("the per-file detail must survive the flag: {value}"));
    assert_eq!(summary["successfulCount"], json!(0), "{summary}");
    assert_eq!(summary["failedCount"], json!(1), "{summary}");
    assert_eq!(summary["hasErrors"], json!(true), "{summary}");
    assert_eq!(
        summary["downloads"][0]["status"], "error",
        "the agent needs to see which file failed: {summary}"
    );
}

/// The deliberate other half: a call where some files arrived is not reported
/// as a failure. The summary names exactly which ones did not - `hasErrors`,
/// `failedCount`, and a per-file `status` - so nothing is hidden, and flagging
/// the whole call as an error would misreport the files that did arrive.
#[tokio::test]
async fn a_download_where_some_files_arrived_reports_is_error_false() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/api/dataset/half-fail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "slug": "half-fail",
                "title": "Half Fail",
                "dcat": {
                    "@type": "dcat:Dataset",
                    "title": "Half Fail",
                    "distribution": [
                        {
                            "@type": "dcat:Distribution",
                            "title": "present",
                            "downloadURL": format!("{}/present.csv", mock.uri()),
                            "mediaType": "text/csv"
                        },
                        {
                            "@type": "dcat:Distribution",
                            "title": "gone",
                            "downloadURL": format!("{}/missing.csv", mock.uri()),
                            "mediaType": "text/csv"
                        }
                    ]
                }
            }],
            "sort": "relevance"
        })))
        .mount(&mock)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/present.csv"))
        .respond_with(ResponseTemplate::new(200).set_body_string("a,b\n1,2\n"))
        .mount(&mock)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/missing.csv"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());
    let output_dir = scratch_dir("half-fail");

    let value = server
        .dispatch(
            "tools/call",
            Some(json!({
                "name": "data_gov_download_resources",
                "arguments": {
                    "datasetId": "half-fail",
                    "outputDir": output_dir.to_string_lossy(),
                    "datasetSubdirectory": false
                }
            })),
        )
        .await
        .expect("a partial download is a result");

    let _ = std::fs::remove_dir_all(&output_dir);

    assert!(
        !tool_error_flag(&value),
        "one file arrived, so this is a partial success, not a failure: {value}"
    );
    let summary = &value["structuredContent"];
    assert_eq!(summary["successfulCount"], json!(1), "{summary}");
    assert_eq!(summary["failedCount"], json!(1), "{summary}");
    assert_eq!(
        summary["hasErrors"],
        json!(true),
        "the partial failure must still be visible in the payload: {summary}"
    );
}

// ---------------------------------------------------------------------------
// A format filter that names no format
// ---------------------------------------------------------------------------

/// A dataset with one distribution the mock server actually serves.
///
/// The `formats` tests need a download that can succeed, so the failure they
/// report can only come from the filter.
fn one_csv_dataset(slug: &str, mock_uri: &str) -> Value {
    json!({
        "results": [{
            "slug": slug,
            "title": "One CSV",
            "dcat": {
                "@type": "dcat:Dataset",
                "title": "One CSV",
                "distribution": [{
                    "@type": "dcat:Distribution",
                    "title": "readings",
                    "downloadURL": format!("{mock_uri}/readings.csv"),
                    "mediaType": "text/csv"
                }]
            }
        }],
        "sort": "relevance"
    })
}

/// Dispatch `data_gov_download_resources` for `one_csv_dataset` with the given
/// `formats` argument, and return the tool result.
async fn download_with_formats(scratch: &str, formats: Value) -> Value {
    let mock = MockServer::start().await;
    let slug = "one-csv";
    Mock::given(wm_method("GET"))
        .and(wm_path(format!("/api/dataset/{slug}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(one_csv_dataset(slug, &mock.uri())))
        .mount(&mock)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/readings.csv"))
        .respond_with(ResponseTemplate::new(200).set_body_string("a,b\n1,2\n"))
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());
    let output_dir = scratch_dir(scratch);

    let value = server
        .dispatch(
            "tools/call",
            Some(json!({
                "name": "data_gov_download_resources",
                "arguments": {
                    "datasetId": slug,
                    "formats": formats,
                    "outputDir": output_dir.to_string_lossy(),
                    "datasetSubdirectory": false
                }
            })),
        )
        .await
        .expect("a download is a tool result, not a JSON-RPC error");

    let _ = std::fs::remove_dir_all(&output_dir);
    value
}

/// An empty `formats` array names no format to keep out, so it keeps
/// everything - the same as omitting the argument.
///
/// Filtering on it instead removed every distribution and reported "no
/// matching downloadable distributions" with an empty `unavailableFormats`,
/// naming no format the model could correct.
#[tokio::test]
async fn an_empty_formats_array_downloads_every_distribution() {
    let value = download_with_formats("empty-formats", json!([])).await;

    assert!(
        !tool_error_flag(&value),
        "an empty filter names no format to exclude: {value}"
    );
    let summary = &value["structuredContent"];
    assert_eq!(summary["successfulCount"], json!(1), "{summary}");
    assert_eq!(summary["failedCount"], json!(0), "{summary}");
}

/// Blank entries are dropped because each would match every distribution, so
/// an array of nothing but blanks is a filter that matches everything.
#[tokio::test]
async fn a_formats_array_of_only_blanks_downloads_every_distribution() {
    let value = download_with_formats("blank-formats", json!(["", "  ", "\t"])).await;

    assert!(
        !tool_error_flag(&value),
        "every entry matches everything, so the filter excludes nothing: {value}"
    );
    let summary = &value["structuredContent"];
    assert_eq!(summary["successfulCount"], json!(1), "{summary}");
    assert_eq!(summary["failedCount"], json!(0), "{summary}");
}

/// The other half: a filter that does name a format still filters. Without
/// this, "keep everything when the list is empty" could be written as "keep
/// everything", and both tests above would still pass.
#[tokio::test]
async fn a_format_filter_that_names_a_format_still_excludes_the_rest() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/api/dataset/csv-only"))
        .respond_with(ResponseTemplate::new(200).set_body_json(csv_only_dataset("csv-only")))
        .mount(&mock)
        .await;

    let server = test_server(&mock.uri());

    let value = server
        .dispatch(
            "tools/call",
            Some(json!({
                "name": "data_gov_download_resources",
                "arguments": {"datasetId": "csv-only", "formats": ["", "XLSX"]}
            })),
        )
        .await
        .expect("nothing matched, which is an outcome rather than a fault");

    assert!(
        tool_error_flag(&value),
        "XLSX matches no distribution, so nothing is left to download: {value}"
    );
    assert!(
        tool_text(&value).contains("XLSX"),
        "the model needs to be told which format was unavailable: {value}"
    );
}
