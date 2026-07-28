//! Unit tests for the CKAN client using mock HTTP responses.
//!
//! These tests validate URL construction, response parsing, and error handling
//! without requiring network access. Run with:
//!
//! ```bash
//! cargo test -p data-gov-ckan --test unit_tests
//! ```
//!
//! # Fixtures vs. hand-written bodies
//!
//! Where a wiremock response body needs to demonstrate real deserialization
//! (does a genuine CKAN payload parse correctly?), it is loaded from
//! `tests/fixtures/` via `include_str!` -- see `package_search_builds_correct_
//! url_and_parses_response`, `package_show_returns_full_dataset`,
//! `organization_list_with_sort_and_limit`, and the two `http_*` structured-
//! error tests. A hand-written body encodes the author's assumption about the
//! shape, and then the test only confirms that assumption; this crate's
//! standing example is #63 and #62, where every pre-existing hand-written
//! body used a UUID-shaped id and a small integer size because that is what
//! the (buggy) models expected.
//!
//! The bodies that remain hand-written all fall into shapes a live capture
//! cannot produce on demand, not shapes nobody thought to capture:
//!
//! - **Boundaries a real server will not spontaneously exhibit**: `rows=0`,
//!   an offset past the total, a query with special characters to URL-encode.
//! - **Malformed or absent data**: a non-JSON body, a `result` that is a bare
//!   string instead of an object, a `success: false` response with no
//!   `error` field at all. These test failure handling, so the failure has
//!   to be constructed.
//! - **A `success: false` combined with HTTP 200**: both captured error
//!   fixtures (`package_show_not_found.json`,
//!   `package_show_validation_error.json`) came back over a non-2xx status;
//!   no live capture in this branch's set exercises the success:false path
//!   at HTTP 200, so those tests stay synthetic.
//! - **Flat, low-risk response shapes**: `group_list`, and the four
//!   `*_autocomplete` endpoints, return either a bare array of strings or a
//!   3-4 field struct with no id or numeric field of the kind #63/#62
//!   affected. They were not part of #102's required capture set
//!   (`package_search`, `package_show`, `organization_list`, error
//!   responses), and a hand-written body for them carries little of the risk
//!   the fixture work exists to catch.
//! - **Client-side behavior with no response body to speak of**: the
//!   credential, user-agent, Debug-redaction, and timeout tests below
//!   configure a `Configuration` and assert what the *client* sends or
//!   prints, not how it parses a response.

use data_gov_ckan::{ApiKey, CkanClient, CkanError, Configuration};
use serde_json::json;
use std::sync::Arc;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Create a test client pointed at the given mock server.
fn test_client(base_url: &str) -> CkanClient {
    let config = Arc::new(Configuration {
        base_path: base_url.to_string(),
        user_agent: Some("test/1.0".to_string()),
        ..Configuration::default()
    });
    CkanClient::new(config)
}

// ---------------------------------------------------------------------------
// package_search
// ---------------------------------------------------------------------------

