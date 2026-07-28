use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, Result as RustyResult};
use std::io;
use std::path::Path;
use std::str::FromStr;
use tokio::runtime::Runtime;

use super::commands::{ReplCommand, SessionContext};
use super::display::print_repl_help;
use super::handlers::execute_command;
use super::{color_blue, color_blue_bold, color_dimmed, color_green_bold, color_red_bold};
use data_gov::DataGovClient;

/// What the read-eval loop does in response to one `readline()` result.
///
/// Kept as a plain enum, decided by a pure function, so the branching that
/// distinguishes Ctrl-C from Ctrl-D can be unit tested without a real
/// terminal — `ReadlineError::Interrupted` and `::Eof` are constructible
/// directly, no pty required.
enum LoopAction {
    /// A line was read; process it as a command.
    Process(String),
    /// Ctrl-C: discard the half-typed line and re-prompt. Must never exit
    /// the REPL — rustyline maps Ctrl-C to `Cmd::Interrupt` specifically so
    /// the host can cancel the current line, the way bash, python, node,
    /// and psql all do.
    Reprompt,
    /// Ctrl-D (clean EOF) or an unrecoverable I/O error: exit the loop.
    /// Carries the message to print first.
    Exit(String),
}

/// Decide the loop action for a `readline()` result.
fn loop_action(result: RustyResult<String>) -> LoopAction {
    match result {
        Ok(line) => LoopAction::Process(line),
        Err(ReadlineError::Interrupted) => LoopAction::Reprompt,
        Err(ReadlineError::Eof) => LoopAction::Exit("CTRL-D".to_string()),
        Err(err) => LoopAction::Exit(format!("Error: {err:?}")),
    }
}

/// Whether `run()`'s loop should read another line or stop.
///
/// A plain enum rather than a bare `bool`, so the call site in `run()`
/// reads as "keep looping" / "stop looping" instead of an unlabeled
/// `true`/`false` a reviewer has to trace back to its meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopControl {
    Continue,
    Stop,
}

/// REPL state and logic
pub struct DataGovRepl {
    client: DataGovClient,
    rt: Runtime,
    ctx: SessionContext,
}

impl DataGovRepl {
    pub fn new(client: DataGovClient) -> io::Result<Self> {
        let rt = Runtime::new()?;
        Ok(Self {
            client,
            rt,
            ctx: SessionContext::default(),
        })
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
            let prompt = self.build_prompt();
            let readline = rl.readline(&prompt);

            if self.process_readline_result(readline, &mut rl)? == LoopControl::Stop {
                break;
            }
        }

