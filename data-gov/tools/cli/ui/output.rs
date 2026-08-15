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
//! The two streams are treated differently on purpose:
//!
//! - **stdout** carries the answer. Nobody is reading it any more, so the
//!   process stops, quietly and successfully. A reader that has seen
//!   enough is not an error, which is why `ls | head` does not report one.
//! - **stderr** carries diagnostics. A lost stderr must *not* end the
//!   process, because the exit code is the only signal a caller has left:
//!   exiting 0 here would report success for a command that failed. The
//!   line is dropped and the command finishes and reports as it would
//!   have.
//!
//! Any other write error keeps the old loud behaviour, so a genuinely
//! broken stdout is still not silent.

use std::fmt::Arguments;
use std::io::{self, Write};

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

/// Write one line to stdout, exiting quietly if nobody is reading it.
///
/// # Panics
///
/// Panics if the write fails for any reason other than a closed pipe,
/// matching what `println!` did before.
pub(crate) fn stdout_line(args: Arguments<'_>) {
    let mut stdout = io::stdout().lock();
    match write_line(&mut stdout, args) {
        WriteOutcome::Written => {}
        // Nobody is reading the answer, so there is no reason to keep
        // producing it. Exit 0: the command did its job and the reader
        // stopped listening, which is not a failure.
        WriteOutcome::ReaderGone => std::process::exit(0),
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

/// Print a line to stdout, exiting quietly if the reader has closed the
/// pipe. Drop-in replacement for `println!`.
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