/// Response body is a real capture (see `fixture_parity_tests.rs` for the
/// deserialization-focused assertions on the same file). The query
/// parameters this test sends (`q=climate&rows=5&start=0`) are independent
/// of what the fixture was captured with -- this test is about request
/// shaping, not about the fixture's own provenance.
#[tokio::test]
async fn package_search_builds_correct_url_and_parses_response() {
    let server = MockServer::start().await;
    let body = include_str!("fixtures/package_search.json");

    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .and(query_param("q", "climate"))
        .and(query_param("rows", "5"))
        .and(query_param("start", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let result = client
        .package_search(Some("climate"), Some(5), Some(0), None)
        .await
        .expect("should succeed");

    assert_eq!(result.count, Some(1));
    let results = result.results.expect("should have results");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "432527ab-7aac-45b5-81d6-7597107a7013");
    assert_eq!(
        results[0].title.as_deref(),
        Some("Proactive Disclosure - Grants and Contributions")
    );
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

/// Boundary: limit=0 must be sent verbatim as `rows=0` — not silently dropped
/// or replaced with a default. Callers rely on this to fetch only counts.
#[tokio::test]
async fn package_search_with_limit_zero_sends_rows_zero_and_returns_empty_results() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .and(query_param("rows", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": { "count": 1234, "results": [] }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let result = client
        .package_search(Some("climate"), Some(0), None, None)
        .await
        .expect("rows=0 should be a valid request");

    assert_eq!(result.count, Some(1234));
    assert!(
        result.results.unwrap_or_default().is_empty(),
        "server returned empty results; client must not synthesize any"
    );
}

/// Boundary: an offset past the end of the result set returns success with an
/// empty results array. The client must parse this as valid data, not an error.
#[tokio::test]
async fn package_search_with_offset_past_total_parses_empty_results_without_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .and(query_param("start", "100000"))
        .and(query_param("rows", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": { "count": 42, "results": [] }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let result = client
        .package_search(Some("anything"), Some(10), Some(100000), None)
        .await
        .expect("offset past total is a valid server response, not a client error");

    assert_eq!(result.count, Some(42));
    assert!(result.results.unwrap_or_default().is_empty());
}

// ---------------------------------------------------------------------------
// package_show
// ---------------------------------------------------------------------------

/// Response body is a real capture: the same open.canada.ca dataset used by
/// `fixture_parity_tests.rs`'s #62 acceptance test, so this exercises
/// `package_show` end to end (client -> deserialization) against a record
/// that includes a resource over i32::MAX bytes.
#[tokio::test]
async fn package_show_returns_full_dataset() {
    let server = MockServer::start().await;
    let body = include_str!("fixtures/package_show.json");

    Mock::given(method("GET"))
        .and(path("/action/package_show"))
        .and(query_param("id", "my-dataset"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let pkg = client
        .package_show("my-dataset")
        .await
        .expect("should succeed");

    assert_eq!(pkg.name, "432527ab-7aac-45b5-81d6-7597107a7013");
    assert_eq!(
        pkg.title.as_deref(),
        Some("Proactive Disclosure - Grants and Contributions")
    );
    assert!(pkg.notes.is_some());

    let resources = pkg.resources.expect("should have resources");
    assert_eq!(resources.len(), 6);
    assert!(resources.iter().any(|r| r.format.as_deref() == Some("CSV")));
    assert!(
        resources.iter().any(|r| r.size == Some(2_290_761_766)),
        "the over-i32::MAX resource must survive deserialization"
    );
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

/// Response body is a real capture from open.canada.ca.
#[tokio::test]
async fn organization_list_with_sort_and_limit() {
    let server = MockServer::start().await;
    let body = include_str!("fixtures/organization_list.json");

    Mock::given(method("GET"))
        .and(path("/action/organization_list"))
        .and(query_param("sort", "name"))
        .and(query_param("limit", "3"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let orgs = client
        .organization_list(Some("name"), Some(3), None)
        .await
        .expect("should succeed");

    assert_eq!(
        orgs,
        vec!["16342451-canada-inc", "2canl", "3can", "3nih", "3nii"]
    );
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

// `http_404_returns_api_error_with_status` covers the fallback path
// deliberately: a plain-text, non-JSON error body (a stock web server or
// proxy error page rather than CKAN's own envelope) has no structure to
// parse, so the raw text is the best message available.
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

/// Real capture: open.canada.ca's 404 body for `package_show`. This is
/// CKAN's documented error envelope (`ErrorResponse` / `ErrorResponseError`
/// -- `__type` and `message`), which the crate ships and never referenced.
/// `ApiError.message` must be the parsed "Not found", not the raw envelope.
#[tokio::test]
async fn http_error_extracts_message_from_ckans_structured_error_envelope() {
    let server = MockServer::start().await;
    let body = include_str!("fixtures/package_show_not_found.json");

    Mock::given(method("GET"))
        .and(path("/action/package_show"))
        .respond_with(ResponseTemplate::new(404).set_body_raw(body, "application/json"))
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
            assert_eq!(message, "Not found");
        }
        other => panic!("expected ApiError, got: {:?}", other),
    }
}

/// Real capture: open.canada.ca's 409 body for `package_show` called with no
/// `id`. CKAN's validation-error shape replaces the documented `message`
/// field with per-field arrays, so it does not fit `ErrorResponseError`
/// (whose `message` is required). The message must still surface that
/// structure -- rendering the raw `error` object -- rather than discarding it
/// or falling back to a generic literal.
#[tokio::test]
async fn http_validation_error_renders_the_raw_error_object_as_message() {
    let server = MockServer::start().await;
    let body = include_str!("fixtures/package_show_validation_error.json");

    Mock::given(method("GET"))
        .and(path("/action/package_show"))
        .respond_with(ResponseTemplate::new(409).set_body_raw(body, "application/json"))
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
            assert_eq!(status, 409);
            assert!(message.contains("name_or_id"), "message: {message}");
            assert!(message.contains("Missing value"), "message: {message}");
            // Distinguishes "parsed just the error object" from "fell back to
            // the whole raw body", which also contains those substrings.
            assert!(
                !message.contains("\"success\""),
                "message should be the error object alone, not the whole envelope: {message}"
            );
        }
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
        CkanError::ApiError {
            status: 400,
            message,
        } => {
            assert_eq!(message, "something went wrong");
        }
        other => panic!("expected ApiError with status 400, got: {:?}", other),
    }
}

/// The HTTP-200-with-`success:false` path, CKAN's validation-error shape.
/// `ActionResponse` did not declare an `error` field at all, so serde simply
/// dropped it during deserialization and the whole object was lost -- not
/// just left unparsed, as with the non-2xx path.
#[tokio::test]
async fn success_false_with_validation_style_error_renders_the_raw_error_object() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": false, "result": null,
            "error": { "name_or_id": ["Missing value"], "__type": "Validation Error" }
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
        CkanError::ApiError {
            status: 400,
            message,
        } => {
            assert!(message.contains("name_or_id"), "message: {message}");
            assert!(message.contains("Missing value"), "message: {message}");
        }
        other => panic!("expected ApiError with status 400, got: {:?}", other),
    }
}

/// When CKAN sends `success: false` with no `error` field at all, there is
/// genuinely nothing to parse. The literal fallback message is correct here,
/// not a symptom of the bug the other tests in this section cover.
#[tokio::test]
async fn success_false_with_no_error_field_uses_fallback_message() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": false, "result": null
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
        CkanError::ApiError {
            status: 400,
            message,
        } => {
            assert_eq!(message, "CKAN API reported failure");
        }
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
    let err = client.package_show("test").await.expect_err("should fail");

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
    let err = client.package_show("test").await.expect_err("should fail");

    assert!(
        matches!(err, CkanError::ParseError(_)),
        "expected ParseError, got: {:?}",
        err
    );
}

/// `RequestError` is documented as covering connection failures, timeouts,
/// and DNS resolution -- transport, not content. A body that arrived intact
/// but is not valid JSON is a decode failure, so it must be `ParseError`, the
/// variant documented as covering exactly that. Before the fix, `call_action`
/// deserialized straight from the `reqwest::Response` in one step, so a
/// malformed body and a dropped connection produced the same variant and
/// were indistinguishable to a caller matching on it.
#[tokio::test]
async fn malformed_json_body_returns_parse_error() {
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
        matches!(err, CkanError::ParseError(_)),
        "expected ParseError, got: {:?}",
        err
    );
}

/// Reproduces the class of bug #72.2 was filed for: a connection that drops
/// mid-body on a *non*-2xx response must surface as `RequestError` -- the
/// variant documented as covering "connection failures, timeouts... HTTP
/// protocol errors" -- not `ApiError`, which is for a status CKAN itself
/// reported. Before the fix, `call_action`'s non-2xx branch read the body
/// with `.text().await.unwrap_or_else(|_| "Unknown error".to_string())`,
/// which discards the real transport error and reports the literal string
/// "Unknown error" as though CKAN had said so -- indistinguishable, to a
/// caller retrying on `RequestError`, from an actual permanent 500.
///
/// wiremock cannot reproduce this: it always serves a well-formed response.
/// A raw TCP listener sends `HTTP/1.1 500` with `Content-Length: 1000`,
/// writes 9 bytes of body, then closes -- the connection drops with 991
/// bytes still promised and never sent.
#[tokio::test]
async fn a_connection_dropped_mid_body_on_a_non_2xx_status_is_a_request_error() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("read local addr");

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept connection");
        // Drain the request so reqwest has genuinely sent it before we
        // respond; the exact bytes read don't matter.
        let mut buf = [0u8; 4096];
        let _ = socket.read(&mut buf).await;

        socket
            .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 1000\r\n\r\n")
            .await
            .expect("write status line and headers");
        socket
            .write_all(b"123456789")
            .await
            .expect("write partial body");
        // Dropping `socket` here closes the connection with 991 of the
        // promised 1000 bytes never sent.
    });

    let config = Arc::new(Configuration {
        base_path: format!("http://{addr}"),
        ..Configuration::default()
    });
    let client = CkanClient::new(config);

    let result = client.package_search(None, None, None, None).await;

    match result {
        Err(CkanError::RequestError(_)) => {}
        other => panic!(
            "a connection dropped mid-body on a non-2xx status must be \
             RequestError, not the API's own error, got: {other:?}"
        ),
    }
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

    let parse_err =
        CkanError::ParseError(serde_json::from_str::<serde_json::Value>("invalid").unwrap_err());
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
// Credentials and user agent (#56)
// ---------------------------------------------------------------------------
//
// `Configuration`'s credential and user-agent fields were accepted but never
// read: `call_action` built every request from `client.get(&url).query(...)`
// alone. A caller who believed a request was authenticated sent it anonymous,
// with nothing in the response distinguishing "not authorized" from
// "credential never left the client".

