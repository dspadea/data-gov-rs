pub mod colors;

mod commands;
mod display;
mod handlers;
mod repl;
mod reporter;

use clap::{Arg, ArgMatches, Command};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use tokio::runtime::Runtime;

use self::colors::{ColorHelper, ColorMode};
use self::commands::{ReplCommand, SessionContext};
use self::handlers::execute_command;
use self::repl::DataGovRepl;
use self::reporter::CliStatusReporter;

use data_gov::{DataGovClient, DataGovConfig, OperatingMode};

/// Global color helper - will be set at startup
static COLOR_HELPER: OnceLock<ColorHelper> = OnceLock::new();

/// Helper functions for color formatting
pub fn color_red_bold(text: &str) -> String {
    COLOR_HELPER
        .get()
        .map(|h| h.style().red(text).bold().to_string())
        .unwrap_or_else(|| text.to_string())
}

pub fn color_green_bold(text: &str) -> String {
    COLOR_HELPER
        .get()
        .map(|h| h.style().green(text).bold().to_string())
        .unwrap_or_else(|| text.to_string())
}

pub fn color_blue_bold(text: &str) -> String {
    COLOR_HELPER
        .get()
        .map(|h| h.style().blue(text).bold().to_string())
        .unwrap_or_else(|| text.to_string())
}

pub fn color_yellow_bold(text: &str) -> String {
    COLOR_HELPER
        .get()
        .map(|h| h.style().yellow(text).bold().to_string())
        .unwrap_or_else(|| text.to_string())
}

