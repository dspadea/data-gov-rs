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
use wiremock::matchers::{method as wm_method, path as wm_path, path_regex as wm_path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::test_support::{
    CURRENT_MCP_REVISION, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR, Session,
    drive, error_code, error_message, search_body, test_server, test_server_with_gate,
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

// ---------------------------------------------------------------------------
// The full handshake (#44)
// ---------------------------------------------------------------------------

/// One session that walks `initialize` -> `notifications/initialized` ->
/// `tools/list` -> `tools/call`, the sequence every MCP client performs before
/// it can do anything, and validates each result against the schema.
///
/// The three methods were each covered alone. Alone, they cannot show that the
/// output of one is usable as the input of the next - which is the only thing
/// a client cares about. The tool called here is chosen from what `tools/list`
/// answered, not named in the test, so a `tools/list` that advertised
/// something uncallable would fail rather than be worked around.
#[tokio::test]
async fn a_client_can_initialize_then_list_then_call_a_tool() {
    let (_mock, server) = server_with_catalog().await;
    let mut session = Session::start(server);

    // 1. initialize. `InitializeResult` requires protocolVersion, capabilities
    //    and serverInfo; serverInfo requires name and version. A supported
    //    revision MUST be echoed verbatim.
    let requested = CURRENT_MCP_REVISION;
    session
        .send(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": requested,
                "capabilities": {},
                "clientInfo": {"name": "chained-test", "version": "1.0"}
            }
        }))
        .await;

    let initialized = session.next_response().await;
    assert_eq!(initialized.get("id"), Some(&json!(1)), "got: {initialized}");
    let result = initialized
        .get("result")
        .unwrap_or_else(|| panic!("initialize must succeed: {initialized}"));
    assert_eq!(
        result["protocolVersion"].as_str(),
        Some(requested),
        "a supported revision must come back verbatim: {result}"
    );
    assert!(
        result["capabilities"]["tools"].is_object(),
        "a tool server MUST declare capabilities.tools: {result}"
    );
    assert!(
        result["serverInfo"]["name"]
            .as_str()
            .is_some_and(|n| !n.is_empty()),
        "serverInfo.name must be a non-empty string: {result}"
    );
    assert!(
        result["serverInfo"]["version"]
            .as_str()
            .is_some_and(|v| !v.is_empty()),
        "serverInfo.version must be a non-empty string: {result}"
    );

    // 2. The lifecycle notification a client sends next. It is a notification,
    //    so nothing may come back for it - and it must not derail the session.
    session
        .send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await;

    // 3. tools/list. Each entry needs a name and an inputSchema that "MUST be a
    //    valid JSON Schema object (not null)".
    session
        .send(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
        .await;

    let listed = session.next_response().await;
    assert_eq!(
        listed.get("id"),
        Some(&json!(2)),
        "the notification must not have been answered, and tools/list must \
         come next: {listed}"
    );
    let tools = listed["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list must answer with a tools array: {listed}"))
        .clone();
    assert!(!tools.is_empty(), "a tool server must advertise a tool");

    for tool in &tools {
        let name = tool["name"]
            .as_str()
            .unwrap_or_else(|| panic!("every tool has a name: {tool}"));
        assert!(
            (1..=128).contains(&name.chars().count()),
            "tool names SHOULD be 1 to 128 characters: {name}"
        );
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')),
            "tool names SHOULD use only letters, digits, underscore, hyphen and dot: {name}"
        );
        assert_eq!(
            tool["inputSchema"]["type"], "object",
            "inputSchema MUST be a valid JSON Schema object: {tool}"
        );
    }

    // 4. tools/call, on a tool taken from the list rather than named here.
    let callable = tools
        .iter()
        .find(|tool| {
            tool["inputSchema"]
                .get("required")
                .and_then(Value::as_array)
                .is_none_or(|required| required.is_empty())
        })
        .unwrap_or_else(|| panic!("no advertised tool is callable with no arguments: {tools:?}"));
    let name = callable["name"].as_str().expect("a name");

    session
        .send(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": name, "arguments": {}}
        }))
        .await;

    let called = session.next_response().await;
    assert_eq!(called.get("id"), Some(&json!(3)), "got: {called}");
    let result = called
        .get("result")
        .unwrap_or_else(|| panic!("calling `{name}` must succeed: {called}"));
    assert_eq!(
        result["isError"],
        json!(false),
        "`{name}` was called with arguments its own schema accepts: {result}"
    );

    let content = result["content"]
        .as_array()
        .unwrap_or_else(|| panic!("a tool result carries a content array: {result}"));
    assert!(
        !content.is_empty(),
        "an empty array satisfies a loop vacuously"
    );
    for block in content {
        let ty = block["type"].as_str().expect("every block has a type");
        assert!(
            matches!(
                ty,
                "text" | "image" | "audio" | "resource_link" | "resource"
            ),
            "`{ty}` is not a member of MCP's content union: {block}"
        );
    }
    assert!(
        result["structuredContent"].is_object(),
        "structured content is returned as a JSON object: {result}"
    );

    assert!(
        session.finish().await.is_empty(),
        "the session ends with nothing outstanding"
    );
}