async fn mount_ok_package_search(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "help": "", "success": true,
            "result": { "count": 0, "results": [] }
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn configured_api_key_is_sent_as_authorization_header() {
    let server = MockServer::start().await;
    mount_ok_package_search(&server).await;

    let config = Arc::new(Configuration {
        base_path: server.uri(),
        api_key: Some(ApiKey {
            prefix: None,
            key: "my-secret-key".to_string(),
        }),
        ..Configuration::default()
    });
    CkanClient::new(config)
        .package_search(None, None, None, None)
        .await
        .expect("should succeed");

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert_eq!(requests.len(), 1);
    let auth = requests[0]
        .headers
        .get("authorization")
        .expect("Authorization header missing");
    assert_eq!(auth.to_str().unwrap(), "my-secret-key");
}

#[tokio::test]
async fn configured_api_key_with_prefix_is_sent_as_authorization_header() {
    let server = MockServer::start().await;
    mount_ok_package_search(&server).await;

    let config = Arc::new(Configuration {
        base_path: server.uri(),
        api_key: Some(ApiKey {
            prefix: Some("Token".to_string()),
            key: "my-secret-key".to_string(),
        }),
        ..Configuration::default()
    });
    CkanClient::new(config)
        .package_search(None, None, None, None)
        .await
        .expect("should succeed");

    let requests = server.received_requests().await.unwrap();
    let auth = requests[0].headers.get("authorization").unwrap();
    assert_eq!(auth.to_str().unwrap(), "Token my-secret-key");
}

#[tokio::test]
async fn configured_bearer_token_is_sent_as_authorization_header() {
    let server = MockServer::start().await;
    mount_ok_package_search(&server).await;

    let config = Arc::new(Configuration {
        base_path: server.uri(),
        bearer_access_token: Some("bearer-token-value".to_string()),
        ..Configuration::default()
    });
    CkanClient::new(config)
        .package_search(None, None, None, None)
        .await
        .expect("should succeed");

    let requests = server.received_requests().await.unwrap();
    let auth = requests[0].headers.get("authorization").unwrap();
    assert_eq!(auth.to_str().unwrap(), "Bearer bearer-token-value");
}

#[tokio::test]
async fn configured_basic_auth_is_sent_as_authorization_header() {
    let server = MockServer::start().await;
    mount_ok_package_search(&server).await;

    let config = Arc::new(Configuration {
        base_path: server.uri(),
        basic_auth: Some(("alice".to_string(), Some("hunter2".to_string()))),
        ..Configuration::default()
    });
    CkanClient::new(config)
        .package_search(None, None, None, None)
        .await
        .expect("should succeed");

    let requests = server.received_requests().await.unwrap();
    let auth = requests[0].headers.get("authorization").unwrap();
    // "Basic " + base64("alice:hunter2")
    assert_eq!(auth.to_str().unwrap(), "Basic YWxpY2U6aHVudGVyMg==");
}

#[tokio::test]
async fn no_credential_configured_sends_no_authorization_header() {
    let server = MockServer::start().await;
    mount_ok_package_search(&server).await;

    let config = Arc::new(Configuration {
        base_path: server.uri(),
        ..Configuration::default()
    });
    CkanClient::new(config)
        .package_search(None, None, None, None)
        .await
        .expect("should succeed");

    let requests = server.received_requests().await.unwrap();
    assert!(
        requests[0].headers.get("authorization").is_none(),
        "no credential was configured; the client must not send one anyway"
    );
}

#[tokio::test]
async fn configured_user_agent_is_sent_on_every_request() {
    let server = MockServer::start().await;
    mount_ok_package_search(&server).await;

    let config = Arc::new(Configuration {
        base_path: server.uri(),
        user_agent: Some("my-app/9.9".to_string()),
        ..Configuration::default()
    });
    CkanClient::new(config)
        .package_search(None, None, None, None)
        .await
        .expect("should succeed");

    let requests = server.received_requests().await.unwrap();
    let ua = requests[0]
        .headers
        .get("user-agent")
        .expect("User-Agent header missing");
    assert_eq!(ua.to_str().unwrap(), "my-app/9.9");
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[test]
fn default_configuration_has_expected_values() {
    let config = Configuration::default();
    // catalog.data.gov is a confirmed 404 (data.gov retired its CKAN
    // endpoint in 2026); open.canada.ca is a live, government-run CKAN
    // portal, verified responding, used as the default so a caller who runs
    // the crate's own quick-start example unmodified sees it actually work
    // (#72.3). Point Configuration::base_path at your own instance for any
    // real use.
    assert_eq!(config.base_path, "https://open.canada.ca/data/en/api/3");
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

// A `Configuration` is plausibly logged whole (`tracing::debug!(?config)`),
// not just wrapped in `CkanClient`, so `Debug` on `Configuration` itself must
// never print a credential in the clear.

#[test]
fn configuration_debug_redacts_api_key() {
    let config = Configuration {
        api_key: Some(ApiKey {
            prefix: None,
            key: "super-secret-api-key".to_string(),
        }),
        ..Configuration::default()
    };
    let debug = format!("{:?}", config);
    assert!(
        !debug.contains("super-secret-api-key"),
        "API key leaked in Configuration Debug output: {debug}"
    );
}

#[test]
fn configuration_debug_redacts_bearer_token() {
    let config = Configuration {
        bearer_access_token: Some("super-secret-bearer-token".to_string()),
        ..Configuration::default()
    };
    let debug = format!("{:?}", config);
    assert!(
        !debug.contains("super-secret-bearer-token"),
        "bearer token leaked in Configuration Debug output: {debug}"
    );
}

#[test]
fn configuration_debug_redacts_oauth_access_token() {
    let config = Configuration {
        oauth_access_token: Some("super-secret-oauth-token".to_string()),
        ..Configuration::default()
    };
    let debug = format!("{:?}", config);
    assert!(
        !debug.contains("super-secret-oauth-token"),
        "OAuth token leaked in Configuration Debug output: {debug}"
    );
}

#[test]
fn configuration_debug_redacts_basic_auth_password() {
    let config = Configuration {
        basic_auth: Some((
            "alice".to_string(),
            Some("super-secret-password".to_string()),
        )),
        ..Configuration::default()
    };
    let debug = format!("{:?}", config);
    assert!(
        !debug.contains("super-secret-password"),
        "basic auth password leaked in Configuration Debug output: {debug}"
    );
}

// ---------------------------------------------------------------------------
// Timeouts (#48)
// ---------------------------------------------------------------------------
//
// `Configuration::default()` built its client with a bare `reqwest::Client::
// new()`: no connect timeout, no overall request timeout. A host that
// accepts the TCP/TLS connection but never sends response headers -- a
// partial outage, a blackholing middlebox -- hangs the caller forever. The
// MCP server's run loop awaits each request to completion before reading the
// next line, so one stalled call blocks every request after it, including
// `shutdown`.

#[tokio::test]
async fn a_request_against_a_non_responding_endpoint_errors_within_the_configured_timeout() {
    let server = MockServer::start().await;

    // Far longer than the client's configured timeout below, so a client
    // that ignores its own timeout would make this test hang instead of
    // fail -- a stronger signal than a merely-slow response would give.
    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"help": "", "success": true, "result": {}}))
                .set_delay(std::time::Duration::from_secs(10)),
        )
        .mount(&server)
        .await;

    let short_timeout_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(100))
        .build()
        .expect("client with just a timeout must build");
    let config = Arc::new(Configuration {
        base_path: server.uri(),
        client: short_timeout_client,
        ..Configuration::default()
    });

    let started = std::time::Instant::now();
    let result = CkanClient::new(config)
        .package_search(None, None, None, None)
        .await;
    let elapsed = started.elapsed();

    assert!(
        result.is_err(),
        "a request against a server that never responds within the timeout must error"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "expected the 100ms client timeout to fire well before the mock's \
         10s delay; took {elapsed:?}"
    );
}

