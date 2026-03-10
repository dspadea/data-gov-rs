use data_gov::DataGovClient;
use data_gov::util::sanitize_path_component;
use tokio::runtime::Runtime;

use super::commands::{ReplCommand, SessionContext};
use super::display::{print_cli_help, print_package_details};
use super::{
    color_blue, color_blue_bold, color_bold, color_cyan, color_dimmed, color_green,
    color_green_bold, color_red, color_red_bold, color_yellow, color_yellow_bold,
};

/// Resolve a dataset_id from the command or fall back to session context.
fn resolve_dataset<'a>(
    explicit: &'a Option<String>,
    ctx: &'a SessionContext,
) -> Result<&'a str, &'static str> {
    explicit
        .as_deref()
        .or(ctx.dataset.as_deref())
        .ok_or("no dataset specified and none selected (use: select /org/dataset)")
}

/// Execute a command (shared between REPL and CLI modes).
///
/// The `ctx` is updated in place by `select` commands. Other commands read
/// from it to fill in omitted arguments.
pub fn execute_command(
    client: &DataGovClient,
    rt: &Runtime,
    command: ReplCommand,
    ctx: &mut SessionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        ReplCommand::Search { query, limit } => {
            handle_search(client, rt, &query, limit, ctx)?;
        }

        ReplCommand::Show { dataset_id } => {
            let id = resolve_dataset(&dataset_id, ctx)?;
            handle_show(client, rt, id)?;
        }

        ReplCommand::Download { args } => {
            handle_download(client, rt, &args, ctx)?;
        }

        ReplCommand::List { what } => {
            handle_list(client, rt, &what)?;
        }

        ReplCommand::Select { path } => {
            handle_select(ctx, &path)?;
        }

        ReplCommand::Info => {
            handle_info(client, ctx);
        }

        ReplCommand::SetDir { .. } => {
            println!(
                "{} lcd is only available in interactive REPL mode",
                color_red_bold("Error:")
            );
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

/// Handle select/cd command
fn handle_select(ctx: &mut SessionContext, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    ctx.apply_navigate(path)?;

    let label = ctx.prompt_label();
    if label.is_empty() {
        println!("{} Selection cleared", color_green_bold("OK"));
    } else {
        println!(
            "{} Active context: {}",
            color_green_bold("OK"),
            color_yellow_bold(&label)
        );
    }

    Ok(())
}

/// Handle search command
fn handle_search(
    client: &DataGovClient,
    rt: &Runtime,
    query: &str,
    limit: Option<i32>,
    ctx: &SessionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let org = ctx.org.as_deref();
    if let Some(org_name) = org {
        println!(
            "{} '{}' in org {}...",
            color_cyan("Searching for"),
            query,
            color_yellow(org_name)
        );
    } else {
        println!("{} '{}'...", color_cyan("Searching for"), query);
    }

    let results = rt.block_on(client.search(query, limit, None, org, None))?;

    println!(
        "\n{} {} results:\n",
        color_green_bold("Found"),
        results.count.unwrap_or(0)
    );

    if let Some(packages) = results.results {
        for (i, package) in packages.iter().enumerate().take(20) {
            println!(
                "{}. {} {}",
                color_blue_bold(&format!("{:2}", i + 1)),
                color_yellow_bold(&package.name),
                color_dimmed(package.title.as_deref().unwrap_or(""))
            );

            if let Some(notes) = &package.notes {
                let truncated = if notes.chars().count() > 100 {
                    let s: String = notes.chars().take(100).collect();
                    format!("{s}...")
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

    Ok(())
}

/// Handle show command
fn handle_show(
    client: &DataGovClient,
    rt: &Runtime,
    dataset_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("{} dataset '{}'...", color_cyan("Fetching"), dataset_id);

    let package = rt.block_on(client.get_dataset(dataset_id))?;
    print_package_details(&package);

    Ok(())
}

/// Handle download command.
///
/// Interpretation depends on session context:
/// - **Active dataset**: all args are resource selectors (index or name).
/// - **No active dataset**: first arg is dataset ID, rest are resource selectors.
/// - **No args + active dataset**: download all resources.
/// - **No args + no active dataset**: error.
///
/// Each selector that doesn't match a resource is reported as an error.
fn handle_download(
    client: &DataGovClient,
    rt: &Runtime,
    args: &[String],
    ctx: &SessionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let (dataset_id, selectors) = if ctx.dataset.is_some() {
        // In a dataset: all args are resource selectors
        let id = ctx.dataset.as_deref().unwrap();
        (id, args)
    } else if let Some(first) = args.first() {
        // No active dataset: first arg is dataset ID, rest are selectors
        (first.as_str(), &args[1..])
    } else {
        return Err("no dataset specified and none selected (use: select /org/dataset)".into());
    };

    println!("{} dataset '{}'...", color_cyan("Fetching"), dataset_id);

    let package = rt.block_on(client.get_dataset(dataset_id))?;
    let resources = DataGovClient::get_downloadable_resources(&package);

    if resources.is_empty() {
        println!(
            "{} No downloadable resources found in this dataset.",
            color_yellow_bold("Warning:")
        );
        return Ok(());
    }

    let safe_dataset_id = sanitize_path_component(dataset_id);
    let dataset_dir = client.download_dir().join(&safe_dataset_id);

    if selectors.is_empty() {
        // No selectors — download all resources
        let results = rt.block_on(client.download_resources(&resources, Some(&dataset_dir)));
        print_download_summary(&results);
    } else {
        // Resolve each selector to matching resources
        download_selected(client, rt, selectors, &resources, &dataset_dir)?;
    }

    Ok(())
}

/// Resolve selectors and download matching resources.
///
/// Each selector is either a numeric index or a name (case-insensitive substring).
/// Unmatched selectors are reported as errors but don't stop other downloads.
fn download_selected(
    client: &DataGovClient,
    rt: &Runtime,
    selectors: &[String],
    resources: &[data_gov::ckan::models::Resource],
    dataset_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut success_count = 0;
    let mut error_count = 0;

    for selector in selectors {
        if let Ok(index) = selector.parse::<usize>() {
            // Numeric index
            if index >= resources.len() {
                println!(
                    "  {} '{}': index out of range (0-{})",
                    color_red("✗"),
                    selector,
                    resources.len() - 1
                );
                error_count += 1;
                continue;
            }
            let resource = &resources[index];
            match rt.block_on(client.download_resource(resource, Some(dataset_dir))) {
                Ok(path) => {
                    success_count += 1;
                    println!(
                        "  {} {}: {}",
                        color_green("✓"),
                        color_yellow(selector),
                        color_blue(&path.display().to_string())
                    );
                }
                Err(e) => {
                    error_count += 1;
                    println!(
                        "  {} {}: {}",
                        color_red("✗"),
                        selector,
                        color_red(&e.to_string())
                    );
                }
            }
        } else {
            // Name match (case-insensitive substring)
            let sel_lower = selector.to_lowercase();
            let matches: Vec<_> = resources
                .iter()
                .filter(|r| {
                    r.name
                        .as_ref()
                        .is_some_and(|n| n.to_lowercase().contains(&sel_lower))
                })
                .collect();

            if matches.is_empty() {
                println!("  {} '{}': no matching resource", color_red("✗"), selector);
                print_available_resources(resources);
                error_count += 1;
                continue;
            }

            for resource in &matches {
                let name = resource.name.as_deref().unwrap_or("unnamed");
                match rt.block_on(client.download_resource(resource, Some(dataset_dir))) {
                    Ok(path) => {
                        success_count += 1;
                        println!(
                            "  {} {}: {}",
                            color_green("✓"),
                            color_yellow(name),
                            color_blue(&path.display().to_string())
                        );
                    }
                    Err(e) => {
                        error_count += 1;
                        println!(
                            "  {} {}: {}",
                            color_red("✗"),
                            name,
                            color_red(&e.to_string())
                        );
                    }
                }
            }
        }
    }

    if success_count + error_count > 1 {
        println!(
            "\n{} {} downloaded, {} errors",
            color_bold("Summary:"),
            color_green(&success_count.to_string()),
            color_red(&error_count.to_string())
        );
    }

    Ok(())
}

/// Print available resources to help the user find what they want.
fn print_available_resources(resources: &[data_gov::ckan::models::Resource]) {
    println!("    Available resources:");
    for (i, r) in resources.iter().enumerate() {
        let name = r.name.as_deref().unwrap_or("(unnamed)");
        let format = r.format.as_deref().unwrap_or("?");
        println!("      {} {} [{}]", i, name, format);
    }
}

/// Print download summary for bulk downloads (no selectors).
fn print_download_summary(results: &[Result<std::path::PathBuf, data_gov::DataGovError>]) {
    let mut success_count = 0;
    let mut error_count = 0;

    for (i, result) in results.iter().enumerate() {
        match result {
            Ok(path) => {
                success_count += 1;
                println!(
                    "  {} Resource {}: {}",
                    color_green("✓"),
                    i,
                    color_blue(&path.display().to_string())
                );
            }
            Err(e) => {
                error_count += 1;
                println!(
                    "  {} Resource {}: {}",
                    color_red("✗"),
                    i,
                    color_red(&e.to_string())
                );
            }
        }
    }

    println!(
        "\n{} {} downloaded, {} errors",
        color_bold("Summary:"),
        color_green(&success_count.to_string()),
        color_red(&error_count.to_string())
    );
}

/// Handle list command
fn handle_list(
    client: &DataGovClient,
    rt: &Runtime,
    what: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match what.to_lowercase().as_str() {
        "organizations" | "orgs" => {
            println!("{} organizations...", color_cyan("Fetching"));

            let orgs = rt.block_on(client.list_organizations(Some(50)))?;

            println!("\n{} organizations:", color_green_bold("Government"));
            for (i, org) in orgs.iter().enumerate() {
                println!(
                    "{}. {}",
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

    Ok(())
}

/// Handle info command
fn handle_info(client: &DataGovClient, ctx: &SessionContext) {
    println!("\n{}", color_blue_bold("📊 Client Information"));
    let label = ctx.prompt_label();
    if !label.is_empty() {
        println!("Active context:    {}", color_yellow_bold(&label));
    }
    if let Some(org) = &ctx.org {
        println!("Active org:        {}", color_yellow(org));
    }
    if let Some(ds) = &ctx.dataset {
        println!("Active dataset:    {}", color_yellow(ds));
    }
    println!(
        "Download directory: {}",
        color_blue(&client.download_dir().display().to_string())
    );
    println!(
        "CKAN endpoint:     {}",
        color_blue("https://catalog.data.gov/api/3")
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_resource_name_matching_case_insensitive() {
        // Create test resources
        let resources = [
            data_gov::ckan::models::Resource {
                name: Some("Data.CSV".to_string()),
                format: Some("CSV".to_string()),
                url: Some("https://example.com/data.csv".to_string()),
                ..Default::default()
            },
            data_gov::ckan::models::Resource {
                name: Some("report.json".to_string()),
                format: Some("JSON".to_string()),
                url: Some("https://example.com/report.json".to_string()),
                ..Default::default()
            },
            data_gov::ckan::models::Resource {
                name: Some("ARCHIVE.CSV".to_string()),
                format: Some("CSV".to_string()),
                url: Some("https://example.com/archive.csv".to_string()),
                ..Default::default()
            },
        ];

        // Test case-insensitive matching for "csv"
        let name_lower = "csv".to_lowercase();
        let matching: Vec<_> = resources
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.name
                    .as_ref()
                    .map(|n| n.to_lowercase().contains(&name_lower))
                    .unwrap_or(false)
            })
            .collect();

        assert_eq!(matching.len(), 2);
        assert_eq!(matching[0].0, 0); // Data.CSV
        assert_eq!(matching[1].0, 2); // ARCHIVE.CSV
    }

    #[test]
    fn test_resource_name_matching_partial() {
        // Create test resources
        let resources = [
            data_gov::ckan::models::Resource {
                name: Some("complaints-2023.csv".to_string()),
                format: Some("CSV".to_string()),
                url: Some("https://example.com/file1.csv".to_string()),
                ..Default::default()
            },
            data_gov::ckan::models::Resource {
                name: Some("data.json".to_string()),
                format: Some("JSON".to_string()),
                url: Some("https://example.com/data.json".to_string()),
                ..Default::default()
            },
            data_gov::ckan::models::Resource {
                name: Some("complaints-2024.csv".to_string()),
                format: Some("CSV".to_string()),
                url: Some("https://example.com/file2.csv".to_string()),
                ..Default::default()
            },
        ];

        // Test partial matching for "complaint"
        let name_lower = "complaint".to_lowercase();
        let matching: Vec<_> = resources
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.name
                    .as_ref()
                    .map(|n| n.to_lowercase().contains(&name_lower))
                    .unwrap_or(false)
            })
            .collect();

        assert_eq!(matching.len(), 2);
        assert_eq!(matching[0].0, 0); // complaints-2023.csv
        assert_eq!(matching[1].0, 2); // complaints-2024.csv
    }

    #[test]
    fn test_resource_name_no_matches() {
        // Create test resources
        let resources = [
            data_gov::ckan::models::Resource {
                name: Some("data.csv".to_string()),
                format: Some("CSV".to_string()),
                url: Some("https://example.com/data.csv".to_string()),
                ..Default::default()
            },
            data_gov::ckan::models::Resource {
                name: Some("report.json".to_string()),
                format: Some("JSON".to_string()),
                url: Some("https://example.com/report.json".to_string()),
                ..Default::default()
            },
        ];

        // Test with no matches
        let name_lower = "pdf".to_lowercase();
        let matching: Vec<_> = resources
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.name
                    .as_ref()
                    .map(|n| n.to_lowercase().contains(&name_lower))
                    .unwrap_or(false)
            })
            .collect();

        assert_eq!(matching.len(), 0);
    }
}
