//! Tests for concurrent dispatch, cancellation, and the keepalive.
//!
//! Nothing here waits on a clock to decide whether it passed. The slow request
//! is held at a gate the test owns, so "the second request was answered while
//! the first was still running" is a fact about ordering rather than a
//! measurement. The only time bound is the one inside
//! [`crate::test_support::Session`], which turns a run loop that has stopped
//! answering into a named failure instead of a hung suite.

use serde_json::{Value, json};
use std::collections::HashSet;
use wiremock::MockServer;

use crate::test_support::{Session, drive, test_server, test_server_with_gate};

/// The tool method the gate holds in these tests. Held at the dispatch, so no
/// download, no network, and no timing is involved.
const SLOW_METHOD: &str = "data_gov.downloadResources";

/// A `tools/call` for the tool that maps to [`SLOW_METHOD`].
fn slow_call(id: i64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": "data_gov_download_resources",
            "arguments": {"datasetId": "held-dataset"}
        }
    })
}

/// A `data_gov.downloadResources` call can hold a request open for many
/// minutes. Everything behind it in the pipeline used to wait, because the
/// loop awaited each dispatch before reading the next line.
///
/// The gate is what makes this an ordering assertion: request 1 cannot have
/// finished when request 2 is answered, because nothing has released it.
#[tokio::test]
async fn a_held_request_does_not_delay_the_one_behind_it() {
    let mock = MockServer::start().await;
    let (server, release) = test_server_with_gate(&mock.uri(), SLOW_METHOD);
    let mut session = Session::start(server);

    session.send(&slow_call(1)).await;
    session
        .send(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
        .await;

    let first = session.next_response().await;
    assert_eq!(
        first.get("id"),
        Some(&json!(2)),
        "request 1 is held and cannot have produced this; a loop that awaited \
         it before reading the next line could produce nothing here at all: {first}"
    );
    assert!(
        first["result"]["tools"].is_array(),
        "and it must be a real tools/list result: {first}"
    );

    release.notify_one();
    let second = session.next_response().await;
    assert_eq!(
        second.get("id"),
        Some(&json!(1)),
        "the held request is still owed an answer once it is released: {second}"
    );

    assert!(session.finish().await.is_empty());
}

/// The issue's other example: `shutdown` queued behind a download too, so a
/// client could not even end the session promptly.
#[tokio::test]
async fn a_held_request_does_not_delay_shutdown() {
    let mock = MockServer::start().await;
    let (server, release) = test_server_with_gate(&mock.uri(), SLOW_METHOD);
    let mut session = Session::start(server);

    session.send(&slow_call(1)).await;
    session
        .send(&json!({"jsonrpc": "2.0", "id": 2, "method": "shutdown"}))
        .await;

    let first = session.next_response().await;
    assert_eq!(first.get("id"), Some(&json!(2)), "got: {first}");

    release.notify_one();
    session.finish().await;
}

/// MCP ping: "The receiver MUST respond promptly with an empty response",
/// `{"jsonrpc": "2.0", "id": "123", "result": {}}`.
///
/// A keepalive that answers -32601 is worse than none: the client reads the
/// error as a dead connection and may tear the session down.
#[tokio::test]
async fn ping_is_answered_with_an_empty_result() {
    let mock = MockServer::start().await;
    let server = std::sync::Arc::new(test_server(&mock.uri()));

    let responses = drive(
        &server,
        b"{\"jsonrpc\":\"2.0\",\"id\":\"123\",\"method\":\"ping\"}\n",
    )
    .await;

    assert_eq!(responses.len(), 1, "exactly one response: {responses:?}");
    assert_eq!(responses[0].get("id"), Some(&json!("123")));
    assert_eq!(
        responses[0].get("result"),
        Some(&json!({})),
        "the spec's own example result is the empty object: {}",
        responses[0]
    );
}

/// A keepalive is only a keepalive if it answers while the server is busy.
#[tokio::test]
async fn ping_is_answered_while_another_request_is_held() {
    let mock = MockServer::start().await;
    let (server, release) = test_server_with_gate(&mock.uri(), SLOW_METHOD);
    let mut session = Session::start(server);

    session.send(&slow_call(1)).await;
    session
        .send(&json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}))
        .await;

    let first = session.next_response().await;
    assert_eq!(first.get("id"), Some(&json!(2)), "got: {first}");
    assert_eq!(first.get("result"), Some(&json!({})));

    release.notify_one();
    session.finish().await;
}