// ---------------------------------------------------------------------------
// Messages that are not Request objects at all
// ---------------------------------------------------------------------------

/// JSON-RPC 2.0 section 4: "A Notification is a Request object without an "id"
/// member." A top-level array or scalar is not a Request object, so it cannot
/// be a Notification either, and the spec's own worked examples answer `[]`
/// and `[1]` with -32600 and a null id.
///
/// MCP 2025-06-18 removed batching, so a client still sending a batch array is
/// exactly the client that needs to be told.
#[tokio::test]
async fn a_message_that_is_not_a_json_object_is_answered_with_invalid_request() {
    let (_mock, server) = server_with_catalog().await;

    for message in ["[]", "[1]", "1", "\"hello\"", "true", "null"] {
        let responses = drive(&server, format!("{message}\n").as_bytes()).await;

        assert_eq!(
            responses.len(),
            1,
            "`{message}` is not a Request object, so it cannot be a notification \
             and must be answered: {responses:?}"
        );
        assert_eq!(
            error_code(&responses[0]),
            INVALID_REQUEST,
            "`{message}` is not a valid Request object: {}",
            responses[0]
        );
        assert_eq!(
            responses[0].get("id"),
            Some(&json!(null)),
            "no id can be read from `{message}`, so it MUST be null: {}",
            responses[0]
        );
    }
}

/// The same rule for an object: without an `id` it is a Notification only if
/// it is otherwise a valid Request. `{"jsonrpc":"2.0","method":1}` is the
/// spec's own example of an invalid Request, and it is answered.
#[tokio::test]
async fn an_object_with_no_id_that_is_not_a_request_is_answered_with_invalid_request() {
    let (_mock, server) = server_with_catalog().await;

    for message in [
        json!({"jsonrpc": "2.0"}),
        json!({"jsonrpc": "2.0", "method": 1, "params": "bar"}),
        json!({"jsonrpc": "2.0", "method": ["tools/list"]}),
        json!({"method": null}),
    ] {
        let responses = drive(&server, format!("{message}\n").as_bytes()).await;

        assert_eq!(
            responses.len(),
            1,
            "{message} has no `method` string, so it is not a Request and not a \
             notification: {responses:?}"
        );
        assert_eq!(error_code(&responses[0]), INVALID_REQUEST, "for {message}");
        assert_eq!(responses[0].get("id"), Some(&json!(null)));
    }
}