        Ok(())
    }

    /// Handle one `readline()` outcome: decide the action via
    /// [`loop_action`], perform its side effects (recording history,
    /// dispatching the command, printing errors), and report whether
    /// `run()`'s loop should read another line.
    ///
    /// Split out from `run()` so the *wiring* around `loop_action` — not
    /// just its pure decision — can be driven directly by a test without a
    /// real terminal: `rustyline` needs an actual (or pty) terminal to
    /// ever produce `ReadlineError::Interrupted` for real, so nothing
    /// short of this seam can exercise "does the REPL actually keep going
    /// after Ctrl-C" outside of one. A test supplies a canned
    /// `RustyResult<String>` directly and asserts on the returned
    /// `LoopControl` — exactly the wiring that regressed when `Reprompt`'s
    /// `continue` became a `break` (#69.4).
    fn process_readline_result(
        &mut self,
        result: RustyResult<String>,
        rl: &mut DefaultEditor,
    ) -> RustyResult<LoopControl> {
        match loop_action(result) {
            LoopAction::Process(line) => {
                let trimmed = line.trim();

                // Skip empty lines and comments
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    return Ok(LoopControl::Continue);
                }

                rl.add_history_entry(line.as_str())?;

                match ReplCommand::from_str(&line) {
                    Ok(command) => {
                        if let ReplCommand::Quit = command {
                            println!("Goodbye! 👋");
                            return Ok(LoopControl::Stop);
                        }

                        if let Err(e) = self.handle_command(command) {
                            println!("{} {}", color_red_bold("Error:"), e);
                        }
                    }
                    Err(e) => {
                        println!("{} {}", color_red_bold("Invalid command:"), e);
                    }
                }

                Ok(LoopControl::Continue)
            }
            LoopAction::Reprompt => {
                println!("CTRL-C");
                // Must keep looping — this is the exact wiring decision
                // that regressed to `break` under #69.4.
                Ok(LoopControl::Continue)
            }
            LoopAction::Exit(msg) => {
                println!("{msg}");
                Ok(LoopControl::Stop)
            }
        }
    }

    fn build_prompt(&self) -> String {
        let label = self.ctx.prompt_label();
        if label.is_empty() {
            format!("{} ", color_green_bold("data.gov>"))
        } else {
            // Show context on a line above the prompt to conserve horizontal space
            format!(
                "{}\n{} ",
                color_dimmed(&label),
                color_green_bold("data.gov>")
            )
        }
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
        execute_command(&self.client, &self.rt, command, &mut self.ctx)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_c_reprompts_instead_of_exiting() {
        // rustyline maps Ctrl-C to ReadlineError::Interrupted specifically so
        // the host can discard the half-typed line and re-prompt, the way
        // bash, python, node, and psql all do. It must never end the
        // session the way Ctrl-D does.
        let action = loop_action(Err(ReadlineError::Interrupted));
        assert!(
            matches!(action, LoopAction::Reprompt),
            "Ctrl-C must reprompt, not exit the REPL"
        );
    }

    #[test]
    fn ctrl_d_exits() {
        let action = loop_action(Err(ReadlineError::Eof));
        assert!(matches!(action, LoopAction::Exit(_)));
    }

    #[test]
    fn a_line_is_read_and_processed() {
        let action = loop_action(Ok("search foo".to_string()));
        match action {
            LoopAction::Process(line) => assert_eq!(line, "search foo"),
            _ => panic!("expected LoopAction::Process"),
        }
    }

    // --- process_readline_result: the real `run()` wiring, not just
    // loop_action's pure decision (#69.4) ---

    fn test_repl() -> DataGovRepl {
        let client = DataGovClient::with_config(data_gov::DataGovConfig::default())
            .expect("test client must build");
        DataGovRepl::new(client).expect("test repl must build")
    }

    #[test]
    fn ctrl_c_wiring_keeps_the_loop_running() {
        // This is the wiring `ctrl_c_reprompts_instead_of_exiting` (above)
        // cannot reach: that test only proves `loop_action` classifies
        // Ctrl-C as `Reprompt`. It says nothing about what `run()`'s loop
        // *does* with a `Reprompt` — and swapping that arm's `continue`
        // for a `break` passed all 90 tests before this one existed.
        let mut repl = test_repl();
        let mut rl = DefaultEditor::new().expect("test editor must build");

        let control = repl
            .process_readline_result(Err(ReadlineError::Interrupted), &mut rl)
            .expect("Reprompt must not itself error");

        assert_eq!(
            control,
            LoopControl::Continue,
            "Ctrl-C must tell run()'s loop to keep going, not stop"
        );
    }

    #[test]
    fn ctrl_c_wiring_does_not_stop_processing_that_follows() {
        // Proves the loop genuinely carries on past a Ctrl-C within the
        // same session — not just that one isolated call returns
        // `Continue` — by feeding a real command through immediately
        // afterward on the same `repl`/`rl` and checking it, too, reports
        // "keep going" (a non-Quit command never stops the loop).
        let mut repl = test_repl();
        let mut rl = DefaultEditor::new().expect("test editor must build");

        let after_interrupt = repl
            .process_readline_result(Err(ReadlineError::Interrupted), &mut rl)
            .expect("Reprompt must not itself error");
        assert_eq!(after_interrupt, LoopControl::Continue);

        let after_next_line = repl
            .process_readline_result(Ok("info".to_string()), &mut rl)
            .expect("a valid command must not error the wiring");
        assert_eq!(
            after_next_line,
            LoopControl::Continue,
            "the line typed right after Ctrl-C must still be processed"
        );
    }

    #[test]
    fn quit_wiring_stops_the_loop() {
        let mut repl = test_repl();
        let mut rl = DefaultEditor::new().expect("test editor must build");

        let control = repl
            .process_readline_result(Ok("quit".to_string()), &mut rl)
            .expect("quit must not itself error");

        assert_eq!(control, LoopControl::Stop);
    }

    #[test]
    fn ctrl_d_wiring_stops_the_loop() {
        let mut repl = test_repl();
        let mut rl = DefaultEditor::new().expect("test editor must build");

        let control = repl
            .process_readline_result(Err(ReadlineError::Eof), &mut rl)
            .expect("Ctrl-D must not itself error");

        assert_eq!(control, LoopControl::Stop);
    }
}
