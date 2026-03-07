use rustyline::{DefaultEditor, Result as RustyResult};
use std::io;
use std::path::Path;
use std::str::FromStr;
use tokio::runtime::Runtime;

use super::commands::ReplCommand;
use super::display::print_repl_help;
use super::handlers::execute_command;
use super::{color_blue, color_blue_bold, color_dimmed, color_green_bold, color_red_bold};
use data_gov::DataGovClient;

/// REPL state and logic
pub struct DataGovRepl {
    client: DataGovClient,
    rt: Runtime,
}

impl DataGovRepl {
    pub fn new(client: DataGovClient) -> io::Result<Self> {
        let rt = Runtime::new()?;
        Ok(Self { client, rt })
    }

    pub fn run(&mut self) -> RustyResult<()> {
        println!("{}", color_blue_bold("🇺🇸 Data.gov Interactive Explorer"));
        println!(
            "{}",
            color_dimmed("Type 'help' for available commands, 'quit' to exit")
        );
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
                self.handle_setdir(path)?;
                return Ok(());
            }
            ReplCommand::Help => {
                print_repl_help();
                return Ok(());
            }
            _ => {}
        }

        // Use shared command execution logic for other commands
        execute_command(&self.client, &self.rt, command)?;
        Ok(())
    }

    fn handle_setdir(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        // Clone existing config and update only the download directory
        let new_config = self
            .client
            .config()
            .clone()
            .with_download_dir(path.to_path_buf());

        // Validate directory
        self.rt.block_on(async {
            let temp_client = DataGovClient::with_config(new_config.clone())?;
            temp_client.validate_download_dir().await?;
            self.client = temp_client;
            Ok::<(), data_gov::DataGovError>(())
        })?;

        println!(
            "{} Download directory set to: {}",
            color_green_bold("Success!"),
            color_blue(&path.display().to_string())
        );

        Ok(())
    }
}
