//! Wire-level tests for the JSON-RPC envelope, driven through the run loop.
//!
//! These assert what a client actually receives on stdout, byte for byte,
//! rather than what `dispatch` returns. Every defect they cover lives between
//! the read and the dispatch, so nothing below `serve` can see them.
//!
//! The requirements are quoted from JSON-RPC 2.0
//! (<https://www.jsonrpc.org/specification>) and from the MCP base protocol
//! (<https://modelcontextprotocol.io/specification/2025-11-25/basic>).

use serde_json::{Value, json};
use std::sync::Arc;
use wiremock::matchers::{method as wm_method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::test_support::{
    INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR, drive, error_code,
    error_message, search_body, test_server,
};

/// Start a mock catalog that answers the two endpoints a no-argument tool call
/// reaches, and a server pointed at it.
async fn server_with_catalog() -> (MockServer, Arc<crate::server::DataGovMcpServer>) {
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
            "organizations": [{"id": "1", "name": "NASA", "slug": "nasa"}],
            "total": 1
        })))
        .mount(&mock)
        .await;
    let server = Arc::new(test_server(&mock.uri()));
    (mock, server)
}

// ---------------------------------------------------------------------------
// The `jsonrpc` member (#93)
// ---------------------------------------------------------------------------

/// JSON-RPC 2.0, section 4: "jsonrpc: A String specifying the version of the
/// JSON-RPC protocol. MUST be exactly "2.0"." A member that is absent cannot be
/// exactly "2.0", so the message is not a Request object.
#[tokio::test]
async fn a_request_with_no_jsonrpc_member_is_rejected_as_invalid_request() {
    let (_mock, server) = server_with_catalog().await;

    let responses = drive(&server, b"{\"id\":2,\"method\":\"tools/list\"}\n").await;

    assert_eq!(responses.len(), 1, "exactly one response: {responses:?}");
    assert_eq!(
        error_code(&responses[0]),
        INVALID_REQUEST,
        "a message with no jsonrpc member is not a Request object: {}",
        responses[0]
    );
    assert_eq!(
        responses[0].get("id"),
        Some(&json!(2)),
        "the rejection must echo the id the client sent: {}",
        responses[0]
    );
}

/// The only version this protocol has. It must still be served.
#[tokio::test]
async fn a_request_with_jsonrpc_2_0_is_served() {
    let (_mock, server) = server_with_catalog().await;

    let responses = drive(
        &server,
        b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
    )
    .await;

    assert_eq!(responses.len(), 1, "exactly one response: {responses:?}");
    assert!(
        responses[0]["result"]["tools"].is_array(),
        "tools/list must answer with a tools array: {}",
        responses[0]
    );
    assert_eq!(responses[0].get("id"), Some(&json!(2)));
}

/// A version that is present but not "2.0" is the case that already worked, and
/// it has to keep working: the fix for the absent member must not swallow it.
#[tokio::test]
async fn a_request_with_jsonrpc_1_0_is_rejected_as_invalid_request() {
    let (_mock, server) = server_with_catalog().await;

    let responses = drive(
        &server,
        b"{\"jsonrpc\":\"1.0\",\"id\":3,\"method\":\"tools/list\"}\n",
    )
    .await;

    assert_eq!(responses.len(), 1, "exactly one response: {responses:?}");
    assert_eq!(error_code(&responses[0]), INVALID_REQUEST);
    assert!(
        error_message(&responses[0]).contains("1.0"),
        "the message must name the version the client sent: {}",
        responses[0]
    );
    assert_eq!(responses[0].get("id"), Some(&json!(3)));
}

