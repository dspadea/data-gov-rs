//! Tests for concurrent dispatch, cancellation, the keepalive, and the limits
//! the read loop admits work under.
//!
//! Nothing here waits on a clock to decide whether it passed. The slow request
//! is held at a gate the test owns, so "the second request was answered while
//! the first was still running" is a fact about ordering rather than a
//! measurement. The only time bound is the one inside
//! [`crate::test_support::Session`], which turns a run loop that has stopped
//! answering into a named failure instead of a hung suite.
//!
//! The two limits are stated here as literals rather than read out of the
//! server, so that changing a constant fails a test instead of silently
//! agreeing with it.

use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use wiremock::matchers::{method as wm_method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::test_support::{
    INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR, Session, drive, error_code,
    error_message, gated_server, gated_twice, scratch_dir, test_server, test_server_with_gate,
    timed_server,
};

/// The largest message the server accepts, in bytes, newline included.
///
/// See the server's `MAX_LINE_BYTES` for how the number was chosen against a
/// real payload.
const ACCEPTED_LINE_BYTES: usize = 1024 * 1024;

/// How many requests the server dispatches at once before it stops accepting.
const DISPATCH_SLOTS: usize = 256;

/// The tool method the gate holds in these tests. Held at the dispatch, so no
/// download, no network, and no timing is involved.
const SLOW_METHOD: &str = "data_gov.downloadResources";

/// A tool method the per-request budget does bound, for the tests that check
/// the budget still works. [`SLOW_METHOD`] cannot serve here: it is the one
/// tool the registry marks exempt.
const BOUNDED_METHOD: &str = "data_gov.search";

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

/// A `tools/call` for the tool that maps to [`BOUNDED_METHOD`].
fn bounded_call(id: i64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": "data_gov_search",
            "arguments": {"query": "held"}
        }
    })
}

/// The same tool, named directly rather than through `tools/call`.
///
/// The epilogue hold matches the envelope method, so a test that needs it
/// takes this door. Both doors reach the same handler.
fn slow_method_call(id: i64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": SLOW_METHOD,
        "params": {"datasetId": "held-dataset"}
    })
}

/// `notifications/cancelled` naming `id`.
fn cancellation_of(id: i64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": {"requestId": id, "reason": "retrying"}
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

// ---------------------------------------------------------------------------
// The per-request timeout
// ---------------------------------------------------------------------------

/// A bounded request that never finishes must not hold its slot forever. The
/// gate is never released here, so the wait is unbounded and any finite timeout
/// has to fire: the outcome does not depend on how long the test takes.
///
/// A tool that timed out reports it the way any other execution failure does,
/// because the model can act on it - retry with fewer files, or a narrower
/// filter.
///
/// The name says *bounded* because that is all this exercises. Exempting the
/// download tool from the budget must not weaken the budget for anything else,
/// and the mirror case is
/// [`a_progressing_download_outlives_the_wall_clock_budget`].
#[tokio::test]
async fn a_bounded_tool_that_outruns_the_timeout_is_answered_with_a_tool_error() {
    let mock = MockServer::start().await;
    let (server, _never_released) =
        gated_server(&mock.uri(), BOUNDED_METHOD, Duration::from_millis(50));
    let mut session = Session::start(server);

    session.send(&bounded_call(1)).await;

    let response = session.next_response().await;
    assert_eq!(response.get("id"), Some(&json!(1)), "got: {response}");
    assert_eq!(
        response["result"]["isError"],
        json!(true),
        "an abandoned tool call is a tool execution error: {response}"
    );
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        text.contains(BOUNDED_METHOD),
        "the message must name the call that was abandoned: {response}"
    );

    session.finish().await;
}

