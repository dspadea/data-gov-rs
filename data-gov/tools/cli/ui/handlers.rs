use data_gov::DataGovClient;
use data_gov::catalog::models::Distribution;
use data_gov::util::sanitize_path_component;
use tokio::runtime::Runtime;

use super::commands::{ReplCommand, SessionContext};
use super::display::{print_cli_help, print_package_details};
use super::{
    color_blue, color_blue_bold, color_bold, color_cyan, color_dimmed, color_green,
    color_green_bold, color_red, color_red_bold, color_yellow, color_yellow_bold,
};

/// Resolve a dataset slug from the command or fall back to session context.
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
            let slug = resolve_dataset(&dataset_id, ctx)?;
            handle_show(client, rt, slug)?;
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

/// Handle select/cd command.
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

/// Handle search command.
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

    let page = rt.block_on(client.search(query, limit, None, org))?;
    let more_available = page.after.is_some();
    let shown = page.results.len();

    if more_available {
        println!(
            "\n{} {} results on this page (more available):\n",
            color_green_bold("Found"),
            shown
        );
    } else {
        println!("\n{} {} results:\n", color_green_bold("Found"), shown);
    }

    for (i, hit) in page.results.iter().enumerate().take(20) {
        let slug = hit.slug.as_deref().unwrap_or("(no-slug)");
        println!(
            "{}. {} {}",
            color_blue_bold(&format!("{:2}", i + 1)),
            color_yellow_bold(slug),
            color_dimmed(hit.title.as_deref().unwrap_or(""))
        );

        if let Some(description) = &hit.description {
            let truncated = if description.chars().count() > 100 {
                let s: String = description.chars().take(100).collect();
                format!("{s}...")
            } else {
                description.clone()
            };
            println!("   {}", color_dimmed(&truncated));
        }
        println!();
    }

    if shown > 20 {
        println!("... and {} more results on this page", shown - 20);
    }

    Ok(())
}

/// Handle show command.
fn handle_show(
    client: &DataGovClient,
    rt: &Runtime,
    dataset_slug: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("{} dataset '{}'...", color_cyan("Fetching"), dataset_slug);

    let hit = rt.block_on(client.get_dataset(dataset_slug))?;
    print_package_details(&hit);

    Ok(())
}

/// Collect the downloadable distributions from a fetched dataset hit, with a
/// helpful error if the hit has no DCAT record attached.
fn downloadable_for(
    hit: &data_gov::catalog::models::SearchHit,
) -> Result<Vec<Distribution>, Box<dyn std::error::Error>> {
    let dcat = hit
        .dcat
        .as_ref()
        .ok_or("dataset is missing DCAT metadata; cannot determine distributions")?;
    Ok(DataGovClient::get_downloadable_distributions(dcat))
}

/// Handle download command.
///
/// Interpretation depends on session context:
/// - **Active dataset**: all args are distribution selectors (index or title).
/// - **No active dataset**: first arg is the dataset slug, rest are selectors.
/// - **No args + active dataset**: download all distributions.
/// - **No args + no active dataset**: error.
///
/// Each selector that doesn't match a distribution is reported as an error.
fn handle_download(
    client: &DataGovClient,
    rt: &Runtime,
    args: &[String],
    ctx: &SessionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let (dataset_slug, selectors) = if ctx.dataset.is_some() {
        let id = ctx.dataset.as_deref().unwrap();
        (id, args)
    } else if let Some(first) = args.first() {
        (first.as_str(), &args[1..])
    } else {
        return Err("no dataset specified and none selected (use: select /org/dataset)".into());
    };

    println!("{} dataset '{}'...", color_cyan("Fetching"), dataset_slug);

    let hit = rt.block_on(client.get_dataset(dataset_slug))?;
    let distributions = downloadable_for(&hit)?;

    if distributions.is_empty() {
        println!(
            "{} No downloadable distributions found in this dataset.",
            color_yellow_bold("Warning:")
        );
        return Ok(());
    }

    let safe_dataset_slug = sanitize_path_component(dataset_slug);
    let dataset_dir = client.download_dir().join(&safe_dataset_slug);

    if selectors.is_empty() {
        let results =
            rt.block_on(client.download_distributions(&distributions, Some(&dataset_dir)));
        print_download_summary(&results);
    } else {
        download_selected(client, rt, selectors, &distributions, &dataset_dir)?;
    }

    Ok(())
}

