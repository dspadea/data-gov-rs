use std::io;
use std::path::PathBuf;
use std::str::FromStr;

use clap::{Arg, Command};
use rustyline::{DefaultEditor, Result as RustyResult};
use tokio::runtime::Runtime;

use data_gov::{DataGovClient, DataGovConfig, OperatingMode, ColorHelper, ColorMode, ckan::models::Package};

/// Global color helper - will be set at startup
use std::sync::OnceLock;
static COLOR_HELPER: OnceLock<ColorHelper> = OnceLock::new();

/// Helper functions for color formatting
fn color_red_bold(text: &str) -> String {
    COLOR_HELPER.get().map(|h| h.style().red(text).bold().to_string())
        .unwrap_or_else(|| text.to_string())
}

fn color_green_bold(text: &str) -> String {
    COLOR_HELPER.get().map(|h| h.style().green(text).bold().to_string())
        .unwrap_or_else(|| text.to_string())
}

fn color_blue_bold(text: &str) -> String {
    COLOR_HELPER.get().map(|h| h.style().blue(text).bold().to_string())
        .unwrap_or_else(|| text.to_string())
}

fn color_yellow_bold(text: &str) -> String {
    COLOR_HELPER.get().map(|h| h.style().yellow(text).bold().to_string())
        .unwrap_or_else(|| text.to_string())
}