/// A protocol method has no tool result to travel in, so its timeout is a
/// JSON-RPC error. JSON-RPC 2.0 section 5.1 reserves -32000 to -32099 for
/// "implementation-defined server-errors" and assigns the rest, so the code
/// has to sit in that band and must not collide with a defined one.
#[tokio::test]
async fn a_protocol_method_that_outruns_the_timeout_is_a_server_error() {
    let mock = MockServer::start().await;
    let (server, _never_released) =
        gated_server(&mock.uri(), "tools/list", Duration::from_millis(50));
    let mut session = Session::start(server);

    session
        .send(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .await;

    let response = session.next_response().await;
    let code = error_code(&response);
    assert!(
        (-32099..=-32000).contains(&code),
        "a timeout is an implementation-defined server error, got {code}: {response}"
    );
    for reserved in [
        PARSE_ERROR,
        INVALID_REQUEST,
        METHOD_NOT_FOUND,
        INVALID_PARAMS,
        -32603,
    ] {
        assert_ne!(
            code, reserved,
            "a timeout must not reuse a code JSON-RPC already defines: {response}"
        );
    }

    session.finish().await;
}

/// The product rule: work that is progressing is never killed by a wall-clock
/// budget. `data_gov.downloadResources` is the one tool whose honest runtime is
/// the size of a file over the width of a link, so the request budget cannot
/// say anything true about it - a 222 MB dataset over a 250 KB/s link is a
/// quarter of an hour of healthy transfer, and the old blanket bound killed it
/// mid-flight after the user had already waited that long.
///
/// The transfer here is deliberately slower than the budget the server is
/// given. Nothing about the numbers decides the outcome in the passing
/// direction: with the exemption in place no wall clock applies at all, so a
/// loaded machine cannot fail this. Without it the call is abandoned, because
/// the response cannot arrive before the budget expires.
///
/// What still stops a download is unchanged and untested here: reqwest's
/// `read_timeout` on the download client, which fires when no byte arrives for
/// `download_timeout_secs`, and `notifications/cancelled`.
#[tokio::test]
async fn a_progressing_download_outlives_the_wall_clock_budget() {
    /// Shorter than the transfer, so a bounded download cannot survive it.
    const REQUEST_BUDGET: Duration = Duration::from_millis(50);
    /// Longer than the budget, so the transfer provably outruns it.
    const TRANSFER_TIME: Duration = Duration::from_millis(400);

    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/api/dataset/slow-but-alive"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "slug": "slow-but-alive",
                "title": "Slow But Alive",
                "dcat": {
                    "@type": "dcat:Dataset",
                    "title": "Slow But Alive",
                    "distribution": [{
                        "@type": "dcat:Distribution",
                        "title": "bulk",
                        "downloadURL": format!("{}/bulk.csv", mock.uri()),
                        "mediaType": "text/csv"
                    }]
                }
            }],
            "sort": "relevance"
        })))
        .mount(&mock)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/bulk.csv"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("a,b\n1,2\n")
                .set_delay(TRANSFER_TIME),
        )
        .mount(&mock)
        .await;

    let server = timed_server(&mock.uri(), REQUEST_BUDGET);
    let output_dir = scratch_dir("slow-but-alive");

    let value = server
        .dispatch(
            "tools/call",
            Some(json!({
                "name": "data_gov_download_resources",
                "arguments": {
                    "datasetId": "slow-but-alive",
                    "outputDir": output_dir.to_string_lossy(),
                    "datasetSubdirectory": false
                }
            })),
        )
        .await
        .expect("a download is a tool result, not a JSON-RPC error");

    let _ = std::fs::remove_dir_all(&output_dir);

    assert_eq!(
        value["isError"],
        json!(false),
        "a transfer that was progressing must not be abandoned: {value}"
    );
    let summary = &value["structuredContent"];
    assert_eq!(
        summary["successfulCount"],
        json!(1),
        "the file must actually have arrived: {value}"
    );
    assert_eq!(
        summary["downloads"][0]["status"], "success",
        "the file must actually have arrived: {value}"
    );
}

// ---------------------------------------------------------------------------
// Request id reuse
// ---------------------------------------------------------------------------

