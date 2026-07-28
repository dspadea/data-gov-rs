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
    /// Fetch the next page of the most recent listing or search.
    Next,
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

/// Cursor describing what was last listed so a subsequent `next` knows
/// what to advance.
#[derive(Debug, Clone)]
pub enum ListingCursor {
    /// Datasets in `org`, paginated via `after`.
    OrgDatasets {
        org: String,
        after: String,
        page_size: i32,
    },
    /// Search results for `query` (optionally filtered by org), paginated
    /// via `after`. Mirrors the args originally passed to `search`.
    SearchResults {
        query: String,
        organization: Option<String>,
        after: String,
        page_size: i32,
    },
}

/// Active session context set via `select /org/dataset`.
#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    pub org: Option<String>,
    pub dataset: Option<String>,
    /// Pagination cursor from the most recent listing or search. Populated
    /// when the previous response carried an `after` cursor; consumed by
    /// the `next` command.
    pub last_listing: Option<ListingCursor>,
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
    ///
    /// Splits on `/` and folds each segment in order, the way a shell folds
    /// a relative path one component at a time. `.` and empty segments
    /// (from a leading, trailing, or doubled `/`) are no-ops; `..` goes up
    /// one level (or is a no-op at root); anything else is an org name (at
    /// root) or a dataset name (at an org) — and an error if a dataset is
    /// already selected, since there's nowhere deeper to go.
    fn apply_relative(&mut self, path: &str) -> Result<(), String> {
        if path.is_empty() {
            return Err("empty path".to_string());
        }

        for segment in path.split('/') {
            match segment {
                "" | "." => {} // no-op
                ".." => {
                    if self.dataset.is_some() {
                        self.dataset = None;
                    } else if self.org.is_some() {
                        self.org = None;
                    }
                    // Already at root — no-op
                }
                name => {
                    if self.dataset.is_some() {
                        return Err(format!(
                            "already in a dataset; use '..' to go up first, or use an absolute path: /org/{path}"
                        ));
                    }
                    if self.org.is_some() {
                        // At org level — the segment is a dataset.
                        self.dataset = Some(name.to_string());
                    } else {
                        // At root — the segment is an org.
                        self.org = Some(name.to_string());
                    }
                }
            }
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

/// Expand a leading `~` or `~/` against the user's home directory.
///
/// The REPL's tokenizer does no shell-style expansion, so unlike a real
/// shell (which expands `~` before `--download-dir` ever sees it),
/// `PathBuf::from("~/dgtest")` would otherwise reach the config verbatim and
/// `validate_download_dir` would create a literal `~` directory. Only a
/// leading `~` is special — `foo~bar` is a literal path component, exactly
/// as it is in a shell.
fn expand_tilde(raw: &str) -> Result<PathBuf, String> {
    if raw == "~" {
        return dirs::home_dir()
            .ok_or_else(|| "could not determine home directory to expand '~'".to_string());
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        let home = dirs::home_dir()
            .ok_or_else(|| "could not determine home directory to expand '~'".to_string())?;
        return Ok(home.join(rest));
    }
    Ok(PathBuf::from(raw))
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
                // Parse the limit off the tail FIRST, then build the query
                // from what remains — never the other way round, or the
                // limit token stays embedded in the text sent to the API.
                //
                // Decision: a numeric trailing token is *always* read as the
                // limit, never as query text, even for a query that
                // genuinely ends in a number (e.g. "route 66"). The
                // alternative — guessing from context whether a trailing
                // number is "meant" as a limit — has no reliable signal to
                // guess from, so the rule is kept mechanical: shape decides
                // meaning. A one-word query is always literal, since
                // stripping it as a limit would leave nothing to search for.
                let mut rest = &parts[1..];
                let mut limit = None;
                if rest.len() > 1
                    && let Ok(n) = rest[rest.len() - 1].parse::<i32>()
                {
                    if !(1..=1000).contains(&n) {
                        return Err(format!("limit must be between 1 and 1000, got {n}"));
                    }
                    limit = Some(n);
                    rest = &rest[..rest.len() - 1];
                }
                let query = rest.join(" ");
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
                    path: expand_tilde(&parts[1])?,
                })
            }
            "info" | "status" => Ok(ReplCommand::Info),
            "next" | "n" | "more" => {
                if parts.len() != 1 {
                    return Err("Usage: next  (no arguments)".to_string());
                }
                Ok(ReplCommand::Next)
            }
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
            last_listing: None,
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
            last_listing: None,
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
            last_listing: None,
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
            last_listing: None,
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
            last_listing: None,
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
            last_listing: None,
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
            last_listing: None,
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

    // --- SessionContext: multi-segment relative paths (#69.1) ---

    #[test]
    fn test_relative_multi_segment_from_root_sets_org_and_dataset() {
        let mut ctx = SessionContext::default();
        ctx.apply_navigate("epa/some-dataset").unwrap();
        assert_eq!(ctx.org, Some("epa".to_string()));
        assert_eq!(ctx.dataset, Some("some-dataset".to_string()));
    }

    #[test]
    fn test_relative_dotdot_dotdot_from_dataset_reaches_root() {
        let mut ctx = SessionContext {
            org: Some("epa-gov".to_string()),
            dataset: Some("air-quality".to_string()),
            last_listing: None,
        };
        ctx.apply_navigate("../..").unwrap();
        assert!(ctx.org.is_none());
        assert!(ctx.dataset.is_none());
    }

    #[test]
    fn test_relative_dotdot_sibling_org_from_org_level() {
        let mut ctx = SessionContext {
            org: Some("epa-gov".to_string()),
            dataset: None,
            last_listing: None,
        };
        ctx.apply_navigate("../noaa").unwrap();
        assert_eq!(ctx.org, Some("noaa".to_string()));
        assert!(ctx.dataset.is_none());
    }

    #[test]
    fn test_relative_multi_segment_from_dataset_errors() {
        let mut ctx = SessionContext {
            org: Some("epa-gov".to_string()),
            dataset: Some("air-quality".to_string()),
            last_listing: None,
        };
        let result = ctx.apply_navigate("another/thing");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already in a dataset"));
    }

    // --- SessionContext: `cd .` is a no-op (#69.2) ---

    #[test]
    fn test_dot_from_root_is_noop() {
        let mut ctx = SessionContext::default();
        ctx.apply_navigate(".").unwrap();
        assert!(ctx.org.is_none());
        assert!(ctx.dataset.is_none());
    }

    #[test]
    fn test_dot_from_org_is_noop() {
        let mut ctx = SessionContext {
            org: Some("epa-gov".to_string()),
            dataset: None,
            last_listing: None,
        };
        ctx.apply_navigate(".").unwrap();
        assert_eq!(ctx.org, Some("epa-gov".to_string()));
        assert!(ctx.dataset.is_none());
    }

    #[test]
    fn test_dot_from_dataset_is_noop() {
        let mut ctx = SessionContext {
            org: Some("epa-gov".to_string()),
            dataset: Some("air-quality".to_string()),
            last_listing: None,
        };
        ctx.apply_navigate(".").unwrap();
        assert_eq!(ctx.org, Some("epa-gov".to_string()));
        assert_eq!(ctx.dataset, Some("air-quality".to_string()));
    }

    // --- SessionContext: prompt_label ---

    #[test]
    fn test_prompt_label_org_and_dataset() {
        let ctx = SessionContext {
            org: Some("epa-gov".to_string()),
            dataset: Some("air-quality".to_string()),
            last_listing: None,
        };
        assert_eq!(ctx.prompt_label(), "/epa-gov/air-quality");
    }

    #[test]
    fn test_prompt_label_dataset_only() {
        let ctx = SessionContext {
            org: None,
            dataset: Some("orphan-ds".to_string()),
            last_listing: None,
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

    // --- lcd tilde expansion (#69.3) ---

    #[test]
    fn test_lcd_tilde_alone_expands_to_home() {
        let home = dirs::home_dir().expect("test environment must have a home directory");
        let result = ReplCommand::from_str("lcd ~");
        let Ok(ReplCommand::SetDir { path }) = result else {
            panic!("Expected SetDir command");
        };
        assert_eq!(path, home);
    }

    #[test]
    fn test_lcd_tilde_slash_expands_against_home() {
        let home = dirs::home_dir().expect("test environment must have a home directory");
        let result = ReplCommand::from_str("lcd ~/dgtest");
        let Ok(ReplCommand::SetDir { path }) = result else {
            panic!("Expected SetDir command");
        };
        assert_eq!(path, home.join("dgtest"));
        // Never a literal "~" path component.
        assert!(!path.components().any(|c| c.as_os_str() == "~"));
    }

    #[test]
    fn test_lcd_without_tilde_is_unchanged() {
        let result = ReplCommand::from_str("lcd ./downloads");
        let Ok(ReplCommand::SetDir { path }) = result else {
            panic!("Expected SetDir command");
        };
        assert_eq!(path, PathBuf::from("./downloads"));
    }

    #[test]
    fn test_lcd_tilde_mid_path_is_left_alone() {
        // Only a *leading* "~" is a home-directory reference; "foo~bar" is a
        // literal path component in a shell, and stays literal here too.
        let result = ReplCommand::from_str("lcd foo~bar");
        let Ok(ReplCommand::SetDir { path }) = result else {
            panic!("Expected SetDir command");
        };
        assert_eq!(path, PathBuf::from("foo~bar"));
    }

    #[test]
    fn test_parse_show_without_dataset() {
        let result = ReplCommand::from_str("show");
        let Ok(ReplCommand::Show { dataset_id }) = result else {
            panic!("Expected Show command");
        };
        assert!(dataset_id.is_none());
    }

    // --- Command parsing: search query/limit split (#54) ---

    #[test]
    fn test_search_with_limit_removes_limit_token_from_query() {
        let result = ReplCommand::from_str("search climate change 5");
        let Ok(ReplCommand::Search { query, limit }) = result else {
            panic!("Expected Search command");
        };
        assert_eq!(query, "climate change");
        assert_eq!(limit, Some(5));
    }

    #[test]
    fn test_search_quoted_multiword_query_with_limit() {
        let result = ReplCommand::from_str("search \"electric vehicle\" 10");
        let Ok(ReplCommand::Search { query, limit }) = result else {
            panic!("Expected Search command");
        };
        assert_eq!(query, "electric vehicle");
        assert_eq!(limit, Some(10));
    }

    #[test]
    fn test_search_without_limit_keeps_full_query() {
        let result = ReplCommand::from_str("search electric vehicle");
        let Ok(ReplCommand::Search { query, limit }) = result else {
            panic!("Expected Search command");
        };
        assert_eq!(query, "electric vehicle");
        assert_eq!(limit, None);
    }

    #[test]
    fn test_search_single_numeric_word_is_the_whole_query_not_a_limit() {
        // With only one word after "search", there's nothing left to search
        // for if we strip it as a limit, so it must stay part of the query.
        let result = ReplCommand::from_str("search 2020");
        let Ok(ReplCommand::Search { query, limit }) = result else {
            panic!("Expected Search command");
        };
        assert_eq!(query, "2020");
        assert_eq!(limit, None);
    }

    #[test]
    fn test_search_rejects_non_positive_limit() {
        let result = ReplCommand::from_str("search climate 0");
        assert!(result.is_err());
        let result = ReplCommand::from_str("search climate -5");
        assert!(result.is_err());
    }

    #[test]
    fn test_search_rejects_limit_over_api_maximum() {
        // The Catalog API rejects per_page outside 1..=1000; reject it at the
        // CLI boundary with a clear usage error instead of forwarding it and
        // letting the API return a 400.
        let result = ReplCommand::from_str("search census 2020");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("1000"));
    }

    #[test]
    fn test_search_trailing_in_range_number_is_always_treated_as_limit() {
        // Decision: a numeric trailing token is always parsed as the limit,
        // never as query text, even when it happens to be a legitimate part
        // of the search terms (e.g. "route 66"). This keeps the rule
        // predictable: the last token's shape (numeric or not) is the only
        // thing that decides its meaning, with no additional heuristics.
        let result = ReplCommand::from_str("search route 66");
        let Ok(ReplCommand::Search { query, limit }) = result else {
            panic!("Expected Search command");
        };
        assert_eq!(query, "route");
        assert_eq!(limit, Some(66));
    }
}