#[test]
fn default_configuration_sets_explicit_connect_and_request_timeouts() {
    let config = Configuration::default();
    assert_eq!(config.connect_timeout, std::time::Duration::from_secs(10));
    assert_eq!(config.timeout, std::time::Duration::from_secs(30));
}

/// The idiomatic way to shorten a timeout is exactly what a consumer reaches
/// for: `Configuration { timeout: ..., ..Configuration::default() }`. But
/// `Configuration::default()` already built `client` from its own 10s/30s
/// values before the struct literal's `connect_timeout`/`timeout` fields are
/// applied, so this pattern silently keeps the old client -- the two new
/// fields sit in the struct doing nothing. A mock delays 300ms; both
/// timeouts are set to 50ms via struct-update syntax, so a client that
/// actually honoured them would error well under 300ms.
#[tokio::test]
async fn setting_timeout_via_struct_update_syntax_has_no_effect_on_the_built_client() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"help": "", "success": true, "result": {}}))
                .set_delay(std::time::Duration::from_millis(300)),
        )
        .mount(&server)
        .await;

    let config = Arc::new(Configuration {
        base_path: server.uri(),
        connect_timeout: std::time::Duration::from_millis(50),
        timeout: std::time::Duration::from_millis(50),
        ..Configuration::default()
    });

    let started = std::time::Instant::now();
    let result = CkanClient::new(config)
        .package_search(None, None, None, None)
        .await;
    let elapsed = started.elapsed();

    assert!(
        result.is_err(),
        "a 50ms timeout set via struct-update syntax must bound a 300ms \
         delayed response, but the call returned {result:?} after {elapsed:?}"
    );
}
