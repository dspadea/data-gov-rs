//! Shared fixtures for the server's test modules.
//!
//! Holds the pieces `dispatch_tests` and `protocol_tests` both need: a server
//! wired to a mock catalog, a Catalog API response body, and a driver that
//! feeds raw bytes through the run loop and collects what comes back.

use data_gov::{DataGovClient, DataGovConfig, OperatingMode};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::server::DataGovMcpServer;

/// JSON-RPC 2.0 error codes, section 5.1 of the published specification at
/// <https://www.jsonrpc.org/specification>.
///
/// Literal on purpose. Reading these out of [`crate::types::ResponseError`]
/// would make every assertion below agree with whatever the mapping happens to
/// say, including a mapping that has the codes swapped.
pub(crate) const PARSE_ERROR: i64 = -32700;
/// "The JSON sent is not a valid Request object."
pub(crate) const INVALID_REQUEST: i64 = -32600;
/// "The method does not exist / is not available."
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
/// "Invalid method parameter(s)."
pub(crate) const INVALID_PARAMS: i64 = -32602;

/// Build a `DataGovMcpServer` whose internal client points at the given mock
/// URL. Callers mount `Mock`s on the same server before exercising a dispatch
/// path.
pub(crate) fn test_server(mock_uri: &str) -> DataGovMcpServer {
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

/// Minimal search response body matching the Catalog API shape.
pub(crate) fn search_body(slug: &str, title: &str) -> Value {
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

/// Feed `input` to the run loop verbatim and return the response lines it
/// wrote, in order.
///
/// `input` is raw bytes rather than a `&str` so a test can send a byte
/// sequence that is not valid UTF-8. Closing the input signals EOF, which is
/// what ends the loop.
///
/// Every collected line is checked against the JSON-RPC 2.0 response envelope
/// before it is returned: that check belongs to no single test, and running it
/// here means no test can accidentally accept a malformed frame.
pub(crate) async fn drive(server: &DataGovMcpServer, input: &[u8]) -> Vec<Value> {
    let (mut client_writer, server_reader) = tokio::io::duplex(1 << 16);
    let (server_writer, mut client_reader) = tokio::io::duplex(1 << 16);

    let feed = async move {
        client_writer.write_all(input).await.expect("feed input");
        // Dropping the writer closes the pipe, which the loop sees as EOF.
        drop(client_writer);
    };

    let collect = async move {
        let mut raw = Vec::new();
        client_reader
            .read_to_end(&mut raw)
            .await
            .expect("read responses");
        raw
    };

    let (served, (), raw) = tokio::join!(server.serve(server_reader, server_writer), feed, collect);
    served.expect("the transport must survive every malformed message");

    let text = String::from_utf8(raw).expect("responses must be valid UTF-8");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: Value = serde_json::from_str(line)
                .unwrap_or_else(|err| panic!("`{line}` is not JSON: {err}"));
            assert_response_envelope(&value);
            value
        })
        .collect()
}

/// Assert one frame satisfies the JSON-RPC 2.0 response envelope.
///
/// Result responses MUST carry `result`; error responses MUST carry an `error`
/// with an integer `code` and a `message`; and both MUST carry `jsonrpc:
/// "2.0"`. A frame carrying both, or neither, is not a response at all.
fn assert_response_envelope(value: &Value) {
    assert_eq!(
        value.get("jsonrpc"),
        Some(&json!("2.0")),
        "every response carries jsonrpc 2.0: {value}"
    );
    let has_result = value.get("result").is_some();
    let has_error = value.get("error").is_some();
    assert!(
        has_result ^ has_error,
        "a response carries exactly one of result and error: {value}"
    );
    if let Some(error) = value.get("error") {
        assert!(
            error.get("code").and_then(Value::as_i64).is_some(),
            "error codes MUST be integers: {value}"
        );
        assert!(
            error.get("message").and_then(Value::as_str).is_some(),
            "error objects MUST carry a message: {value}"
        );
    }
}

/// The `error.code` of a response, or a panic naming the frame.
pub(crate) fn error_code(response: &Value) -> i64 {
    response
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("expected an error response, got: {response}"))
}

/// The `error.message` of a response, or a panic naming the frame.
pub(crate) fn error_message(response: &Value) -> &str {
    response
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("expected an error response, got: {response}"))
}
