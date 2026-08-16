//! User-facing output that survives a reader closing the pipe early.
//!
//! `println!` panics when the write fails. Piping into a reader that quits
//! before the end - `| head`, `| less` closed early, a redirected
//! descriptor the reader dropped - closes the pipe, every later write
//! returns [`io::ErrorKind::BrokenPipe`], and the tool dies with exit 101
//! and a backtrace note over ordinary Unix usage (#115).
//!
//! The usual fix restores the default `SIGPIPE` disposition, which needs
//! `unsafe` and is therefore closed to this project (CLAUDE.md, "No
//! `unsafe`"). So the closed pipe is recognised here instead, at the one
//! boundary every user-facing line passes through, and [`outln!`] and
//! [`errln!`] replace `println!` and `eprintln!` throughout the CLI.
//! `just check-print-macros` fails the build if a bare `println!` returns.
//!
//! Neither stream ends the process, and for the same reason: once a pipe
//! is closed the exit code is the only signal a caller has left, so it has
//! to stay the command's own.
//!
//! - **stdout** carries the answer. The first `BrokenPipe` is recorded in
//!   a process-global flag and every later stdout line is dropped without
//!   a write attempt, so the command runs to its natural end and exits
//!   with the code it would have returned anyway. A reader that has seen
//!   enough is not an error, which is why `ls | head` does not report one,
//!   so a command that succeeded still exits 0 and the closed-pipe tests
//!   pin exactly that. Exiting here instead would report success for a
//!   command that went on to fail (the download path returns "N of M
//!   download(s) failed" long after its first line of output), and would
//!   skip every destructor still on the stack on the way out.
//! - **stderr** carries diagnostics. The line is dropped and the command
//!   finishes and reports as it would have.
//!
//! The cost of not exiting is that a long command keeps working after its
//! reader leaves: `data-gov list organizations | head -5` now fetches every
//! page rather than stopping at the fifth line. That is the price of an
//! honest exit code, and it is bounded by what the command was going to do
//! anyway.
//!
//! Any other write error keeps the old loud behaviour, so a genuinely
//! broken stdout is still not silent.

use std::fmt::Arguments;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set once stdout's reader has closed the pipe.
///
/// Process-global because the descriptor is: every `outln!` anywhere in
/// the process writes to the same stdout, so one closed pipe silences all
/// of them.
static STDOUT_READER_GONE: AtomicBool = AtomicBool::new(false);

/// What one write to a user-facing stream did, and what it means for the
/// process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteOutcome {
    /// The line reached the stream.
    Written,
    /// The reader closed its end of the pipe. Nothing further written to
    /// this stream can be seen by anyone.
    ReaderGone,
    /// The write failed for some other reason, which is a real fault and
    /// must stay loud.
    Failed(io::ErrorKind),
}

/// Write one line to `stream` and classify what happened.
///
/// Split out from the streams themselves so the classification can be
/// tested against a writer that fails on demand: the process-level tests
/// in `tests/cli_broken_pipe_tests.rs` prove the end-to-end behaviour, and
/// these prove that a closed pipe is told apart from a real fault rather
/// than everything being swallowed together.
pub(crate) fn write_line<W: Write>(stream: &mut W, args: Arguments<'_>) -> WriteOutcome {
    match writeln!(stream, "{args}") {
        Ok(()) => WriteOutcome::Written,
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => WriteOutcome::ReaderGone,
        Err(err) => WriteOutcome::Failed(err.kind()),
    }
}

/// Write one line to `stream` unless `reader_gone` already records that
/// nobody is listening, and set that flag if this write is the one that
/// finds the closed pipe.
///
/// The flag is a parameter rather than a read of [`STDOUT_READER_GONE`] so
/// the suppression can be proved against a local flag, without a test
/// silencing stdout for every other test sharing the process.
pub(crate) fn write_line_once<W: Write>(
    stream: &mut W,
    args: Arguments<'_>,
    reader_gone: &AtomicBool,
) -> WriteOutcome {
    // Relaxed is enough: the flag guards nothing but itself, and the worst
    // a stale read can cost is one more write that returns BrokenPipe
    // again and sets the flag a second time.
    if reader_gone.load(Ordering::Relaxed) {
        return WriteOutcome::ReaderGone;
    }

    let outcome = write_line(stream, args);
    if outcome == WriteOutcome::ReaderGone {
        reader_gone.store(true, Ordering::Relaxed);
    }
    outcome
}

/// Write one line to stdout, dropping it - and every line after it - once
/// the reader has closed the pipe.
///
/// The command is left to finish and exit with its own code; see the
/// module docs for why this must not exit the process.
///
/// # Panics
///
/// Panics if the write fails for any reason other than a closed pipe,
/// matching what `println!` did before.
pub(crate) fn stdout_line(args: Arguments<'_>) {
    // Checked before the lock so a command that keeps printing to a stdout
    // nobody reads does not queue on it just to discard each line.
    if STDOUT_READER_GONE.load(Ordering::Relaxed) {
        return;
    }

    let mut stdout = io::stdout().lock();
    match write_line_once(&mut stdout, args, &STDOUT_READER_GONE) {
        WriteOutcome::Written => {}
        // Deliberately not an exit: see the module docs. The line is
        // dropped, the flag now suppresses the rest, and the command keeps
        // its own exit code.
        WriteOutcome::ReaderGone => {}
        WriteOutcome::Failed(kind) => panic!("failed printing to stdout: {kind}"),
    }
}