/// "MUST be exactly "2.0"" is a String comparison. The JSON number `2.0` is not
/// that string, and neither is `true` or an object.
#[tokio::test]
async fn a_request_with_a_non_string_jsonrpc_member_is_rejected_as_invalid_request() {
    let (_mock, server) = server_with_catalog().await;

    for version in [
        json!(2.0),
        json!(2),
        json!(true),
        json!(["2.0"]),
        json!(null),
    ] {
        let line = format!(
            "{}\n",
            json!({"jsonrpc": version, "id": 4, "method": "tools/list"})
        );
        let responses = drive(&server, line.as_bytes()).await;

        assert_eq!(responses.len(), 1, "exactly one response: {responses:?}");
        assert_eq!(
            error_code(&responses[0]),
            INVALID_REQUEST,
            "jsonrpc {version} is not the string \"2.0\": {}",
            responses[0]
        );
        assert_eq!(
            responses[0].get("id"),
            Some(&json!(4)),
            "the rejection must echo the id the client sent: {}",
            responses[0]
        );
    }
}

/// A well-formed request naming a method the server does not have is -32601,
/// and it must reach the client rather than ending the session.
#[tokio::test]
async fn an_unknown_method_is_answered_with_method_not_found() {
    let (_mock, server) = server_with_catalog().await;

    let responses = drive(
        &server,
        b"{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"no/such/method\"}\n",
    )
    .await;

    assert_eq!(responses.len(), 1, "exactly one response: {responses:?}");
    assert_eq!(error_code(&responses[0]), METHOD_NOT_FOUND);
    assert_eq!(responses[0].get("id"), Some(&json!(5)));
}

// ---------------------------------------------------------------------------
// The read loop (#55.1)
// ---------------------------------------------------------------------------

/// One byte the client never meant to send must not end the session. The
/// following request is well formed and has to be answered.
#[tokio::test]
async fn a_non_utf8_byte_does_not_end_the_session() {
    let (_mock, server) = server_with_catalog().await;

    let mut input: Vec<u8> = vec![0xff, 0xfe, b'\n'];
    input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"tools/list\"}\n");

    let responses = drive(&server, &input).await;

    let answered = responses
        .iter()
        .find(|response| response.get("id") == Some(&json!(9)))
        .unwrap_or_else(|| {
            panic!("the well-formed request after the bad byte was never answered: {responses:?}")
        });
    assert!(
        answered["result"]["tools"].is_array(),
        "the surviving request must get a real tools/list result: {answered}"
    );
}

/// A byte sequence that is not UTF-8 cannot be parsed as JSON, so the frame it
/// arrived in is a parse error, not an invalid request.
#[tokio::test]
async fn a_non_utf8_line_is_answered_with_a_parse_error() {
    let (_mock, server) = server_with_catalog().await;

    let responses = drive(&server, &[0xff, 0xfe, b'\n']).await;

    assert_eq!(responses.len(), 1, "exactly one response: {responses:?}");
    assert_eq!(
        error_code(&responses[0]),
        PARSE_ERROR,
        "undecodable input is a parse error: {}",
        responses[0]
    );
    assert_eq!(
        responses[0].get("id"),
        Some(&json!(null)),
        "no id can be recovered from undecodable input, so it MUST be null: {}",
        responses[0]
    );
}

// ---------------------------------------------------------------------------
// Parse error against invalid request (#55.2)
// ---------------------------------------------------------------------------

/// JSON-RPC 2.0, section 5.1: -32700 is "Invalid JSON was received by the
/// server. An error occurred on the server while parsing the JSON text."
#[tokio::test]
async fn text_that_is_not_json_is_answered_with_a_parse_error() {
    let (_mock, server) = server_with_catalog().await;

    for line in [
        &b"{\"jsonrpc\": \"2.0\", \"id\": 1, \"method\":\n"[..],
        &b"not json at all\n"[..],
        &b"[1, 2\n"[..],
    ] {
        let responses = drive(&server, line).await;
        assert_eq!(responses.len(), 1, "exactly one response: {responses:?}");
        assert_eq!(
            error_code(&responses[0]),
            PARSE_ERROR,
            "unparseable text is -32700, not -32600: {}",
            responses[0]
        );
    }
}

