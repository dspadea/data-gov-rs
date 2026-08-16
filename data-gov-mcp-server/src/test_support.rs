//! Shared fixtures for the server's test modules.
//!
//! Holds the pieces `dispatch_tests` and `protocol_tests` both need: a server
//! wired to a mock catalog, a Catalog API response body, and a driver that
//! feeds raw bytes through the run loop and collects what comes back.

use data_gov::{DataGovClient, DataGovConfig, OperatingMode};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::server::{DEFAULT_REQUEST_TIMEOUT, DataGovMcpServer, EpilogueGate, TestGate};
use crate::types::ServerError;

/// How long a session waits for a response before calling the loop hung.
///
/// This bound never decides whether a test passes - each assertion is on the
/// content and the order of what arrives. It exists so a run loop that has
/// stopped answering fails by name instead of hanging the suite.
const RESPONSE_WAIT: Duration = Duration::from_secs(10);

/// Revisions the MCP specification has actually published, oldest first.
///
/// Deliberately literal, and deliberately in one place. Deriving it from
/// `SUPPORTED_PROTOCOL_VERSIONS` would make every assertion agree with whatever
/// that constant happens to say, so a revision quietly dropped - or invented -
/// would stay green.
pub(crate) const PUBLISHED_MCP_REVISIONS: [&str; 4] =
    ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

/// The revision marked *current* at modelcontextprotocol.io/specification/versioning.
/// Bumping this is a deliberate act of adopting a new spec, not a side effect.
pub(crate) const CURRENT_MCP_REVISION: &str = "2025-11-25";

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
///
/// A mock server listens on loopback, which downloads refuse by default. The
/// opt-in is what lets a download test reach the handler it is written to
/// exercise, and it is the same one the `data-gov` download tests take.
pub(crate) fn test_server(mock_uri: &str) -> DataGovMcpServer {
    let config = DataGovConfig::default()
        .with_base_url(mock_uri)
        .with_mode(OperatingMode::CommandLine)
        .with_private_network_downloads(true)
        .with_user_agent("test/1.0");
    let data_gov = DataGovClient::with_config(config).expect("build data_gov");

    DataGovMcpServer {
        data_gov,
        portal_base_url: mock_uri.to_string(),
        request_timeout: DEFAULT_REQUEST_TIMEOUT,
        test_gate: None,
    }
}

/// A server that holds `method` twice: once at dispatch, once in the epilogue
/// of the task that completed it.
///
/// The second hold is what makes the cancellation race an ordering rather than
/// a probability - see [`GatedTwice`].
pub(crate) fn gated_twice(mock_uri: &str, method: &'static str) -> GatedTwice {
    let dispatch = Arc::new(Notify::new());
    let arrived = Arc::new(Notify::new());
    let epilogue = Arc::new(Notify::new());

    let mut server = test_server(mock_uri);
    server.test_gate = Some(TestGate {
        method,
        release: Arc::clone(&dispatch),
        epilogue: Some(EpilogueGate {
            arrived: Arc::clone(&arrived),
            release: Arc::clone(&epilogue),
        }),
    });

    GatedTwice {
        server: Arc::new(server),
        dispatch,
        arrived,
        epilogue,
    }
}

/// The handles for a server built by [`gated_twice`].
///
/// Each `Notify` releases one waiter per `notify_one`, and stores a permit when
/// nobody is waiting yet, so a test can signal before or after the server
/// arrives without changing the outcome.
pub(crate) struct GatedTwice {
    /// The server itself, ready for [`Session::start`].
    pub server: Arc<DataGovMcpServer>,
    /// Releases one request from the dispatch hold.
    pub dispatch: Arc<Notify>,
    /// Signalled by the server when a task reaches the epilogue hold.
    pub arrived: Arc<Notify>,
    /// Releases one task from the epilogue hold.
    pub epilogue: Arc<Notify>,
}

/// A server that holds `method` until the returned handle is released.
///
/// The handle is a [`Notify`] with a stored permit, so releasing before the
/// dispatch reaches the gate works as well as releasing after it.
pub(crate) fn test_server_with_gate(
    mock_uri: &str,
    method: &'static str,
) -> (Arc<DataGovMcpServer>, Arc<Notify>) {
    gated_server(mock_uri, method, DEFAULT_REQUEST_TIMEOUT)
}