/// Write one line to stderr, dropping it if nobody is reading it.
///
/// # Panics
///
/// Panics if the write fails for any reason other than a closed pipe,
/// matching what `eprintln!` did before.
pub(crate) fn stderr_line(args: Arguments<'_>) {
    let mut stderr = io::stderr().lock();
    match write_line(&mut stderr, args) {
        WriteOutcome::Written => {}
        // Deliberately not an exit: see the module docs. The command's own
        // exit code is the only signal a caller has left once stderr is
        // gone, and exiting 0 here would overwrite it with "success".
        WriteOutcome::ReaderGone => {}
        WriteOutcome::Failed(kind) => panic!("failed printing to stderr: {kind}"),
    }
}

/// Print a line to stdout, dropping it if the reader has closed the pipe.
/// Drop-in replacement for `println!`.
macro_rules! outln {
    () => {
        $crate::ui::output::stdout_line(::std::format_args!(""))
    };
    ($($arg:tt)*) => {
        $crate::ui::output::stdout_line(::std::format_args!($($arg)*))
    };
}

/// Print a line to stderr, dropping it if the reader has closed the pipe.
/// Drop-in replacement for `eprintln!`.
macro_rules! errln {
    () => {
        $crate::ui::output::stderr_line(::std::format_args!(""))
    };
    ($($arg:tt)*) => {
        $crate::ui::output::stderr_line(::std::format_args!($($arg)*))
    };
}

pub(crate) use {errln, outln};

#[cfg(test)]
mod tests {
    use super::*;

    /// A writer that fails every write with a chosen error kind.
    struct FailingWriter(io::ErrorKind);

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(self.0, "write refused by the test writer"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(self.0, "flush refused by the test writer"))
        }
    }

    #[test]
    fn a_write_that_succeeds_reports_written_and_appends_a_newline() {
        let mut sink = Vec::new();

        let outcome = write_line(&mut sink, format_args!("dataset {}", 7));

        assert_eq!(outcome, WriteOutcome::Written);
        assert_eq!(String::from_utf8(sink).expect("utf-8"), "dataset 7\n");
    }

    #[test]
    fn a_broken_pipe_reports_the_reader_is_gone() {
        let mut stream = FailingWriter(io::ErrorKind::BrokenPipe);

        let outcome = write_line(&mut stream, format_args!("anything"));

        assert_eq!(outcome, WriteOutcome::ReaderGone);
    }

    #[test]
    fn a_healthy_stream_writes_the_line_and_leaves_the_reader_flag_clear() {
        let reader_gone = AtomicBool::new(false);
        let mut sink = Vec::new();

        let outcome = write_line_once(&mut sink, format_args!("still listening"), &reader_gone);

        assert_eq!(outcome, WriteOutcome::Written);
        assert!(!reader_gone.load(Ordering::Relaxed));
        assert_eq!(String::from_utf8(sink).expect("utf-8"), "still listening\n");
    }

    /// The first closed pipe has to silence the rest of the run. Without
    /// that, every later line repeats a write that can only fail, and the
    /// flag standing in for the old exit would be doing nothing.
    #[test]
    fn a_closed_pipe_is_recorded_and_every_later_line_is_dropped_unwritten() {
        let reader_gone = AtomicBool::new(false);
        let mut closed = FailingWriter(io::ErrorKind::BrokenPipe);

        let first = write_line_once(&mut closed, format_args!("first"), &reader_gone);

        assert_eq!(first, WriteOutcome::ReaderGone);
        assert!(
            reader_gone.load(Ordering::Relaxed),
            "the closed pipe must be recorded, not just reported once"
        );

        // A writer that would happily accept the line: only the flag can
        // stop this one, so an empty sink proves the suppression.
        let mut sink = Vec::new();
        let second = write_line_once(&mut sink, format_args!("second"), &reader_gone);

        assert_eq!(second, WriteOutcome::ReaderGone);
        assert!(
            sink.is_empty(),
            "once the reader is gone no later line may be written, got {sink:?}"
        );
    }

    /// A full disk must stay loud. Recording it as a departed reader would
    /// silence stdout for the rest of the run and hide the fault.
    #[test]
    fn a_real_write_failure_does_not_record_a_departed_reader() {
        let reader_gone = AtomicBool::new(false);
        let mut stream = FailingWriter(io::ErrorKind::StorageFull);

        let outcome = write_line_once(&mut stream, format_args!("anything"), &reader_gone);

        assert_eq!(outcome, WriteOutcome::Failed(io::ErrorKind::StorageFull));
        assert!(
            !reader_gone.load(Ordering::Relaxed),
            "a fault is not a reader that left, and must not silence stdout"
        );
    }

    /// The quiet path must be specific to a closed pipe. Treating every
    /// write error as "the reader left" would turn a full disk or a
    /// revoked descriptor into a silent, successful exit.
    ///
    /// `ErrorKind::Interrupted` is deliberately absent: `Write::write_fmt`
    /// retries it rather than returning it, so it never reaches this
    /// classification. Including it here hung the test forever against a
    /// writer that always fails.
    #[test]
    fn any_other_write_error_reports_a_failure_rather_than_a_gone_reader() {
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::StorageFull,
            io::ErrorKind::WriteZero,
            io::ErrorKind::InvalidInput,
        ] {
            let mut stream = FailingWriter(kind);

            let outcome = write_line(&mut stream, format_args!("anything"));

            assert_eq!(
                outcome,
                WriteOutcome::Failed(kind),
                "{kind:?} is a real fault and must stay loud, not be mistaken \
                 for a reader that closed the pipe"
            );
        }
    }
}
