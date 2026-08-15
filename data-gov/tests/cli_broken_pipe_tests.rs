//! Process-level tests for #115: a reader that closes the pipe early must
//! not crash the CLI.
//!
//! `data-gov list organizations | head -5` panicked with exit 101 and a
//! Rust backtrace note, because `println!` panics when the write fails and
//! `head` closes its end of the pipe after five lines. Piping into a reader
//! that quits early is one of the most ordinary ways a Unix CLI is used, so
//! the tool has to survive it.
//!
//! Every test here closes the read end of a real pipe *before* the child
//! writes anything, so the child's first write to stdout fails with
//! `ErrorKind::BrokenPipe` with no timing race at all. Driving a real
//! `| head` would depend on whether the child happened to outrun the
//! reader; this does not.
//!
//! The project forbids `unsafe` (CLAUDE.md), which rules out the usual
//! `signal(SIGPIPE, SIG_DFL)` fix, so the contract under test is the safe
//! one: recognise the closed pipe at the print boundary and exit quietly.

use std::io::Read;
use std::process::{Command, Stdio};

/// Path to the `data-gov` binary under test, built by cargo.
fn data_gov_binary() -> std::path::PathBuf {
    assert_cmd::cargo::cargo_bin("data-gov")
}

/// Outcome of running a command with its stdout wired to a pipe whose read
/// end is already closed.
struct ClosedPipeRun {
    status: std::process::ExitStatus,
    stderr: String,
}

/// Run `data-gov <args>` with stdout connected to a pipe that has no
/// reader, and stderr captured.
///
/// The read end is dropped before the child is spawned, so there is no
/// window in which the child could write successfully. `XDG_CONFIG_HOME`
/// points at an empty directory so the run does not depend on whoever is
/// running the test having a `config.toml` (#86).
fn run_with_closed_stdout(args: &[&str]) -> ClosedPipeRun {
    let config_home = tempfile::tempdir().expect("tempdir");
    let (reader, writer) = std::io::pipe().expect("create a pipe");
    drop(reader);

    let mut child = Command::new(data_gov_binary())
        .args(args)
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_PROGRESS", "1")
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn data-gov");

    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr is piped")
        .read_to_string(&mut stderr)
        .expect("read stderr");
    let status = child.wait().expect("wait for data-gov");

    ClosedPipeRun { status, stderr }
}

#[test]
fn a_closed_stdout_pipe_does_not_panic() {
    let run = run_with_closed_stdout(&["--color", "never", "help"]);

    assert!(
        !run.stderr.contains("panicked"),
        "a closed stdout pipe must not panic, got stderr: {}",
        run.stderr
    );
}

#[test]
fn a_closed_stdout_pipe_exits_zero() {
    let run = run_with_closed_stdout(&["--color", "never", "help"]);

    assert_eq!(
        run.status.code(),
        Some(0),
        "a reader that quits early is not an error: the command did its job \
         and the reader stopped listening. Got exit {:?}, stderr: {}",
        run.status.code(),
        run.stderr
    );
}

#[test]
fn a_closed_stdout_pipe_prints_no_backtrace_note() {
    let run = run_with_closed_stdout(&["--color", "never", "help"]);

    assert!(
        !run.stderr.contains("RUST_BACKTRACE"),
        "a closed pipe is ordinary Unix usage, not a crash to be debugged; \
         got stderr: {}",
        run.stderr
    );
}

/// The quiet exit must be specific to a closed pipe, not a blanket
/// "swallow everything stdout does". An invalid command still has to fail
/// loudly on stderr, which is a different fd and is still open here.
#[test]
fn a_closed_stdout_pipe_does_not_silence_a_real_error() {
    let run = run_with_closed_stdout(&["--color", "never", "definitely-not-a-real-command-xyz123"]);

    assert!(
        run.stderr.contains("Error:"),
        "an unknown command must still report on stderr even when stdout is \
         closed, got stderr: {}",
        run.stderr
    );
    assert_ne!(
        run.status.code(),
        Some(0),
        "an unknown command must still exit non-zero when stdout is closed"
    );
}
