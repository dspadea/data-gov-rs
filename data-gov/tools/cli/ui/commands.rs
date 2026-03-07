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
        dataset_id: String,
    },
    Download {
        dataset_id: String,
        resource_selector: Option<ResourceSelector>,
    },
    List {
        what: String,
    }, // organizations, formats, etc.
    SetDir {
        path: PathBuf,
    },
    Info,
    Help,
    Quit,
}

/// Resource selector for download command
#[derive(Debug, Clone)]
pub enum ResourceSelector {
    Index(usize),
    Name(String),
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
                if parts.len() != 2 {
                    return Err("Usage: show <dataset_id>".to_string());
                }
                Ok(ReplCommand::Show {
                    dataset_id: parts[1].clone(),
                })
            }
            "download" | "dl" => {
                if parts.len() < 2 || parts.len() > 3 {
                    return Err("Usage: download <dataset_id> [resource_index_or_name]".to_string());
                }
                let resource_selector = if parts.len() == 3 {
                    // Try to parse as index first, otherwise treat as name
                    if let Ok(index) = parts[2].parse::<usize>() {
                        Some(ResourceSelector::Index(index))
                    } else {
                        Some(ResourceSelector::Name(parts[2].clone()))
                    }
                } else {
                    None
                };
                Ok(ReplCommand::Download {
                    dataset_id: parts[1].clone(),
                    resource_selector,
                })
            }
            "list" | "ls" => {
                if parts.len() != 2 {
                    return Err("Usage: list <organizations|orgs>".to_string());
                }
                Ok(ReplCommand::List {
                    what: parts[1].clone(),
                })
            }
            "setdir" | "cd" => {
                if parts.len() != 2 {
                    return Err("Usage: setdir <path>".to_string());
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
    fn test_parse_download_command_with_index() {
        let result = ReplCommand::from_str("download my-dataset 0");
        assert!(result.is_ok());

        if let Ok(ReplCommand::Download {
            dataset_id,
            resource_selector,
        }) = result
        {
            assert_eq!(dataset_id, "my-dataset");
            assert!(matches!(
                resource_selector,
                Some(ResourceSelector::Index(0))
            ));
        } else {
            panic!("Expected Download command with Index(0)");
        }
    }

    #[test]
    fn test_parse_download_command_with_large_index() {
        let result = ReplCommand::from_str("download my-dataset 42");
        assert!(result.is_ok());

        if let Ok(ReplCommand::Download {
            dataset_id,
            resource_selector,
        }) = result
        {
            assert_eq!(dataset_id, "my-dataset");
            assert!(matches!(
                resource_selector,
                Some(ResourceSelector::Index(42))
            ));
        } else {
            panic!("Expected Download command with Index(42)");
        }
    }

    #[test]
    fn test_parse_download_command_with_name() {
        let result = ReplCommand::from_str("download my-dataset csv");
        assert!(result.is_ok());

        if let Ok(ReplCommand::Download {
            dataset_id,
            resource_selector,
        }) = result
        {
            assert_eq!(dataset_id, "my-dataset");
            if let Some(ResourceSelector::Name(name)) = resource_selector {
                assert_eq!(name, "csv");
            } else {
                panic!("Expected Download command with Name(csv)");
            }
        } else {
            panic!("Expected Download command");
        }
    }

    #[test]
    fn test_parse_download_command_with_partial_name() {
        let result = ReplCommand::from_str("download dataset-id complaints");
        assert!(result.is_ok());

        if let Ok(ReplCommand::Download {
            dataset_id,
            resource_selector,
        }) = result
        {
            assert_eq!(dataset_id, "dataset-id");
            if let Some(ResourceSelector::Name(name)) = resource_selector {
                assert_eq!(name, "complaints");
            } else {
                panic!("Expected Download command with Name(complaints)");
            }
        } else {
            panic!("Expected Download command");
        }
    }

    #[test]
    fn test_parse_download_command_no_selector() {
        let result = ReplCommand::from_str("download my-dataset");
        assert!(result.is_ok());

        if let Ok(ReplCommand::Download {
            dataset_id,
            resource_selector,
        }) = result
        {
            assert_eq!(dataset_id, "my-dataset");
            assert!(resource_selector.is_none());
        } else {
            panic!("Expected Download command with no selector");
        }
    }

    #[test]
    fn test_parse_download_command_dl_alias() {
        let result = ReplCommand::from_str("dl my-dataset 0");
        assert!(result.is_ok());

        if let Ok(ReplCommand::Download {
            dataset_id,
            resource_selector,
        }) = result
        {
            assert_eq!(dataset_id, "my-dataset");
            assert!(matches!(
                resource_selector,
                Some(ResourceSelector::Index(0))
            ));
        } else {
            panic!("Expected Download command using 'dl' alias");
        }
    }

    #[test]
    fn test_parse_download_command_with_extension() {
        // This should be treated as a name since it's not a valid number
        let result = ReplCommand::from_str("download my-dataset data.csv");
        assert!(result.is_ok());

        if let Ok(ReplCommand::Download {
            dataset_id,
            resource_selector,
        }) = result
        {
            assert_eq!(dataset_id, "my-dataset");
            if let Some(ResourceSelector::Name(name)) = resource_selector {
                assert_eq!(name, "data.csv");
            } else {
                panic!("Expected Download command with Name(data.csv)");
            }
        } else {
            panic!("Expected Download command");
        }
    }

    #[test]
    fn test_parse_download_command_too_many_args() {
        let result = ReplCommand::from_str("download dataset-id resource extra-arg");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("Usage: download <dataset_id> [resource_index_or_name]")
        );
    }

    #[test]
    fn test_parse_download_command_too_few_args() {
        let result = ReplCommand::from_str("download");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("Usage: download <dataset_id> [resource_index_or_name]")
        );
    }

    #[test]
    fn test_resource_selector_numeric_string_is_index() {
        // "0" should be parsed as Index(0), not Name("0")
        let result = ReplCommand::from_str("download dataset 0");
        if let Ok(ReplCommand::Download {
            resource_selector, ..
        }) = result
        {
            assert!(matches!(
                resource_selector,
                Some(ResourceSelector::Index(0))
            ));
        } else {
            panic!("Expected Index, not Name");
        }

        // "999" should be parsed as Index(999), not Name("999")
        let result = ReplCommand::from_str("download dataset 999");
        if let Ok(ReplCommand::Download {
            resource_selector, ..
        }) = result
        {
            assert!(matches!(
                resource_selector,
                Some(ResourceSelector::Index(999))
            ));
        } else {
            panic!("Expected Index, not Name");
        }
    }

    #[test]
    fn test_resource_selector_mixed_string_is_name() {
        // "0abc" cannot be parsed as usize, should be Name
        let result = ReplCommand::from_str("download dataset 0abc");
        if let Ok(ReplCommand::Download {
            resource_selector, ..
        }) = result
        {
            if let Some(ResourceSelector::Name(name)) = resource_selector {
                assert_eq!(name, "0abc");
            } else {
                panic!("Expected Name(0abc)");
            }
        } else {
            panic!("Expected Download command");
        }
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
        assert!(result.is_ok());

        if let Ok(ReplCommand::Download {
            dataset_id,
            resource_selector,
        }) = result
        {
            assert_eq!(dataset_id, "my-dataset");
            if let Some(ResourceSelector::Name(name)) = resource_selector {
                assert_eq!(name, "CSV File");
            } else {
                panic!("Expected Download command with Name(CSV File)");
            }
        } else {
            panic!("Expected Download command");
        }
    }

    #[test]
    fn test_parse_download_with_long_quoted_name() {
        let result = ReplCommand::from_str("download dataset \"Comma Separated Values File\"");
        assert!(result.is_ok());

        if let Ok(ReplCommand::Download {
            dataset_id,
            resource_selector,
        }) = result
        {
            assert_eq!(dataset_id, "dataset");
            if let Some(ResourceSelector::Name(name)) = resource_selector {
                assert_eq!(name, "Comma Separated Values File");
            } else {
                panic!("Expected Download command with quoted name");
            }
        } else {
            panic!("Expected Download command");
        }
    }
}
