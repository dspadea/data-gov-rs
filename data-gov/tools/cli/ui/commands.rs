use std::path::PathBuf;
use std::str::FromStr;

/// REPL Commands
#[derive(Debug, Clone)]
pub enum ReplCommand {
    Search {
        query: String,
        limit: Option<i32>,
    },
    Show {
        dataset_id: Option<String>,
    },
    Download {
        /// Raw arguments — interpretation depends on session context.
        /// In a dataset: all args are resource selectors.
        /// Otherwise: first arg is dataset, rest are resource selectors.
        args: Vec<String>,
    },
    List {
        /// Explicit subject (`organizations`/`orgs`). When `None`, the command
        /// is context-dependent: at root it lists organizations, at an org it
        /// lists that org's datasets, and at a dataset it lists distributions.
        what: Option<String>,
    },
    Select {
        path: String,
    },
    SetDir {
        path: PathBuf,
    },
    Info,
    Help,
    Quit,
}

/// Active session context set via `select /org/dataset`.
#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    pub org: Option<String>,
    pub dataset: Option<String>,
}

impl SessionContext {
    /// Navigate the context, similar to `cd` in a filesystem.
    ///
    /// Absolute paths (leading `/`):
    /// - `/org/dataset` — set both org and dataset
    /// - `/org` or `/org/` — set org, clear dataset
    /// - `/` — clear both (go to root)
    ///
    /// Relative paths (no leading `/`):
    /// - At root: `org` sets the org
    /// - At org: `dataset` sets the dataset
    /// - At dataset: error (nowhere deeper to go)
    ///
    /// Special:
    /// - `..` — go up one level (dataset→org, org→root)
    pub fn apply_navigate(&mut self, path: &str) -> Result<(), String> {
        if path.starts_with('/') {
            return self.apply_absolute(path);
        }
        self.apply_relative(path)
    }

    /// Handle absolute path navigation (leading `/`).
    fn apply_absolute(&mut self, path: &str) -> Result<(), String> {
        let inner = &path[1..]; // strip leading '/'

        if inner.is_empty() {
            // `/` — clear everything
            self.org = None;
            self.dataset = None;
            return Ok(());
        }

        // `/org` or `/org/dataset`
        match inner.split_once('/') {
            None => {
                self.org = Some(inner.to_string());
                self.dataset = None;
            }
            Some((org, rest)) => {
                let dataset = rest.trim_end_matches('/');
                self.org = Some(org.to_string());
                if dataset.is_empty() {
                    self.dataset = None;
                } else {
                    self.dataset = Some(dataset.to_string());
                }
            }
        }

        Ok(())
    }

    /// Handle relative path navigation (no leading `/`).
    fn apply_relative(&mut self, path: &str) -> Result<(), String> {
        let path = path.trim_end_matches('/');

        if path == ".." {
            // Go up one level
            if self.dataset.is_some() {
                self.dataset = None;
            } else if self.org.is_some() {
                self.org = None;
            }
            // Already at root — no-op
            return Ok(());
        }

        if path.is_empty() {
            return Err("empty path".to_string());
        }

        if self.dataset.is_some() {
            return Err(format!(
                "already in a dataset; use '..' to go up first, or use an absolute path: /org/{path}"
            ));
        }

        if self.org.is_some() {
            // At org level — relative path is a dataset
            self.dataset = Some(path.to_string());
        } else {
            // At root — relative path is an org
            self.org = Some(path.to_string());
        }

        Ok(())
    }

    /// Format the context as a prompt-friendly string.
    pub fn prompt_label(&self) -> String {
        match (&self.org, &self.dataset) {
            (Some(org), Some(ds)) => format!("/{org}/{ds}"),
            (Some(org), None) => format!("/{org}"),
            (None, Some(ds)) => format!("//{ds}"),
            (None, None) => String::new(),
        }
    }
}

/// Parse a command string respecting quoted arguments
/// Example: `foo bar "baz qux"` -> ["foo", "bar", "baz qux"]
pub fn parse_command_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current_arg = String::new();
    let mut in_quotes = false;
    let chars = s.trim().chars().peekable();

    for ch in chars {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
            }
            ' ' | '\t' if !in_quotes => {
                if !current_arg.is_empty() {
                    args.push(current_arg.clone());
                    current_arg.clear();
                }
            }
            _ => {
                current_arg.push(ch);
            }
        }
    }

    if !current_arg.is_empty() {
        args.push(current_arg);
    }

    args
}