/// MCP base protocol: "The request ID MUST NOT have been previously used by
/// the requestor within the same session."
///
/// The cancellation registry is keyed by request id, so a second request
/// arriving under an id already in flight has nowhere of its own to live. It
/// must be refused rather than allowed to displace the first, and the first
/// must still be answered.
#[tokio::test]
async fn a_request_id_already_in_flight_is_refused_and_the_first_still_answers() {
    let mock = MockServer::start().await;
    let (server, release) = test_server_with_gate(&mock.uri(), SLOW_METHOD);
    let mut session = Session::start(server);

    // Held, so it is provably still in flight when the duplicate arrives.
    session.send(&slow_call(1)).await;
    session
        .send(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .await;

    let refusal = session.next_response().await;
    assert_eq!(refusal.get("id"), Some(&json!(1)), "got: {refusal}");
    assert_eq!(
        error_code(&refusal),
        INVALID_REQUEST,
        "an id already in flight cannot identify a second request: {refusal}"
    );

    release.notify_one();
    let answer = session.next_response().await;
    assert_eq!(
        answer.get("id"),
        Some(&json!(1)),
        "the first request keeps its id and its answer: {answer}"
    );
    assert!(
        answer.get("result").is_some(),
        "the duplicate must not have cancelled the request it collided with: {answer}"
    );

    assert!(session.finish().await.is_empty());
}

/// The other side of it: reusing an id once the first request has been
/// answered is not a collision, and a client that numbers requests from a
/// small pool must keep working.
#[tokio::test]
async fn a_request_id_is_reusable_once_its_request_has_been_answered() {
    let mock = MockServer::start().await;
    let server = std::sync::Arc::new(test_server(&mock.uri()));
    let mut session = Session::start(server);

    for _ in 0..3 {
        session
            .send(&json!({"jsonrpc": "2.0", "id": 7, "method": "ping"}))
            .await;
        let response = session.next_response().await;
        assert_eq!(response.get("id"), Some(&json!(7)), "got: {response}");
        assert_eq!(
            response.get("result"),
            Some(&json!({})),
            "an id freed by its answer is usable again: {response}"
        );
    }

    assert!(session.finish().await.is_empty());
}

/// Cancelling a request frees its id, and a client that cancels and retries
/// under the same id - a normal thing to do - must be answered.
///
/// This is the sequential half of the epilogue rule in `accept`: a cancelled
/// task must not deregister an id that a later request has since claimed. The
/// interleaved half - the same three events with the completion landing last -
/// is
/// [`a_completion_does_not_deregister_a_retry_that_claimed_its_id`].
#[tokio::test]
async fn an_id_freed_by_cancellation_can_be_used_again() {
    let mock = MockServer::start().await;
    let (server, release) = test_server_with_gate(&mock.uri(), SLOW_METHOD);
    let mut session = Session::start(server);

    session.send(&slow_call(1)).await;
    session
        .send(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {"requestId": 1, "reason": "retrying"}
        }))
        .await;
    release.notify_one();

    session
        .send(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .await;

    let retried = session.next_response().await;
    assert_eq!(retried.get("id"), Some(&json!(1)), "got: {retried}");
    assert!(
        retried["result"]["tools"].is_array(),
        "the retry under the freed id must be answered, not refused and not \
         swallowed: {retried}"
    );

    assert!(
        session.finish().await.is_empty(),
        "and the cancelled request is still never answered"
    );
}

