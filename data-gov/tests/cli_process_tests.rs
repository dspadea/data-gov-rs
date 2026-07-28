//! Process-level tests that drive the real `data-gov` binary: argv parsed
//! by clap, stdout and stderr captured *separately*, and the real exit
//! code observed.
//!
//! Nothing else in this crate's test suite does this. The unit tests in
//! `tools/cli/ui/*.rs` exercise the pure decision logic (`ReplCommand`
//! parsing, `ColorMode`, ...) but never the glue in `run()` and
//! `run_cli_mode` that wires a decision to an actual effect — stdout vs.
//! stderr, a process exit code, `colored`'s process-global state. A
//! mutation review found four regressions that survived the full 90-test
//! suite specifically because nothing here drove the process for real; see
//! the sibling `cli_color_tests.rs` and `cli_dispatch_tests.rs` for the
//! other two.
//!
//! Every test here is network-free. The one exception —
//! `search_with_no_matches_exits_zero` — talks to a `wiremock` server on
//! loopback, the same pattern `tests/download_tests.rs` already uses; it
//! never reaches data.gov.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

/// Path to `data-gov-catalog`'s own captured-and-manifest-tracked
/// "zero results" fixture (see that crate's `tests/fixtures/MANIFEST.json`
/// and CLAUDE.md's "Capture the negatives too" section). Reused rather
/// than hand-written, so this test's mock response is a real captured
/// envelope shape (`{"results": [], "sort": "relevance"}` — no `total`,
/// no cursor), not a guess at one.
fn search_no_matches_fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../data-gov-catalog/tests/fixtures/search_no_matches.json")
}

/// Start a mock Catalog API that answers every `/search` with zero
/// results, and return the `Runtime` and `MockServer` that keep it alive.
/// Callers must keep both bindings alive for as long as the child process
/// needs to reach the server — dropping either shuts it down.
fn mock_search_no_matches_server() -> (tokio::runtime::Runtime, wiremock::MockServer) {
    let rt = tokio::runtime::Runtime::new().expect("mock server runtime");
    let server = rt.block_on(async {
        let server = wiremock::MockServer::start().await;
        let body = std::fs::read_to_string(search_no_matches_fixture_path()).expect(
            "search_no_matches.json fixture must exist (captured by data-gov-catalog's \
             scripts/capture-fixtures.sh)",
        );

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/search"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_raw(body, "application/json"),
            )
            .mount(&server)
            .await;

        server
    });
    (rt, server)
}

// --- #68: error text must land on stderr, never stdout, and a failing
// command must exit non-zero ---

#[test]
fn unknown_command_error_lands_on_stderr_not_stdout_and_exits_nonzero() {
    let dir = tempfile::tempdir().expect("tempdir");

    let assert = Command::cargo_bin("data-gov")
        .expect("data-gov binary must build")
        .current_dir(dir.path())
        .arg("definitely-not-a-real-command-xyz123")
        .assert()
        .failure();

    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("Error:"),
        "the error message must land on stderr, got stderr={stderr:?}"
    );
    assert!(
        !stdout.contains("Error:"),
        "stdout must carry no error text, got stdout={stdout:?}"
    );
    assert_ne!(
        output.status.code(),
        Some(0),
        "an unknown command must exit non-zero, so `set -e` and callers checking $? both see it"
    );
}

#[test]
fn successful_command_writes_nothing_to_stderr() {
    let dir = tempfile::tempdir().expect("tempdir");

    Command::cargo_bin("data-gov")
        .expect("data-gov binary must build")
        .current_dir(dir.path())
        .arg("help")
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

#[test]
fn search_with_no_matches_exits_zero() {
    // A normal negative answer is not a failure (AGENTS.md /
    // CLAUDE.md): zero search hits is a successful search. This drives
    // that claim through the real process and its real exit code, not an
    // in-process `Result` — the mutation this whole harness exists to
    // catch could just as easily have turned "empty results" into a
    // nonzero exit as it could have swapped stdout for stderr.
    let (rt, server) = mock_search_no_matches_server();

    Command::cargo_bin("data-gov")
        .expect("data-gov binary must build")
        .env("DATA_GOV_BASE_URL", server.uri())
        .args([
            "search",
            "a-query-the-mock-answers-with-zero-hits-regardless",
            "5",
        ])
        .assert()
        .success();

    drop(server);
    drop(rt);
}