/// A server that abandons a request after `request_timeout` and gates nothing.
///
/// A gate holds a method before it does any work, which is exactly wrong for a
/// test that has to watch real work run past the budget. This door leaves the
/// handler free to run and moves only the budget.
pub(crate) fn timed_server(mock_uri: &str, request_timeout: Duration) -> Arc<DataGovMcpServer> {
    let mut server = test_server(mock_uri);
    server.request_timeout = request_timeout;
    Arc::new(server)
}

/// A scratch directory under the system temp dir, removed first so a previous
/// run cannot influence this one. The caller removes it when finished.
pub(crate) fn scratch_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "data-gov-mcp-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A gated server that abandons a request after `request_timeout`.
pub(crate) fn gated_server(
    mock_uri: &str,
    method: &'static str,
    request_timeout: Duration,
) -> (Arc<DataGovMcpServer>, Arc<Notify>) {
    let release = Arc::new(Notify::new());
    let mut server = test_server(mock_uri);
    server.request_timeout = request_timeout;
    server.test_gate = Some(TestGate {
        method,
        release: Arc::clone(&release),
        epilogue: None,
    });
    (Arc::new(server), release)
}

/// A live session against the run loop.
///
/// Unlike [`drive`], which feeds everything and then reads everything, a
/// session interleaves: send a line, read a response, send another. That is
/// what makes a concurrency claim testable without a clock.
pub(crate) struct Session {
    requests: Option<DuplexStream>,
    responses: Lines<BufReader<DuplexStream>>,
    loop_task: JoinHandle<Result<(), ServerError>>,
}

impl Session {
    /// Start the run loop over a pair of in-memory pipes.
    pub(crate) fn start(server: Arc<DataGovMcpServer>) -> Self {
        let (client_writer, server_reader) = tokio::io::duplex(1 << 16);
        let (server_writer, client_reader) = tokio::io::duplex(1 << 16);

        let loop_task =
            tokio::spawn(async move { server.serve(server_reader, server_writer).await });

        Self {
            requests: Some(client_writer),
            responses: BufReader::new(client_reader).lines(),
            loop_task,
        }
    }

    /// Write one message, followed by the newline that frames it.
    pub(crate) async fn send(&mut self, message: &Value) {
        let requests = self
            .requests
            .as_mut()
            .expect("the session is still accepting requests");
        let line = format!("{message}\n");
        requests.write_all(line.as_bytes()).await.expect("send");
        requests.flush().await.expect("flush");
    }

    /// Read the next response the server writes.
    ///
    /// Panics when nothing arrives within [`RESPONSE_WAIT`], which is what a
    /// run loop blocked behind an earlier request looks like from here.
    pub(crate) async fn next_response(&mut self) -> Value {
        let line = tokio::time::timeout(RESPONSE_WAIT, self.responses.next_line())
            .await
            .expect("the run loop stopped answering")
            .expect("read a response line")
            .expect("the run loop closed its output early");
        serde_json::from_str(&line).unwrap_or_else(|err| panic!("`{line}` is not JSON: {err}"))
    }

    /// Close the input and collect every response still to come.
    pub(crate) async fn finish(mut self) -> Vec<Value> {
        // Dropping the writer is EOF, which is what ends the loop.
        self.requests = None;

        let mut rest = Vec::new();
        while let Some(line) = tokio::time::timeout(RESPONSE_WAIT, self.responses.next_line())
            .await
            .expect("the run loop did not finish")
            .expect("read a response line")
        {
            rest.push(
                serde_json::from_str(&line)
                    .unwrap_or_else(|err| panic!("`{line}` is not JSON: {err}")),
            );
        }

        self.loop_task
            .await
            .expect("the run loop task must not panic")
            .expect("the transport must survive every message");
        rest
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
pub(crate) async fn drive(server: &Arc<DataGovMcpServer>, input: &[u8]) -> Vec<Value> {
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

    let owned = Arc::clone(server);
    let (served, (), raw) = tokio::join!(owned.serve(server_reader, server_writer), feed, collect);
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
