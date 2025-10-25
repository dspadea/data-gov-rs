pub mod colors;

mod commands;
mod display;
mod handlers;
mod repl;
mod reporter;

use clap::{Arg, ArgMatches, Command};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::runtime::Runtime;

use self::colors::{ColorHelper, ColorMode};
use self::commands::ReplCommand;
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

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app = Command::new("data-gov")
        .about("Interactive REPL and CLI for exploring data.gov datasets")
        .version("1.0")
        .arg(
            Arg::new("api-key")
                .long("api-key")
                .value_name("KEY")
                .help("CKAN API key for higher rate limits")
        )
        .arg(
            Arg::new("download-dir")
                .long("download-dir")
                .short('d')
                .value_name("DIR")
                .help("Base directory for downloads (REPL: ~/Downloads/<dataset>/, CLI: ./<dataset>/)")
                .default_value("./downloads")
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
             \x20 data-gov list organizations\n\n\
             Available commands:\n\
             \x20 search <query> [limit]     Search for datasets\n\
             \x20 show <dataset_id>         Show dataset details\n\
             \x20 download <dataset_id> [index|name]  Download by index or name\n\
             \x20 list <organizations>      List organizations\n\
             \x20 info                      Show client info"
        );

    let matches = app.get_matches();

    // Build configuration
    let mut config = DataGovConfig::default();
    let mut color_mode = ColorMode::default();

    if let Some(api_key) = matches.get_one::<String>("api-key") {
        config = config.with_api_key(api_key);
    }

    if let Some(download_dir) = matches.get_one::<String>("download-dir") {
        // Only override the default if explicitly provided and not the CLI default value
        if download_dir != "./downloads" {
            config = config.with_download_dir(PathBuf::from(download_dir));
        }
        // If it's the CLI default, keep using the system default from DataGovConfig::default()
    }

    // Parse color mode
    if let Some(color_str) = matches.get_one::<String>("color") {
        match color_str.parse::<ColorMode>() {
            Ok(mode) => color_mode = mode,
            Err(_) => eprintln!("Warning: Invalid color mode '{}', using 'auto'", color_str),
        }
    }

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

/// Run a single command in CLI mode
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

    // Parse the command
    let repl_command = match ReplCommand::from_parts(&cmd_parts) {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("{} {}", color_red_bold("Error:"), e);
            eprintln!("Use --help to see available commands and examples");
            std::process::exit(1);
        }
    };

    // Execute the command using the same logic as the REPL
    let result = execute_command(&client, &rt, repl_command);

    match result {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{} {}", color_red_bold("Error:"), e);
            std::process::exit(1);
        }
    }

    Ok(())
}