pub fn color_cyan(text: &str) -> String {
    COLOR_HELPER
        .get()
        .map(|h| h.cyan(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

pub fn color_blue(text: &str) -> String {
    COLOR_HELPER
        .get()
        .map(|h| h.blue(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

pub fn color_green(text: &str) -> String {
    COLOR_HELPER
        .get()
        .map(|h| h.green(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

pub fn color_yellow(text: &str) -> String {
    COLOR_HELPER
        .get()
        .map(|h| h.yellow(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

pub fn color_red(text: &str) -> String {
    COLOR_HELPER
        .get()
        .map(|h| h.red(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

pub fn color_dimmed(text: &str) -> String {
    COLOR_HELPER
        .get()
        .map(|h| h.dimmed(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

pub fn color_bold(text: &str) -> String {
    COLOR_HELPER
        .get()
        .map(|h| h.bold(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

/// Red, gated on stderr's terminal state. Use for any text about to be
/// written with `eprintln!` — see [`colors::ColorHelper::red_err`].
pub fn color_red_err(text: &str) -> String {
    COLOR_HELPER
        .get()
        .map(|h| h.red_err(text))
        .unwrap_or_else(|| text.to_string())
}

/// Red and bold, gated on stderr's terminal state. Use for any text about
/// to be written with `eprintln!`.
pub fn color_red_bold_err(text: &str) -> String {
    COLOR_HELPER
        .get()
        .map(|h| h.style_err().red(text).bold().to_string())
        .unwrap_or_else(|| text.to_string())
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app = Command::new("data-gov")
        .about("Interactive REPL and CLI for exploring data.gov datasets")
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            Arg::new("download-dir")
                .long("download-dir")
                .short('d')
                .value_name("DIR")
                .help("Base directory for downloads (REPL: ~/Downloads/<dataset>/, CLI: ./<dataset>/)")
        )
        .arg(
            Arg::new("color")
                .long("color")
                .value_name("WHEN")
                .help("Control color output")
                .value_parser(["auto", "always", "never"])
                .default_value("auto")
        )
        .arg(
            Arg::new("command")
                .help("Command to execute (if provided, runs in CLI mode instead of interactive REPL)")
                .value_name("COMMAND")
                .index(1)
        )
        .arg(
            Arg::new("args")
                .help("Arguments for the command")
                .value_name("ARGS")
                .num_args(0..)
                .index(2)
        )
        .after_help(
            "EXAMPLES:\n\
             Interactive REPL mode:\n\
             \x20 data-gov\n\n\
             CLI mode:\n\
             \x20 data-gov search \"electric vehicle\" 10\n\
             \x20 data-gov show electric-vehicle-population-data\n\
             \x20 data-gov download electric-vehicle-population-data 0\n\
             \x20 data-gov download electric-vehicle-population-data \"Comma Separated Values File\"\n\
             \x20 data-gov cd /epa-gov/air-quality\n\
             \x20 data-gov list organizations\n\n\
             Available commands:\n\
             \x20 search <query> [limit]              Search for datasets\n\
             \x20 show [dataset_slug]                 Show dataset details\n\
             \x20 download [dataset] [selectors...]   Download distributions by index or title\n\
             \x20 cd <path>                           Navigate org/dataset (cd, select, sel)\n\
             \x20 list <organizations>                List organizations\n\
             \x20 info                                Show client info"
        );

    let matches = app.get_matches();

    // Build configuration
    let mut config = DataGovConfig::default();
    let mut color_mode = ColorMode::default();

    if let Some(download_dir) = matches.get_one::<String>("download-dir") {
        config = config.with_download_dir(PathBuf::from(download_dir));
    }

    // Mirrors `data-gov-mcp-server`'s own `DATA_GOV_BASE_URL` handling: lets
    // either front door point at a mirror, a proxy, or — for this crate's
    // own process-level tests — a mock server, without a CLI flag for
    // something nobody sets by hand day to day.
    if let Ok(base_url) = std::env::var("DATA_GOV_BASE_URL") {
        config = config.with_base_url(base_url);
    }

    // Parse color mode
    if let Some(color_str) = matches.get_one::<String>("color") {
        match color_str.parse::<ColorMode>() {
            Ok(mode) => color_mode = mode,
            Err(_) => eprintln!("Warning: Invalid color mode '{}', using 'auto'", color_str),
        }
    }

    // Force `colored`'s own global gate to agree with the resolved mode —
    // otherwise `--color always` piped to a file is a no-op (#58.1).
    color_mode.apply_as_global_override();

    // Create color helper based on configuration
    let color_helper = ColorHelper::new(color_mode);

    // Attach CLI status reporter
    let reporter = Arc::new(CliStatusReporter::new(color_helper.clone()));
    config = config.with_status_reporter(reporter);

    // Set global color helper
    COLOR_HELPER
        .set(color_helper.clone())
        .map_err(|_| "Failed to set color helper")?;

    // Check if we're in CLI mode or REPL mode and set the appropriate mode
    if let Some(command) = matches.get_one::<String>("command") {
        // CLI mode - execute single command and exit
        config = config.with_mode(OperatingMode::CommandLine);
        let client = DataGovClient::with_config(config)?;
        run_cli_mode(client, command, &matches)?;
    } else {
        // REPL mode - interactive session
        config = config.with_mode(OperatingMode::Interactive);
        let client = DataGovClient::with_config(config)?;
        let mut repl = DataGovRepl::new(client)?;
        repl.run()?;
    }

    Ok(())
}

/// Run a single command in CLI mode.
///
/// Resolution order for `command`, matching a kernel shebang launch
/// (`#!/usr/bin/env data-gov` passes the script's own path as this
/// positional):
///
/// 1. If the first token names a known command, dispatch it as that
///    command — even when the rest of the arguments fail to parse, in
///    which case *that command's own usage error* is reported. A known
///    command always wins, so `search` (or a bad `cd` with no path)
///    never becomes a filename, even if a file of that name sits in the
///    working directory (#64).
/// 2. Otherwise, if it names a readable existing file, run it as a script
///    (see [`run_script_file`]).
/// 3. Otherwise, the "Unknown command" error.
fn run_cli_mode(
    client: DataGovClient,
    command: &str,
    matches: &ArgMatches,
) -> Result<(), Box<dyn std::error::Error>> {
    let rt = Runtime::new()?;

    // Collect additional arguments
    let args: Vec<&String> = matches
        .get_many::<String>("args")
        .unwrap_or_default()
        .collect();

    // Build argument list for parsing without losing whitespace information
    let mut cmd_parts: Vec<String> = Vec::with_capacity(1 + args.len());
    cmd_parts.push(command.to_string());
    cmd_parts.extend(args.iter().map(|s| (*s).clone()));

    match ReplCommand::from_parts(&cmd_parts) {
        Ok(repl_command) => {
            let mut ctx = SessionContext::default();
            if let Err(e) = execute_command(&client, &rt, repl_command, &mut ctx) {
                eprintln!("{} {}", color_red_bold_err("Error:"), e);
                std::process::exit(1);
            }
        }
        Err(parse_err) => {
            // Only an *unrecognized* token can fall back to the script
            // path. A known command whose arguments failed to parse
            // (e.g. `cd` with no path) must report its own usage error —
            // it must never be shadowed by a same-named file, or a
            // directory of scripts named after commands (#64) turns a
            // typo'd invocation into a silent, wrong script run.
            let script_path = Path::new(command);
            let run_as_script =
                ReplCommand::is_unrecognized_command_error(&parse_err) && script_path.is_file();

            if run_as_script {
                if let Err(e) = run_script_file(&client, &rt, script_path) {
                    eprintln!("{} {}", color_red_bold_err("Error:"), e);
                    std::process::exit(1);
                }
            } else {
                eprintln!("{} {}", color_red_bold_err("Error:"), parse_err);
                eprintln!("Use --help to see available commands and examples");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

/// Run a script file as a sequence of REPL commands, one per line.
///
/// This is what makes a `#!/usr/bin/env data-gov` shebang launch work: the
/// kernel execs `data-gov <script-path>`, clap binds the path to the
/// `command` positional, `ReplCommand::from_parts` fails to parse it as a
/// known command, and [`run_cli_mode`] falls back to here. Blank lines and
/// comments (lines starting with `#`, which includes the shebang line
/// itself) are skipped, the same as the interactive REPL's own loop.
///
/// A failing line does not stop the rest of the script — each error is
/// reported immediately, against its line number, the way a shell
/// continues past a failing command by default — but the function still
/// returns `Err` if any line failed, so the process exits non-zero. A
/// script that partially ran must never be reported as if it fully
/// succeeded.
///
/// `lcd` is not supported in script mode (same restriction as one-shot CLI
/// mode): swapping the download directory mid-script would require
/// rebuilding the client, which would need `client` here to be owned and
/// mutable rather than shared with the rest of `execute_command`'s
/// call sites. None of the shipped example scripts use `lcd`; a script
/// that needs a specific download directory should pass `--download-dir`
/// on the invocation instead.
fn run_script_file(
    client: &DataGovClient,
    rt: &Runtime,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read script '{}': {e}", path.display()))?;

    let mut ctx = SessionContext::default();
    let mut had_error = false;

    for (line_no, line) in contents.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let outcome = match ReplCommand::from_str(trimmed) {
            Ok(ReplCommand::Quit) => break,
            Ok(command) => execute_command(client, rt, command, &mut ctx),
            Err(e) => Err(e.into()),
        };

        if let Err(e) = outcome {
            eprintln!(
                "{} {}:{}: {}",
                color_red_bold_err("Error:"),
                path.display(),
                line_no + 1,
                e
            );
            had_error = true;
        }
    }

    if had_error {
        return Err(format!(
            "script '{}' had one or more failing commands",
            path.display()
        )
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_client() -> DataGovClient {
        DataGovClient::with_config(DataGovConfig::default()).expect("test client must build")
    }

    #[test]
    fn run_script_file_executes_known_commands_and_skips_comments_and_blanks() {
        // The exact shape a `#!/usr/bin/env data-gov` shebang script takes:
        // the shebang line itself is a comment (starts with '#'), so it is
        // skipped the same way any other comment is.
        let rt = Runtime::new().expect("runtime");
        let client = test_client();

        let mut file = tempfile::NamedTempFile::new().expect("temp script file");
        writeln!(file, "#!/usr/bin/env data-gov").unwrap();
        writeln!(file, "# a comment").unwrap();
        writeln!(file).unwrap();
        writeln!(file, "info").unwrap();
        writeln!(file, "quit").unwrap();
        file.flush().unwrap();

        let result = run_script_file(&client, &rt, file.path());
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn run_script_file_stops_at_quit_without_running_later_lines() {
        let rt = Runtime::new().expect("runtime");
        let client = test_client();

        let mut file = tempfile::NamedTempFile::new().expect("temp script file");
        writeln!(file, "info").unwrap();
        writeln!(file, "quit").unwrap();
        // A line that would fail to parse — proves it was never reached.
        writeln!(file, "this is not a real command").unwrap();
        file.flush().unwrap();

        let result = run_script_file(&client, &rt, file.path());
        assert!(
            result.is_ok(),
            "quit must stop the script before the bad line below it: {result:?}"
        );
    }

    #[test]
    fn run_script_file_returns_err_when_a_line_fails_to_parse() {
        let rt = Runtime::new().expect("runtime");
        let client = test_client();

        let mut file = tempfile::NamedTempFile::new().expect("temp script file");
        writeln!(file, "this-is-not-a-command").unwrap();
        file.flush().unwrap();

        let result = run_script_file(&client, &rt, file.path());
        assert!(
            result.is_err(),
            "a failing line must fail the whole script, not be silently skipped"
        );
    }

    #[test]
    fn run_script_file_returns_err_when_a_command_execution_fails() {
        let rt = Runtime::new().expect("runtime");
        let client = test_client();

        let mut file = tempfile::NamedTempFile::new().expect("temp script file");
        // Parses fine as a command, but fails at execution time (unknown
        // list subject) — a different failure mode from a parse error.
        writeln!(file, "list bogus-subject").unwrap();
        file.flush().unwrap();

        let result = run_script_file(&client, &rt, file.path());
        assert!(result.is_err());
    }

    #[test]
    fn run_script_file_errors_clearly_when_the_path_is_unreadable() {
        let rt = Runtime::new().expect("runtime");
        let client = test_client();

        let missing = std::path::Path::new("/nonexistent/path/does-not-exist.sh");
        let result = run_script_file(&client, &rt, missing);
        assert!(result.is_err());
    }
}