/// A byte that is not UTF-8 inside a JSON string must not be repaired into a
/// different argument. Decoding it lossily would turn the slug `ab<0xff>cd`
/// into `ab\u{fffd}cd` and run the tool on a value the client never sent -
/// harvested metadata is untrusted, and so is a corrupted transport.
#[tokio::test]
async fn a_bad_byte_inside_a_json_string_is_a_parse_error_not_a_repaired_argument() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path_regex("^/api/dataset/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_body("x", "X")))
        .expect(0)
        .mount(&mock)
        .await;
    let server = Arc::new(test_server(&mock.uri()));

    let mut input: Vec<u8> =
        br#"{"jsonrpc":"2.0","id":1,"method":"data_gov.dataset","params":{"slug":"ab"#.to_vec();
    input.push(0xff);
    input.extend_from_slice(br#"cd"}}"#);
    input.push(b'\n');

    let responses = drive(&server, &input).await;

    assert_eq!(responses.len(), 1, "exactly one response: {responses:?}");
    assert_eq!(
        error_code(&responses[0]),
        PARSE_ERROR,
        "an undecodable byte makes the line unparseable, whatever it sits inside: {}",
        responses[0]
    );
}

// ---------------------------------------------------------------------------
// The envelope rule applies to notifications too
// ---------------------------------------------------------------------------

/// A notification gets no response, but that is not the same as getting no
/// scrutiny. A message the server has just declared invalid must not have
/// side effects - and cancelling somebody's in-flight request is a side
/// effect.
///
/// Deterministic because the cancellation is acted on by the reading task
/// before the line after it is read, while request 5 is provably still held.
#[tokio::test]
async fn a_cancellation_with_no_jsonrpc_member_does_not_cancel_anything() {
    let mock = MockServer::start().await;
    let (server, release) = test_server_with_gate(&mock.uri(), "data_gov.downloadResources");
    let mut session = Session::start(server);

    session
        .send(&json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "data_gov_download_resources",
                "arguments": {"datasetId": "held"}
            }
        }))
        .await;
    // No `jsonrpc` member: not a JSON-RPC 2.0 message, so not a cancellation.
    session
        .send(&json!({"method": "notifications/cancelled", "params": {"requestId": 5}}))
        .await;

    release.notify_one();

    let answer = session.next_response().await;
    assert_eq!(
        answer.get("id"),
        Some(&json!(5)),
        "request 5 was never validly cancelled, so it is still owed an answer: {answer}"
    );

    assert!(session.finish().await.is_empty());
}

/// MCP defines no notification that invokes a tool, and this server must not
/// invent one. A tool run from a notification has no id, so a cancellation
/// cannot reach it; owes no response, so nothing reports the files it wrote;
/// and holds no sender, so the loop reaching EOF tears its runtime down
/// mid-write. "Silence reads as success" is the failure AGENTS.md rules out.
///
/// Driven off `TOOL_SPECS`, so a tool added later is covered rather than
/// missed, and asserted on the admission rule so the result does not depend on
/// when a spawned task happens to run.
#[test]
fn no_tool_method_is_dispatchable_as_a_notification() {
    for spec in crate::tools::TOOL_SPECS.iter() {
        assert!(
            !crate::server::notification_may_dispatch(spec.method_name),
            "{} writes files and reaches the network; it must never run from a \
             message that owes no answer",
            spec.method_name
        );
    }
    assert!(
        !crate::server::notification_may_dispatch("tools/call"),
        "the tools/call envelope is the same hazard by another name"
    );

    // The lifecycle notifications a client really does send must still pass.
    for method in ["initialized", "notifications/initialized", "shutdown"] {
        assert!(
            crate::server::notification_may_dispatch(method),
            "{method} is a lifecycle notification, not a tool"
        );
    }
}

/// The same rule at the wire, so the check cannot be bypassed by a path that
/// never consults it.
#[tokio::test]
async fn a_notification_naming_a_tool_does_not_reach_the_catalog() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path_regex("^/api/dataset/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_body("x", "X")))
        .expect(0)
        .mount(&mock)
        .await;
    let server = Arc::new(test_server(&mock.uri()));

    let mut input = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "method": "data_gov.dataset",
            "params": {"slug": "should-never-be-fetched"}
        })
    );
    // A request after it, so the loop has demonstrably run past the
    // notification by the time the mock is verified.
    input.push_str(&format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})
    ));

    let responses = drive(&server, input.as_bytes()).await;

    assert_eq!(
        responses.len(),
        1,
        "only the ping is answered: {responses:?}"
    );
    assert_eq!(responses[0].get("id"), Some(&json!(1)));
    mock.verify().await;
}