/// A request that finishes must give up its own slot in the cancellation
/// registry and nobody else's.
///
/// The window that breaks this is narrow but ordinary: a cancellation arrives
/// after a task has committed to answering, frees the id, and a retry under
/// that id claims it before the finishing task removes its entry. Removing by
/// id alone at that moment takes the retry's entry out, drops its cancellation
/// sender, and the retry is abandoned in silence - no response, no error, and
/// a client waiting until its own timeout.
///
/// Every step of that is reachable from the wire except the last: nothing
/// between the handler resolving and the removal awaits anything a client can
/// influence, so the two read as one step from outside. `tokio::select!` keeps
/// the branch it did not take alive while the branch body runs, which is why
/// the cancellation is delivered to a receiver that will never look at it. The
/// epilogue hold is what turns that window into an order this test states.
#[tokio::test]
async fn a_completion_does_not_deregister_a_retry_that_claimed_its_id() {
    let mock = MockServer::start().await;
    let gated = gated_twice(&mock.uri(), SLOW_METHOD);
    let mut session = Session::start(std::sync::Arc::clone(&gated.server));

    // Registers id 1 and stops at the dispatch hold.
    session.send(&slow_method_call(1)).await;

    // Let its handler resolve. The task commits to the completion arm and
    // stops in the epilogue, still holding id 1.
    gated.dispatch.notify_one();
    gated.arrived.notified().await;

    // The cancellation frees id 1 while that task sits there. Its sender is
    // still live, so the notification is delivered to a receiver that has
    // already lost its race.
    session.send(&cancellation_of(1)).await;

    // The retry claims the freed id, and stops at the dispatch hold in turn.
    session.send(&slow_method_call(1)).await;

    // The ping sits behind the retry in the stream, so its answer is proof
    // that the reading task has already registered the retry. Without that,
    // the ordering below would be a hope rather than a fact.
    session
        .send(&json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}))
        .await;
    let ping = session.next_response().await;
    assert_eq!(ping.get("id"), Some(&json!(2)), "got: {ping}");

    // The first task now finishes. This is the moment the defect fires.
    gated.epilogue.notify_one();
    let raced = session.next_response().await;
    assert_eq!(
        raced.get("id"),
        Some(&json!(1)),
        "the cancellation lost its race, so the first request still answers: {raced}"
    );

    // Release the retry: its dispatch hold, then its own epilogue hold.
    gated.dispatch.notify_one();
    gated.epilogue.notify_one();

    let rest = session.finish().await;
    assert_eq!(
        rest.len(),
        1,
        "the retry must be answered; a completion that deregisters by id alone \
         takes the retry's entry, drops its cancellation sender, and the retry \
         ends in silence: {rest:?}"
    );
    assert_eq!(
        rest[0].get("id"),
        Some(&json!(1)),
        "and it answers under the id it was sent with: {}",
        rest[0]
    );
}

/// A closed output means the session is over. Carrying on reading and
/// spawning after the client has gone executes work - downloads that write to
/// disk among it - that nobody will ever be told about.
#[tokio::test]
async fn a_closed_output_ends_the_session() {
    let mock = MockServer::start().await;
    let server = std::sync::Arc::new(test_server(&mock.uri()));

    let (mut requests, server_reader) = tokio::io::duplex(1 << 16);
    let (server_writer, responses) = tokio::io::duplex(1 << 16);
    // The client is gone before the first answer is written.
    drop(responses);

    let loop_task = tokio::spawn(async move { server.serve(server_reader, server_writer).await });

    // Keep offering work rather than sending a fixed amount and closing: the
    // loop must stop on its own, not because it ran out of input.
    let feeder = tokio::spawn(async move {
        for id in 1i64.. {
            let line = format!(
                "{}\n",
                json!({"jsonrpc": "2.0", "id": id, "method": "ping"})
            );
            if requests.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });

    let outcome = tokio::time::timeout(Duration::from_secs(10), loop_task)
        .await
        .expect("the loop must end once its output is gone, without waiting for EOF")
        .expect("the loop task must not panic");
    feeder.abort();

    assert!(
        outcome.is_err(),
        "a transport that cannot be written to is a failure, not a clean end: {outcome:?}"
    );
}

// ---------------------------------------------------------------------------
// The limits the read loop admits work under
// ---------------------------------------------------------------------------

/// A `ping` request padded to exactly `bytes`, newline included.
///
/// `ping` ignores its params, so the padding changes the size of the message
/// and nothing else about what the server has to do with it.
fn padded_ping(id: i64, bytes: usize) -> Vec<u8> {
    let empty = json!({
        "jsonrpc": "2.0", "id": id, "method": "ping", "params": {"pad": ""}
    })
    .to_string();
    let pad = bytes
        .checked_sub(empty.len() + 1)
        .expect("the requested size must leave room for the envelope");
    let line = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "ping",
            "params": {"pad": "x".repeat(pad)}
        })
    );
    assert_eq!(line.len(), bytes, "the padded line must be exactly {bytes}");
    line.into_bytes()
}