/// Resolve selectors and download matching distributions.
///
/// Each selector is either a numeric index or a title (case-insensitive
/// substring). Unmatched selectors are reported but don't stop other downloads.
fn download_selected(
    client: &DataGovClient,
    rt: &Runtime,
    selectors: &[String],
    distributions: &[Distribution],
    dataset_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut success_count = 0;
    let mut error_count = 0;

    for selector in selectors {
        if let Ok(index) = selector.parse::<usize>() {
            if index >= distributions.len() {
                println!(
                    "  {} '{}': index out of range (0-{})",
                    color_red("✗"),
                    selector,
                    distributions.len() - 1
                );
                error_count += 1;
                continue;
            }
            let distribution = &distributions[index];
            match rt.block_on(client.download_distribution(distribution, Some(dataset_dir))) {
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
            let sel_lower = selector.to_lowercase();
            let matches: Vec<_> = distributions
                .iter()
                .filter(|d| {
                    d.title
                        .as_ref()
                        .is_some_and(|t| t.to_lowercase().contains(&sel_lower))
                })
                .collect();

            if matches.is_empty() {
                println!(
                    "  {} '{}': no matching distribution",
                    color_red("✗"),
                    selector
                );
                print_available_distributions(distributions);
                error_count += 1;
                continue;
            }

            for distribution in &matches {
                let title = distribution.title.as_deref().unwrap_or("untitled");
                match rt.block_on(client.download_distribution(distribution, Some(dataset_dir))) {
                    Ok(path) => {
                        success_count += 1;
                        println!(
                            "  {} {}: {}",
                            color_green("✓"),
                            color_yellow(title),
                            color_blue(&path.display().to_string())
                        );
                    }
                    Err(e) => {
                        error_count += 1;
                        println!(
                            "  {} {}: {}",
                            color_red("✗"),
                            title,
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

/// Print available distributions to help the user find what they want.
fn print_available_distributions(distributions: &[Distribution]) {
    println!("    Available distributions:");
    for (i, d) in distributions.iter().enumerate() {
        let title = d.title.as_deref().unwrap_or("(untitled)");
        let format = d
            .format
            .as_deref()
            .or(d.media_type.as_deref())
            .unwrap_or("?");
        println!("      {i} {title} [{format}]");
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
                    "  {} Distribution {}: {}",
                    color_green("✓"),
                    i,
                    color_blue(&path.display().to_string())
                );
            }
            Err(e) => {
                error_count += 1;
                println!(
                    "  {} Distribution {}: {}",
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

/// Handle list command.
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

/// Handle info command.
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
        "Catalog endpoint:  {}",
        color_blue(&client.config().catalog_config.base_path)
    );
}

#[cfg(test)]
mod tests {
    use data_gov::catalog::models::Distribution;

    fn dist(title: &str) -> Distribution {
        Distribution {
            type_hint: None,
            title: Some(title.to_string()),
            description: None,
            download_url: Some(format!("https://example.com/{title}")),
            access_url: None,
            media_type: None,
            format: None,
            license: None,
            described_by: None,
            described_by_type: None,
        }
    }

    #[test]
    fn title_matching_is_case_insensitive() {
        let distributions = [dist("Data.CSV"), dist("report.json"), dist("ARCHIVE.CSV")];

        let needle = "csv".to_lowercase();
        let matches: Vec<_> = distributions
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                d.title
                    .as_ref()
                    .is_some_and(|t| t.to_lowercase().contains(&needle))
            })
            .collect();

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].0, 0);
        assert_eq!(matches[1].0, 2);
    }

    #[test]
    fn title_matching_partial() {
        let distributions = [
            dist("complaints-2023.csv"),
            dist("data.json"),
            dist("complaints-2024.csv"),
        ];

        let needle = "complaint".to_string();
        let matches: Vec<_> = distributions
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                d.title
                    .as_ref()
                    .is_some_and(|t| t.to_lowercase().contains(&needle))
            })
            .collect();

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].0, 0);
        assert_eq!(matches[1].0, 2);
    }

    #[test]
    fn title_matching_no_matches() {
        let distributions = [dist("data.csv"), dist("report.json")];

        let needle = "pdf".to_string();
        let matches: Vec<_> = distributions
            .iter()
            .filter(|d| {
                d.title
                    .as_ref()
                    .is_some_and(|t| t.to_lowercase().contains(&needle))
            })
            .collect();

        assert!(matches.is_empty());
    }
}