fn color_cyan(text: &str) -> String {
    COLOR_HELPER.get().map(|h| h.cyan(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

fn color_blue(text: &str) -> String {
    COLOR_HELPER.get().map(|h| h.blue(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

fn color_green(text: &str) -> String {
    COLOR_HELPER.get().map(|h| h.green(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

fn color_yellow(text: &str) -> String {
    COLOR_HELPER.get().map(|h| h.yellow(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

fn color_red(text: &str) -> String {
    COLOR_HELPER.get().map(|h| h.red(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

fn color_dimmed(text: &str) -> String {
    COLOR_HELPER.get().map(|h| h.dimmed(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

fn color_bold(text: &str) -> String {
    COLOR_HELPER.get().map(|h| h.bold(text).to_string())
        .unwrap_or_else(|| text.to_string())
}

/// REPL Commands
#[derive(Debug, Clone)]
enum ReplCommand {
    Search { query: String, limit: Option<i32> },
    Show { dataset_id: String },
    Download { dataset_id: String, resource_index: Option<usize> },
    List { what: String },  // organizations, formats, etc.
    SetDir { path: PathBuf },
    Info,
    Help,
    Quit,
}

impl FromStr for ReplCommand {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.trim().split_whitespace().collect();
        
        if parts.is_empty() {
            return Err("Empty command".to_string());
        }
        
        match parts[0].to_lowercase().as_str() {
            "search" | "s" => {
                if parts.len() < 2 {
                    return Err("Usage: search <query> [limit]".to_string());
                }
                let query = parts[1..].join(" ");
                let limit = if parts.len() > 2 {
                    parts.last().unwrap().parse().ok()
                } else {
                    None
                };
                Ok(ReplCommand::Search { query, limit })
            }
            "show" | "describe" | "d" => {
                if parts.len() != 2 {
                    return Err("Usage: show <dataset_id>".to_string());
                }
                Ok(ReplCommand::Show {
                    dataset_id: parts[1].to_string(),
                })
            }
            "download" | "dl" => {
                if parts.len() < 2 || parts.len() > 3 {
                    return Err("Usage: download <dataset_id> [resource_index]".to_string());
                }
                let resource_index = if parts.len() == 3 {
                    parts[2].parse().ok()
                } else {
                    None
                };
                Ok(ReplCommand::Download {
                    dataset_id: parts[1].to_string(),
                    resource_index,
                })
            }
            "list" | "ls" => {
                if parts.len() != 2 {
                    return Err("Usage: list <organizations|orgs>".to_string());
                }
                Ok(ReplCommand::List {
                    what: parts[1].to_string(),
                })
            }
            "setdir" | "cd" => {
                if parts.len() != 2 {
                    return Err("Usage: setdir <path>".to_string());
                }
                Ok(ReplCommand::SetDir {
                    path: PathBuf::from(parts[1]),
                })
            }
            "info" | "status" => Ok(ReplCommand::Info),
            "help" | "h" | "?" => Ok(ReplCommand::Help),
            "quit" | "exit" | "q" => Ok(ReplCommand::Quit),
            _ => Err(format!("Unknown command: {}", parts[0])),
        }
    }
}

/// REPL state and logic
struct DataGovRepl {
    client: DataGovClient,
    rt: Runtime,
    color_helper: ColorHelper,
}

impl DataGovRepl {
    fn new(client: DataGovClient, color_helper: ColorHelper) -> io::Result<Self> {
        let rt = Runtime::new()?;
        Ok(Self { client, rt, color_helper })
    }
    
    fn run(&mut self) -> RustyResult<()> {
        println!("{}", color_blue_bold("🇺🇸 Data.gov Interactive Explorer"));
        println!("{}", color_dimmed("Type 'help' for available commands, 'quit' to exit"));
        println!();
        
        let mut rl = DefaultEditor::new()?;
        
        loop {
            let readline = rl.readline(&format!("{} ", color_green_bold("data.gov>")));
            
            match readline {
                Ok(line) => {
                    let trimmed = line.trim();
                    
                    // Skip empty lines and comments
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    
                    rl.add_history_entry(line.as_str())?;
                    
                    match ReplCommand::from_str(&line) {
                        Ok(command) => {
                            if let ReplCommand::Quit = command {
                                println!("Goodbye! 👋");
                                break;
                            }
                            
                            if let Err(e) = self.handle_command(command) {
                                println!("{} {}", color_red_bold("Error:"), e);
                            }
                        }
                        Err(e) => {
                            println!("{} {}", color_red_bold("Invalid command:"), e);
                        }
                    }
                }
                Err(rustyline::error::ReadlineError::Interrupted) => {
                    println!("CTRL-C");
                    break;
                }
                Err(rustyline::error::ReadlineError::Eof) => {
                    println!("CTRL-D");
                    break;
                }
                Err(err) => {
                    println!("Error: {:?}", err);
                    break;
                }
            }
        }
        
        Ok(())
    }
    
    fn handle_command(&mut self, command: ReplCommand) -> Result<(), Box<dyn std::error::Error>> {
        // Handle REPL-specific commands
        match &command {
            ReplCommand::SetDir { path } => {
                // Create new config with updated directory
                let new_config = DataGovConfig::new().with_download_dir(path.clone());
                
                // Validate directory
                self.rt.block_on(async {
                    let temp_client = DataGovClient::with_config(new_config.clone())?;
                    temp_client.validate_download_dir().await?;
                    self.client = temp_client;
                    Ok::<(), data_gov::DataGovError>(())
                })?;
                
                println!("{} Download directory set to: {}", 
                    color_green_bold("Success!"), 
                    color_blue(&path.display().to_string())
                );
                return Ok(());
            }
            _ => {}
        }
        
        // Handle Help command specially for REPL mode
        match &command {
            ReplCommand::Help => {
                self.print_repl_help();
                return Ok(());
            }
            _ => {}
        }
        
        // Use shared command execution logic for other commands
        execute_command(&self.client, &self.rt, command)?;
        Ok(())
    }
    
    fn print_repl_help(&self) {
        println!("\n{}", color_blue_bold("📚 Available Commands"));
        println!();
        
        let commands = vec![
            ("search <query> [limit]", "Search for datasets", "search climate data 20"),
            ("show <dataset_id>", "Show detailed dataset information", "show consumer-complaint-database"),
            ("download <dataset_id> [index]", "Download dataset resources", "download my-dataset 0"),
            ("list organizations", "List government organizations", "list orgs"),
            ("setdir <path>", "Set base download directory", "setdir ./downloads"),
            ("info", "Show session information", "info"),
            ("help", "Show this help message", "help"),
            ("quit", "Exit the REPL", "quit"),
        ];
        
        for (cmd, desc, example) in commands {
            println!("{:25} {}", color_green_bold(cmd), desc);
            println!("{:25} {}: {}", "", color_dimmed("Example"), color_blue(example));
            println!();
        }
        
        println!("{}", color_yellow_bold("💡 Pro Tips:"));
        println!("  • Use short commands: {} for search, {} for show, {} for download", 
            color_green("s"), color_green("d"), color_green("dl"));
        println!("  • Search supports multiple words: {}", color_blue("search energy solar wind"));
        println!("  • Downloads are organized by dataset name in subdirectories");
        println!("  • Download without index downloads all resources");
        println!("  • Create scripts with {} for automation", color_blue("#!/usr/bin/env data-gov"));
        println!("  • Run {} to use CLI mode", color_blue("data-gov search \"climate\" 10"));
        println!();
    }
    

}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
             \x20 data-gov search \"climate data\" 10\n\
             \x20 data-gov show consumer-complaint-database\n\
             \x20 data-gov download consumer-complaint-database 0\n\
             \x20 data-gov list organizations\n\n\
             Available commands:\n\
             \x20 search <query> [limit]     Search for datasets\n\
             \x20 show <dataset_id>         Show dataset details\n\
             \x20 download <dataset_id> [index]  Download resources\n\
             \x20 list <organizations>      List organizations\n\
             \x20 info                      Show client info"
        );
    
    let matches = app.get_matches();
    
    // Build configuration
    let mut config = DataGovConfig::default();
    
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
        if let Ok(color_mode) = color_str.parse::<ColorMode>() {
            config = config.with_color_mode(color_mode);
        } else {
            eprintln!("Warning: Invalid color mode '{}', using 'auto'", color_str);
        }
    }
    
    // Create color helper based on configuration
    let color_helper = ColorHelper::new(config.color_mode);
    
    // Set global color helper
    COLOR_HELPER.set(color_helper.clone()).map_err(|_| "Failed to set color helper")?;
    
    // Check if we're in CLI mode or REPL mode and set the appropriate mode
    if let Some(command) = matches.get_one::<String>("command") {
        // CLI mode - execute single command and exit
        config = config.with_mode(OperatingMode::CommandLine);
        let client = DataGovClient::with_config(config)?;
        run_cli_mode(client, command, &matches, &color_helper)?;
    } else {
        // REPL mode - interactive session
        config = config.with_mode(OperatingMode::Interactive);
        let client = DataGovClient::with_config(config)?;
        let mut repl = DataGovRepl::new(client, color_helper)?;
        repl.run()?;
    }
    
    Ok(())
}

/// Run a single command in CLI mode
fn run_cli_mode(
    client: DataGovClient,
    command: &str,
    matches: &clap::ArgMatches,
    color_helper: &ColorHelper,
) -> Result<(), Box<dyn std::error::Error>> {
    let rt = Runtime::new()?;
    
    // Collect additional arguments
    let args: Vec<&String> = matches.get_many::<String>("args").unwrap_or_default().collect();
    
    // Build command string
    let mut cmd_parts = vec![command];
    cmd_parts.extend(args.iter().map(|s| s.as_str()));
    let full_command = cmd_parts.join(" ");
    
    // Parse the command
    let repl_command = match ReplCommand::from_str(&full_command) {
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
        Ok(()) => {},
        Err(e) => {
            eprintln!("{} {}", color_red_bold("Error:"), e);
            std::process::exit(1);
        }
    }
    
    Ok(())
}

/// Execute a command (shared between REPL and CLI modes)
fn execute_command(
    client: &DataGovClient,
    rt: &Runtime,
    command: ReplCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        ReplCommand::Search { query, limit } => {
            println!("{} '{}'...", color_cyan("Searching for"), query);
            
            let results = rt.block_on(
                client.search(&query, limit, None, None, None)
            )?;
            
            println!("\n{} {} results:\n", color_green_bold("Found"), results.count.unwrap_or(0));
            
            if let Some(packages) = results.results {
                for (i, package) in packages.iter().enumerate().take(20) {
                    println!("{}. {} {}", 
                        color_blue_bold(&format!("{:2}", i + 1)),
                        color_yellow_bold(&package.name),
                        color_dimmed(package.title.as_deref().unwrap_or(""))
                    );
                    
                    if let Some(notes) = &package.notes {
                        let truncated = if notes.len() > 100 {
                            format!("{}...", &notes[..100])
                        } else {
                            notes.clone()
                        };
                        println!("   {}", color_dimmed(&truncated));
                    }
                    println!();
                }
                
                if packages.len() > 20 {
                    println!("... and {} more results", packages.len() - 20);
                }
            }
        }
        
        ReplCommand::Show { dataset_id } => {
            println!("{} dataset '{}'...", color_cyan("Fetching"), dataset_id);
            
            let package = rt.block_on(client.get_dataset(&dataset_id))?;
            print_package_details(&package);
        }
        
        ReplCommand::Download { dataset_id, resource_index } => {
            println!("{} dataset '{}'...", color_cyan("Fetching"), dataset_id);
            
            let package = rt.block_on(client.get_dataset(&dataset_id))?;
            let resources = DataGovClient::get_downloadable_resources(&package);
            
            if resources.is_empty() {
                println!("{} No downloadable resources found in this dataset.", color_yellow_bold("Warning:"));
                return Ok(());
            }
            
            match resource_index {
                Some(index) => {
                    if index >= resources.len() {
                        println!("{} Resource index {} is out of range (0-{})", 
                            color_red_bold("Error:"), index, resources.len() - 1);
                        return Ok(());
                    }
                    
                    let resource = &resources[index];
                    println!("{} resource {}...", color_cyan("Downloading"), index);
                    
                    let path = rt.block_on(
                        client.download_dataset_resource(resource, &dataset_id)
                    )?;
                    
                    println!("{} Downloaded to: {}", 
                        color_green_bold("Success!"), 
                        color_blue(&path.display().to_string())
                    );
                }
                None => {
                    // Download all resources
                    println!("{} {} resources...", color_cyan("Downloading"), resources.len());
                    
                    let results = rt.block_on(
                        client.download_dataset_resources(&resources, &dataset_id)
                    );
                    
                    let mut success_count = 0;
                    let mut error_count = 0;
                    
                    for (i, result) in results.iter().enumerate() {
                        match result {
                            Ok(path) => {
                                success_count += 1;
                                println!("  {} Resource {}: {}", 
                                    color_green("✓"), i, color_blue(&path.display().to_string()));
                            }
                            Err(e) => {
                                error_count += 1;
                                println!("  {} Resource {}: {}", 
                                    color_red("✗"), i, color_red(&e.to_string()));
                            }
                        }
                    }
                    
                    println!("\n{} {} downloaded, {} errors", 
                        color_bold("Summary:"),
                        color_green(&success_count.to_string()),
                        color_red(&error_count.to_string())
                    );
                }
            }
        }
        
        ReplCommand::List { what } => {
            match what.to_lowercase().as_str() {
                "organizations" | "orgs" => {
                    println!("{} organizations...", color_cyan("Fetching"));
                    
                    let orgs = rt.block_on(
                        client.list_organizations(Some(50))
                    )?;
                    
                    println!("\n{} organizations:", color_green_bold("Government"));
                    for (i, org) in orgs.iter().enumerate() {
                        println!("{}. {}", 
                            color_blue_bold(&format!("{:2}", i + 1)),
                            color_yellow(org)
                        );
                    }
                }
                _ => {
                    println!("{} Unknown list type: {}", color_red_bold("Error:"), what);
                    println!("Available: {}", color_blue("organizations"));
                }
            }
        }
        
        ReplCommand::Info => {
            println!("\n{}", color_blue_bold("📊 Client Information"));
            println!("Download directory: {}", 
                color_blue(&client.download_dir().display().to_string()));
            println!("CKAN endpoint: {}", color_blue("https://catalog.data.gov/api/3"));
        }
        
        ReplCommand::SetDir { .. } => {
            println!("{} SetDir command is only available in interactive REPL mode", color_red_bold("Error:"));
        }
        
        ReplCommand::Help => {
            print_cli_help();
        }
        
        ReplCommand::Quit => {
            // Not applicable in CLI mode
        }
    }
    
    Ok(())
}

/// Print help for CLI mode
fn print_cli_help() {
    println!("\n{}", color_blue_bold("📚 CLI Mode Commands"));
    println!();
    
    let commands = vec![
        ("search <query> [limit]", "Search for datasets", "search \"climate data\" 20"),
        ("show <dataset_id>", "Show detailed dataset information", "show consumer-complaint-database"),
        ("download <dataset_id> [index]", "Download dataset resources", "download my-dataset 0"),
        ("list organizations", "List government organizations", "list organizations"),
        ("info", "Show client information", "info"),
    ];
    
    for (cmd, desc, example) in commands {
        println!("{:30} {}", color_green_bold(cmd), desc);
        println!("{:30} {}: data-gov {}", "", color_dimmed("Example"), color_blue(example));
        println!();
    }
    
    println!("{}", color_yellow_bold("💡 Interactive Mode:"));
    println!("  Run without arguments to start interactive REPL: {}", color_blue("data-gov"));
    println!();
}

/// Print package details (shared between REPL and CLI modes)  
fn print_package_details(package: &Package) {
    println!("\n{}", color_blue_bold("📦 Dataset Details"));
    println!("{}: {}", color_bold("Name"), color_yellow(&package.name));
    
    if let Some(title) = &package.title {
        println!("{}: {}", color_bold("Title"), title);
    }
    
    if let Some(notes) = &package.notes {
        println!("\n{}: ", color_bold("Description"));
        println!("{}", color_dimmed(notes));
    }
    
    if let Some(license_title) = &package.license_title {
        println!("\n{}: {}", color_bold("License"), color_green(license_title));
    }
    
    if let Some(author) = &package.author {
        println!("{}: {}", color_bold("Author"), author);
    }
    
    if let Some(maintainer) = &package.maintainer {
        println!("{}: {}", color_bold("Maintainer"), maintainer);
    }
    
    // Display resources
    let resources = DataGovClient::get_downloadable_resources(package);
    if !resources.is_empty() {
        println!("\n{} {} downloadable resources:", color_bold("📁"), resources.len());
        
        for (i, resource) in resources.iter().enumerate() {
            let name = resource.name.as_deref().unwrap_or("Unnamed");
            let format = resource.format.as_deref().unwrap_or("Unknown");
            let size = resource.size.map(|s| format!(" ({})", s)).unwrap_or_default();
            
            println!("  {}. {} {} {}{}", 
                color_blue_bold(&i.to_string()),
                color_yellow(name),
                color_green(&format!("[{}]", format)),
                color_dimmed(&size),
                ""
            );
            
            if let Some(desc) = &resource.description {
                if !desc.is_empty() {
                    let truncated = if desc.len() > 80 {
                        format!("{}...", &desc[..80])
                    } else {
                        desc.clone()
                    };
                    println!("     {}", color_dimmed(&truncated));
                }
            }
        }
        
        println!("\n{} Use 'data-gov download {}' to download all resources", 
            color_bold("💡"), color_yellow(&package.name));
        println!("{} Use 'data-gov download {} <index>' to download a specific resource", 
            color_bold("💡"), color_yellow(&package.name));
    } else {
        println!("\n{} No downloadable resources found", color_yellow("⚠️"));
    }
    
    println!();
}