/// One JSON-RPC line, newline included.
fn line_of(message: &Value) -> Vec<u8> {
    format!("{message}\n").into_bytes()
}

/// A line past the cap is refused, and the session carries on.
///
/// Refusing matters more than the size: the server must not buffer a message
/// it has already decided not to serve. Measured before the cap existed, a
/// 1 GiB line peaked at 1046 MiB of resident memory, and a 256 MiB line that
/// was a *valid* request peaked at 4.09x its own size, because the raw buffer,
/// the parsed `Value`, the cloned id, and the serialised response all coexist.
///
/// The line is twice the cap rather than one byte over it, and that is load
/// bearing. An implementation that stops reading at the cap instead of
/// consuming to the newline leaves the remainder to be read as the next
/// message - and at one byte over, that remainder is a byte or two, which
/// trims to nothing and hides the defect. At twice the cap the leak is a
/// megabyte of rubbish and shows up as a third response.
#[tokio::test]
async fn a_line_over_the_cap_is_refused_and_its_tail_is_not_read_as_a_message() {
    let mock = MockServer::start().await;
    let server = Arc::new(test_server(&mock.uri()));

    let mut input = padded_ping(1, ACCEPTED_LINE_BYTES * 2);
    input.extend_from_slice(&line_of(
        &json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}),
    ));

    let responses = drive(&server, &input).await;

    assert_eq!(
        responses.len(),
        2,
        "exactly one refusal and one answer; a third response is the refused \
         line's tail being read as a message of its own: {responses:?}"
    );
    assert_eq!(
        responses[0].get("id"),
        Some(&json!(null)),
        "a line the server refused to read carries no recoverable id: {}",
        responses[0]
    );
    assert_eq!(
        error_code(&responses[0]),
        INVALID_REQUEST,
        "an over-long message is not a valid Request object: {}",
        responses[0]
    );
    assert!(
        error_message(&responses[0]).contains(&ACCEPTED_LINE_BYTES.to_string()),
        "the message must name the limit, or a client cannot act on it: {}",
        responses[0]
    );
    assert_eq!(
        responses[1].get("id"),
        Some(&json!(2)),
        "and the session survives the refusal: {}",
        responses[1]
    );
}

/// The boundary from above. One byte past the cap is already too much.
#[tokio::test]
async fn a_line_one_byte_over_the_cap_is_refused() {
    let mock = MockServer::start().await;
    let server = Arc::new(test_server(&mock.uri()));

    let responses = drive(&server, &padded_ping(1, ACCEPTED_LINE_BYTES + 1)).await;

    assert_eq!(responses.len(), 1, "got: {responses:?}");
    assert_eq!(
        responses[0].get("id"),
        Some(&json!(null)),
        "one byte past the cap is refused, not served: {}",
        responses[0]
    );
    assert_eq!(error_code(&responses[0]), INVALID_REQUEST);
}

/// The boundary itself. A cap that rejected the largest accepted size, or
/// accepted one byte past it, would pass a test written only against extremes.
#[tokio::test]
async fn a_line_at_exactly_the_cap_is_accepted() {
    let mock = MockServer::start().await;
    let server = Arc::new(test_server(&mock.uri()));

    let responses = drive(&server, &padded_ping(1, ACCEPTED_LINE_BYTES)).await;

    assert_eq!(responses.len(), 1, "got: {responses:?}");
    assert_eq!(responses[0].get("id"), Some(&json!(1)));
    assert_eq!(
        responses[0].get("result"),
        Some(&json!({})),
        "a message at the cap is served, not refused: {}",
        responses[0]
    );
}