/// -32600 is "The JSON sent is not a valid Request object" - it parsed, but it
/// is not a Request. Splitting this from -32700 is the whole point: the two
/// tell the client different things about what to fix.
#[tokio::test]
async fn valid_json_that_is_not_a_request_is_answered_with_invalid_request() {
    let (_mock, server) = server_with_catalog().await;

    let responses = drive(&server, b"{\"jsonrpc\":\"2.0\",\"id\":11}\n").await;

    assert_eq!(responses.len(), 1, "exactly one response: {responses:?}");
    assert_eq!(
        error_code(&responses[0]),
        INVALID_REQUEST,
        "a JSON object with no `method` parsed fine; it is simply not a Request: {}",
        responses[0]
    );
    assert_eq!(
        responses[0].get("id"),
        Some(&json!(11)),
        "the id is readable here, so it MUST be echoed: {}",
        responses[0]
    );
}

/// "Error responses MUST include the same ID as the request they correspond to
/// (except in error cases where the ID could not be read due a malformed
/// request)." The id survives a body that fails to deserialize as a Request.
#[tokio::test]
async fn an_error_echoes_a_string_id_the_client_sent() {
    let (_mock, server) = server_with_catalog().await;

    let responses = drive(&server, b"{\"jsonrpc\":\"2.0\",\"id\":\"req-42\"}\n").await;

    assert_eq!(responses.len(), 1, "exactly one response: {responses:?}");
    assert_eq!(
        responses[0].get("id"),
        Some(&json!("req-42")),
        "a string id must come back as the same string: {}",
        responses[0]
    );
}

// ---------------------------------------------------------------------------
// Request ids (#55.3)
// ---------------------------------------------------------------------------

/// MCP base protocol: "Unlike base JSON-RPC, the ID MUST NOT be `null`."
/// A message carrying `"id": null` is therefore a malformed request, not a
/// notification - and a client that sent one is waiting for an answer.
#[tokio::test]
async fn an_explicit_null_id_is_answered_with_an_error() {
    let (_mock, server) = server_with_catalog().await;

    let responses = drive(
        &server,
        b"{\"jsonrpc\":\"2.0\",\"id\":null,\"method\":\"tools/list\"}\n",
    )
    .await;

    assert_eq!(
        responses.len(),
        1,
        "an explicit null id is a request and must be answered, not swallowed: {responses:?}"
    );
    assert_eq!(
        error_code(&responses[0]),
        INVALID_REQUEST,
        "MCP forbids a null request id: {}",
        responses[0]
    );
}

/// MCP restricts `RequestId` to `string | number`. An id of any other JSON type
/// is not a request id, and no client could correlate the answer to it.
#[tokio::test]
async fn a_structurally_invalid_id_is_answered_with_an_error() {
    let (_mock, server) = server_with_catalog().await;

    for id in [json!({"a": 1}), json!([1]), json!(true)] {
        let line = format!(
            "{}\n",
            json!({"jsonrpc": "2.0", "id": id, "method": "tools/list"})
        );
        let responses = drive(&server, line.as_bytes()).await;

        assert_eq!(
            responses.len(),
            1,
            "exactly one response for id {id}: {responses:?}"
        );
        assert_eq!(
            error_code(&responses[0]),
            INVALID_REQUEST,
            "{id} is not a string or a number, so it is not a RequestId: {}",
            responses[0]
        );
    }
}

/// The other half of the id rule, and the one an over-correction breaks:
/// "Notifications MUST NOT include an ID" and "The receiver MUST NOT send a
/// response."
#[tokio::test]
async fn a_notification_with_no_id_is_never_answered() {
    let (_mock, server) = server_with_catalog().await;

    let responses = drive(
        &server,
        b"{\"jsonrpc\":\"2.0\",\"method\":\"tools/list\"}\n{\"jsonrpc\":\"2.0\",\"id\":12,\"method\":\"tools/list\"}\n",
    )
    .await;

    assert_eq!(
        responses.len(),
        1,
        "only the request may be answered, never the notification: {responses:?}"
    );
    assert_eq!(responses[0].get("id"), Some(&json!(12)));
}