/// MCP cancellation: receivers SHOULD "stop processing the cancelled request"
/// and "not send a response for the cancelled request".
///
/// The gate is what makes this deterministic: the cancellation is read while
/// request 1 is provably still running, so the race the spec warns about
/// cannot occur here.
#[tokio::test]
async fn a_cancelled_request_is_never_answered() {
    let mock = MockServer::start().await;
    let (server, release) = test_server_with_gate(&mock.uri(), SLOW_METHOD);
    let mut session = Session::start(server);

    session.send(&slow_call(1)).await;
    session
        .send(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {"requestId": 1, "reason": "user changed their mind"}
        }))
        .await;

    // Released so that a server which ignored the cancellation answers rather
    // than hanging, and fails one of the two assertions below instead.
    release.notify_one();

    session
        .send(&json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}))
        .await;

    let response = session.next_response().await;
    assert_eq!(
        response.get("id"),
        Some(&json!(2)),
        "the cancelled request must produce no response at all: {response}"
    );

    let rest = session.finish().await;
    assert!(
        rest.is_empty(),
        "no answer to a cancelled request may arrive later either: {rest:?}"
    );
}

/// The session survives a cancellation, so a client that cancels one request
/// can keep using the connection.
#[tokio::test]
async fn a_cancellation_leaves_the_session_usable() {
    let mock = MockServer::start().await;
    let (server, release) = test_server_with_gate(&mock.uri(), SLOW_METHOD);
    let mut session = Session::start(server);

    session.send(&slow_call(1)).await;
    session
        .send(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {"requestId": 1}
        }))
        .await;
    release.notify_one();

    for id in 2..=4 {
        session
            .send(&json!({"jsonrpc": "2.0", "id": id, "method": "tools/list"}))
            .await;
        let response = session.next_response().await;
        assert_eq!(response.get("id"), Some(&json!(id)), "got: {response}");
    }

    assert!(session.finish().await.is_empty());
}

/// "Receivers MAY ignore cancellation notifications if the referenced request
/// is unknown", and "Invalid cancellation notifications SHOULD be ignored".
/// Ignored means the session carries on, not that it stops.
#[tokio::test]
async fn a_malformed_or_unknown_cancellation_is_ignored() {
    let mock = MockServer::start().await;
    let server = std::sync::Arc::new(test_server(&mock.uri()));

    let mut input = String::new();
    for params in [
        json!({"requestId": 4242}),
        json!({"requestId": "never-issued"}),
        json!({}),
        json!(null),
    ] {
        input.push_str(&format!(
            "{}\n",
            json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": params})
        ));
    }
    input.push_str(&format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})
    ));

    let responses = drive(&server, input.as_bytes()).await;

    assert_eq!(
        responses.len(),
        1,
        "a notification is never answered, and none of these may end the \
         session: {responses:?}"
    );
    assert_eq!(responses[0].get("id"), Some(&json!(1)));
}

/// Concurrent dispatch means concurrent writers unless something serializes
/// them. Every response here is several kilobytes, so two writes that
/// interleaved would leave a line that is not JSON, and the driver parses
/// every line it collects.
#[tokio::test]
async fn concurrent_responses_are_written_as_whole_lines() {
    const REQUESTS: i64 = 50;

    let mock = MockServer::start().await;
    let server = std::sync::Arc::new(test_server(&mock.uri()));

    let input: String = (1..=REQUESTS)
        .map(|id| {
            format!(
                "{}\n",
                json!({"jsonrpc": "2.0", "id": id, "method": "tools/list"})
            )
        })
        .collect();

    let responses = drive(&server, input.as_bytes()).await;

    assert_eq!(
        responses.len() as i64,
        REQUESTS,
        "every request is answered exactly once"
    );
    let ids: HashSet<i64> = responses
        .iter()
        .filter_map(|response| response.get("id").and_then(Value::as_i64))
        .collect();
    assert_eq!(
        ids,
        (1..=REQUESTS).collect::<HashSet<i64>>(),
        "no id may be lost or duplicated"
    );
    for response in &responses {
        assert!(
            response["result"]["tools"].is_array(),
            "every line must be a whole, parseable result: {response}"
        );
    }
}