/// The cap has to clear the largest message a real client sends, which is a
/// `downloadResources` call naming every distribution of a large dataset.
/// 10,000 indexes is far past anything data.gov carries, and it must still be
/// served rather than refused.
#[tokio::test]
async fn the_largest_realistic_tool_call_is_still_accepted() {
    let mock = MockServer::start().await;
    let server = Arc::new(test_server(&mock.uri()));

    let indexes: Vec<usize> = (0..10_000).collect();
    let call = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "data_gov_download_resources",
            "arguments": {
                "datasetId": "a-dataset-slug-at-the-catalog-truncation-cap-of-ninety-characters-000000000000000000",
                "distributionIndexes": indexes,
                "formats": ["CSV", "JSON", "XML", "GeoJSON", "SHP"],
                "outputDir": "/tmp/a/deliberately/long/output/path"
            }
        }
    });
    let input = line_of(&call);
    assert!(
        input.len() < ACCEPTED_LINE_BYTES,
        "the largest realistic call is {} bytes, and must sit under the cap",
        input.len()
    );

    let responses = drive(&server, &input).await;

    assert_eq!(responses.len(), 1, "got: {responses:?}");
    assert_eq!(
        responses[0].get("id"),
        Some(&json!(1)),
        "it reached a handler; a refused line would answer with a null id: {}",
        responses[0]
    );
}

/// The read loop must stop accepting once every dispatch slot is taken.
///
/// Without a bound, reading is cheap and dispatching is not, so a client that
/// pipelines faster than the server answers queues unbounded work: the
/// response channel throttles the writing, but only after the work is already
/// done, so it does not limit the work. Measured before the bound, with stdout
/// never drained, resident memory reached 27.5 GB in 30 seconds and was still
/// climbing.
///
/// The ordering is the assertion, not a timing. Every slot is held, so an
/// unbounded loop answers the ping immediately while nothing else can; a
/// bounded one cannot answer it until a slot comes free.
#[tokio::test]
async fn the_read_loop_stops_accepting_once_every_dispatch_slot_is_taken() {
    const PING_ID: i64 = 1000;

    let mock = MockServer::start().await;
    let (server, release) = test_server_with_gate(&mock.uri(), SLOW_METHOD);
    let mut session = Session::start(server);

    // Fill every slot. None of these can finish: all are held.
    for id in 1..=DISPATCH_SLOTS as i64 {
        session.send(&slow_method_call(id)).await;
    }
    // One line past the bound.
    session
        .send(&json!({"jsonrpc": "2.0", "id": PING_ID, "method": "ping"}))
        .await;

    // Free exactly one slot.
    release.notify_one();

    let first = session.next_response().await;
    let answered = first.get("id").and_then(Value::as_i64).unwrap_or_default();
    assert!(
        (1..=DISPATCH_SLOTS as i64).contains(&answered),
        "the ping arrived while every dispatch slot was held, so it cannot be \
         answered first; an unbounded read loop dispatches it straight away \
         and answers it while all {DISPATCH_SLOTS} held requests wait: {first}"
    );

    // And it resumes: the slot freed above is what lets the ping through.
    let second = session.next_response().await;
    assert_eq!(
        second.get("id"),
        Some(&json!(PING_ID)),
        "the loop must accept again once a slot comes free: {second}"
    );

    // Release the rest. A `Notify` holds one permit at a time, so each held
    // request needs its own wake, and reading the answer it produces is what
    // proves the wake landed.
    for _ in 1..DISPATCH_SLOTS {
        release.notify_one();
        session.next_response().await;
    }

    assert!(
        session.finish().await.is_empty(),
        "every request is answered exactly once"
    );
}