impl ReplCommand {
    pub fn from_parts(parts: &[String]) -> Result<Self, String> {
        if parts.is_empty() {
            return Err("Empty command".to_string());
        }

        let command = parts[0].to_lowercase();

        match command.as_str() {
            "search" | "s" => {
                if parts.len() < 2 {
                    return Err("Usage: search <query> [limit]".to_string());
                }
                let query = parts[1..].join(" ");
                let limit = if parts.len() > 2 {
                    parts.last().and_then(|s| s.parse().ok())
                } else {
                    None
                };
                Ok(ReplCommand::Search { query, limit })
            }
            "show" | "describe" | "d" => {
                if parts.len() > 2 {
                    return Err("Usage: show [dataset_id]".to_string());
                }
                Ok(ReplCommand::Show {
                    dataset_id: parts.get(1).cloned(),
                })
            }
            "download" | "dl" => Ok(ReplCommand::Download {
                args: parts[1..].to_vec(),
            }),
            "select" | "sel" | "cd" => {
                if parts.len() != 2 {
                    return Err(
                        "Usage: cd <path>  (e.g. cd nasa-gov, cd air-quality, cd .., cd /org/dataset, cd /)"
                            .to_string(),
                    );
                }
                Ok(ReplCommand::Select {
                    path: parts[1].clone(),
                })
            }
            "list" | "ls" => {
                let what = match parts.len() {
                    1 => None,
                    2 => Some(parts[1].clone()),
                    _ => {
                        return Err("Usage: ls [organizations|orgs]".to_string());
                    }
                };
                Ok(ReplCommand::List { what })
            }
            "lcd" | "setdir" => {
                if parts.len() != 2 {
                    return Err("Usage: lcd <path>".to_string());
                }
                Ok(ReplCommand::SetDir {
                    path: PathBuf::from(&parts[1]),
                })
            }
            "info" | "status" => Ok(ReplCommand::Info),
            "help" | "h" | "?" => Ok(ReplCommand::Help),
            "quit" | "exit" | "q" => Ok(ReplCommand::Quit),
            _ => Err(format!("Unknown command: {}", parts[0])),
        }
    }
}