/// A notification that cannot be dispatched still gets no reply. JSON-RPC's
/// "MUST NOT reply" has no exception for errors.
#[tokio::test]
async fn a_notification_is_not_answered_even_when_it_fails() {
    let (_mock, server) = server_with_catalog().await;

    let responses = drive(
        &server,
        b"{\"jsonrpc\":\"1.0\",\"method\":\"no/such/method\"}\n",
    )
    .await;

    assert!(
        responses.is_empty(),
        "a notification gets no response, whatever is wrong with it: {responses:?}"
    );
}

// ---------------------------------------------------------------------------
// Omitted `arguments` (#55.4)
// ---------------------------------------------------------------------------

/// `CallToolRequest` types `arguments` as optional. A tool whose advertised
/// `inputSchema` lists no `required` properties therefore has to accept a call
/// that omits it - every property has a usable absent value by construction.
///
/// Driven off `TOOL_SPECS` rather than one hand-picked tool, so a tool added
/// later is covered rather than skipped.
#[tokio::test]
async fn tools_call_with_omitted_arguments_succeeds_when_the_schema_requires_nothing() {
    let (_mock, server) = server_with_catalog().await;

    let mut checked = 0;
    for spec in crate::tools::TOOL_SPECS.iter() {
        let required = spec.input_schema.get("required").and_then(Value::as_array);
        if required.is_some_and(|fields| !fields.is_empty()) {
            continue;
        }
        checked += 1;

        let line = format!(
            "{}\n",
            json!({
                "jsonrpc": "2.0",
                "id": 20,
                "method": "tools/call",
                "params": {"name": spec.tool_name}
            })
        );
        let responses = drive(&server, line.as_bytes()).await;
        assert_eq!(responses.len(), 1, "exactly one response: {responses:?}");
        let response = &responses[0];

        assert!(
            response.get("error").is_none(),
            "{} advertises no required properties, so omitting `arguments` is a valid call: {response}",
            spec.tool_name
        );
        assert!(
            response["result"]["content"].is_array(),
            "{}: a tool result carries a content array: {response}",
            spec.tool_name
        );
        assert_ne!(
            response["result"].get("isError"),
            Some(&json!(true)),
            "{}: the call must actually run, not fail: {response}",
            spec.tool_name
        );
    }

    assert!(
        checked > 0,
        "no tool advertises an empty `required` list, so this test proved nothing"
    );
}

/// The converse, so the fix cannot be "accept everything": a tool that does
/// declare required properties must still reject a call that omits them.
#[tokio::test]
async fn tools_call_with_omitted_arguments_still_fails_when_the_schema_requires_a_field() {
    let (_mock, server) = server_with_catalog().await;

    let mut checked = 0;
    for spec in crate::tools::TOOL_SPECS.iter() {
        let Some(required) = spec.input_schema.get("required").and_then(Value::as_array) else {
            continue;
        };
        if required.is_empty() {
            continue;
        }
        checked += 1;

        let line = format!(
            "{}\n",
            json!({
                "jsonrpc": "2.0",
                "id": 21,
                "method": "tools/call",
                "params": {"name": spec.tool_name}
            })
        );
        let responses = drive(&server, line.as_bytes()).await;
        assert_eq!(responses.len(), 1, "exactly one response: {responses:?}");
        assert_eq!(
            error_code(&responses[0]),
            INVALID_PARAMS,
            "{} requires {required:?}; omitting `arguments` cannot satisfy that: {}",
            spec.tool_name,
            responses[0]
        );
    }

    assert!(
        checked > 0,
        "no tool declares a required property, so this test proved nothing"
    );
}