impl FromStr for ReplCommand {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = parse_command_args(s);
        ReplCommand::from_parts(&parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_download_with_dataset_and_index() {
        let result = ReplCommand::from_str("download my-dataset 0");
        let Ok(ReplCommand::Download { args }) = result else {
            panic!("Expected Download command");
        };
        assert_eq!(args, vec!["my-dataset", "0"]);
    }

    #[test]
    fn test_parse_download_with_dataset_and_name() {
        let result = ReplCommand::from_str("download my-dataset csv");
        let Ok(ReplCommand::Download { args }) = result else {
            panic!("Expected Download command");
        };
        assert_eq!(args, vec!["my-dataset", "csv"]);
    }

    #[test]
    fn test_parse_download_dataset_only() {
        let result = ReplCommand::from_str("download my-dataset");
        let Ok(ReplCommand::Download { args }) = result else {
            panic!("Expected Download command");
        };
        assert_eq!(args, vec!["my-dataset"]);
    }

    #[test]
    fn test_parse_download_dl_alias() {
        let result = ReplCommand::from_str("dl my-dataset 0");
        let Ok(ReplCommand::Download { args }) = result else {
            panic!("Expected Download command");
        };
        assert_eq!(args, vec!["my-dataset", "0"]);
    }

    #[test]
    fn test_parse_download_no_args() {
        let result = ReplCommand::from_str("download");
        let Ok(ReplCommand::Download { args }) = result else {
            panic!("Expected Download command");
        };
        assert!(args.is_empty());
    }

    #[test]
    fn test_parse_download_multiple_selectors() {
        // "download dataset-id "RDF File" "XML File"" — multiple resource selectors
        let result = ReplCommand::from_str("download dataset-id \"RDF File\" \"XML File\"");
        let Ok(ReplCommand::Download { args }) = result else {
            panic!("Expected Download command");
        };
        assert_eq!(args, vec!["dataset-id", "RDF File", "XML File"]);
    }

    #[test]
    fn test_parse_download_multiple_indices() {
        let result = ReplCommand::from_str("download 0 1 2");
        let Ok(ReplCommand::Download { args }) = result else {
            panic!("Expected Download command");
        };
        assert_eq!(args, vec!["0", "1", "2"]);
    }

    #[test]
    fn test_parse_command_args_simple() {
        let args = parse_command_args("download dataset 0");
        assert_eq!(args, vec!["download", "dataset", "0"]);
    }

    #[test]
    fn test_parse_command_args_with_quotes() {
        let args = parse_command_args("download dataset \"Comma Separated Values File\"");
        assert_eq!(
            args,
            vec!["download", "dataset", "Comma Separated Values File"]
        );
    }

    #[test]
    fn test_parse_command_args_multiple_spaces() {
        let args = parse_command_args("search   climate    data");
        assert_eq!(args, vec!["search", "climate", "data"]);
    }

    #[test]
    fn test_parse_command_args_quotes_with_extra_spaces() {
        let args = parse_command_args("download   dataset   \"Multi Word Name\"  ");
        assert_eq!(args, vec!["download", "dataset", "Multi Word Name"]);
    }

    #[test]
    fn test_parse_download_with_quoted_name() {
        let result = ReplCommand::from_str("download my-dataset \"CSV File\"");
        let Ok(ReplCommand::Download { args }) = result else {
            panic!("Expected Download command");
        };
        assert_eq!(args, vec!["my-dataset", "CSV File"]);
    }

    #[test]
    fn test_parse_download_with_long_quoted_name() {
        let result = ReplCommand::from_str("download dataset \"Comma Separated Values File\"");
        let Ok(ReplCommand::Download { args }) = result else {
            panic!("Expected Download command");
        };
        assert_eq!(args, vec!["dataset", "Comma Separated Values File"]);
    }

    // --- SessionContext: absolute path tests ---

    #[test]
    fn test_absolute_org_and_dataset() {
        let mut ctx = SessionContext::default();
        ctx.apply_navigate("/epa-gov/air-quality").unwrap();
        assert_eq!(ctx.org, Some("epa-gov".to_string()));
        assert_eq!(ctx.dataset, Some("air-quality".to_string()));
        assert_eq!(ctx.prompt_label(), "/epa-gov/air-quality");
    }

    #[test]
    fn test_absolute_org_only() {
        let mut ctx = SessionContext::default();
        ctx.apply_navigate("/nasa-gov").unwrap();
        assert_eq!(ctx.org, Some("nasa-gov".to_string()));
        assert!(ctx.dataset.is_none());
        assert_eq!(ctx.prompt_label(), "/nasa-gov");
    }

    #[test]
    fn test_absolute_org_with_trailing_slash() {
        let mut ctx = SessionContext::default();
        ctx.apply_navigate("/epa-gov/").unwrap();
        assert_eq!(ctx.org, Some("epa-gov".to_string()));
        assert!(ctx.dataset.is_none());
    }

    #[test]
    fn test_absolute_root_clears_all() {
        let mut ctx = SessionContext {
            org: Some("epa-gov".to_string()),
            dataset: Some("air-quality".to_string()),
        };
        ctx.apply_navigate("/").unwrap();
        assert!(ctx.org.is_none());
        assert!(ctx.dataset.is_none());
        assert_eq!(ctx.prompt_label(), "");
    }

    #[test]
    fn test_absolute_replaces_previous_context() {
        let mut ctx = SessionContext {
            org: Some("old-org".to_string()),
            dataset: Some("old-dataset".to_string()),
        };
        ctx.apply_navigate("/new-org/new-dataset").unwrap();
        assert_eq!(ctx.org, Some("new-org".to_string()));
        assert_eq!(ctx.dataset, Some("new-dataset".to_string()));
    }

    #[test]
    fn test_absolute_org_clears_dataset() {
        let mut ctx = SessionContext {
            org: Some("old-org".to_string()),
            dataset: Some("old-dataset".to_string()),
        };
        ctx.apply_navigate("/new-org").unwrap();
        assert_eq!(ctx.org, Some("new-org".to_string()));
        assert!(ctx.dataset.is_none());
    }

    // --- SessionContext: relative path tests ---

    #[test]
    fn test_relative_org_from_root() {
        let mut ctx = SessionContext::default();
        ctx.apply_navigate("nasa-gov").unwrap();
        assert_eq!(ctx.org, Some("nasa-gov".to_string()));
        assert!(ctx.dataset.is_none());
    }

    #[test]
    fn test_relative_dataset_from_org() {
        let mut ctx = SessionContext {
            org: Some("epa-gov".to_string()),
            dataset: None,
        };
        ctx.apply_navigate("water-data").unwrap();
        assert_eq!(ctx.org, Some("epa-gov".to_string()));
        assert_eq!(ctx.dataset, Some("water-data".to_string()));
    }

    #[test]
    fn test_relative_from_dataset_errors() {
        let mut ctx = SessionContext {
            org: Some("epa-gov".to_string()),
            dataset: Some("air-quality".to_string()),
        };
        let result = ctx.apply_navigate("something");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already in a dataset"));
    }

    #[test]
    fn test_dotdot_from_dataset_to_org() {
        let mut ctx = SessionContext {
            org: Some("epa-gov".to_string()),
            dataset: Some("air-quality".to_string()),
        };
        ctx.apply_navigate("..").unwrap();
        assert_eq!(ctx.org, Some("epa-gov".to_string()));
        assert!(ctx.dataset.is_none());
    }

    #[test]
    fn test_dotdot_from_org_to_root() {
        let mut ctx = SessionContext {
            org: Some("epa-gov".to_string()),
            dataset: None,
        };
        ctx.apply_navigate("..").unwrap();
        assert!(ctx.org.is_none());
        assert!(ctx.dataset.is_none());
    }

    #[test]
    fn test_dotdot_from_root_is_noop() {
        let mut ctx = SessionContext::default();
        ctx.apply_navigate("..").unwrap();
        assert!(ctx.org.is_none());
        assert!(ctx.dataset.is_none());
    }

    #[test]
    fn test_relative_with_trailing_slash() {
        let mut ctx = SessionContext::default();
        ctx.apply_navigate("nasa-gov/").unwrap();
        assert_eq!(ctx.org, Some("nasa-gov".to_string()));
        assert!(ctx.dataset.is_none());
    }

    // --- SessionContext: prompt_label ---

    #[test]
    fn test_prompt_label_org_and_dataset() {
        let ctx = SessionContext {
            org: Some("epa-gov".to_string()),
            dataset: Some("air-quality".to_string()),
        };
        assert_eq!(ctx.prompt_label(), "/epa-gov/air-quality");
    }

    #[test]
    fn test_prompt_label_dataset_only() {
        let ctx = SessionContext {
            org: None,
            dataset: Some("orphan-ds".to_string()),
        };
        assert_eq!(ctx.prompt_label(), "//orphan-ds");
    }

    #[test]
    fn test_prompt_label_empty() {
        let ctx = SessionContext::default();
        assert_eq!(ctx.prompt_label(), "");
    }

    // --- Command parsing: select/cd/lcd ---

    #[test]
    fn test_parse_select_command() {
        let result = ReplCommand::from_str("select /epa-gov/air-quality");
        let Ok(ReplCommand::Select { path }) = result else {
            panic!("Expected Select command");
        };
        assert_eq!(path, "/epa-gov/air-quality");
    }

    #[test]
    fn test_parse_sel_alias() {
        let result = ReplCommand::from_str("sel /epa-gov");
        let Ok(ReplCommand::Select { path }) = result else {
            panic!("Expected Select command via 'sel' alias");
        };
        assert_eq!(path, "/epa-gov");
    }

    #[test]
    fn test_parse_cd_alias() {
        let result = ReplCommand::from_str("cd nasa-gov");
        let Ok(ReplCommand::Select { path }) = result else {
            panic!("Expected Select command via 'cd' alias");
        };
        assert_eq!(path, "nasa-gov");
    }

    #[test]
    fn test_parse_cd_dotdot() {
        let result = ReplCommand::from_str("cd ..");
        let Ok(ReplCommand::Select { path }) = result else {
            panic!("Expected Select command");
        };
        assert_eq!(path, "..");
    }

    #[test]
    fn test_parse_lcd_command() {
        let result = ReplCommand::from_str("lcd ./downloads");
        let Ok(ReplCommand::SetDir { path }) = result else {
            panic!("Expected SetDir command");
        };
        assert_eq!(path, PathBuf::from("./downloads"));
    }

    #[test]
    fn test_parse_setdir_alias() {
        let result = ReplCommand::from_str("setdir /tmp");
        let Ok(ReplCommand::SetDir { path }) = result else {
            panic!("Expected SetDir command via 'setdir' alias");
        };
        assert_eq!(path, PathBuf::from("/tmp"));
    }

    #[test]
    fn test_parse_show_without_dataset() {
        let result = ReplCommand::from_str("show");
        let Ok(ReplCommand::Show { dataset_id }) = result else {
            panic!("Expected Show command");
        };
        assert!(dataset_id.is_none());
    }
}
