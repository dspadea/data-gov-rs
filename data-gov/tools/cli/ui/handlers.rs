use data_gov::DataGovClient;
use data_gov::util::sanitize_path_component;
use tokio::runtime::Runtime;

use super::commands::{ReplCommand, ResourceSelector};
use super::display::{print_cli_help, print_package_details};
use super::{
    color_blue, color_blue_bold, color_bold, color_cyan, color_dimmed, color_green,
    color_green_bold, color_red, color_red_bold, color_yellow, color_yellow_bold,
};

/// Execute a command (shared between REPL and CLI modes)
pub fn execute_command(
    client: &DataGovClient,
    rt: &Runtime,
    command: ReplCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        ReplCommand::Search { query, limit } => {
            handle_search(client, rt, &query, limit)?;
        }

        ReplCommand::Show { dataset_id } => {
            handle_show(client, rt, &dataset_id)?;
        }

        ReplCommand::Download {
            dataset_id,
            resource_selector,
        } => {
            handle_download(client, rt, &dataset_id, resource_selector)?;
        }

        ReplCommand::List { what } => {
            handle_list(client, rt, &what)?;
        }

        ReplCommand::Info => {
            handle_info(client);
        }

        ReplCommand::SetDir { .. } => {
            println!(
                "{} SetDir command is only available in interactive REPL mode",
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

/// Handle search command
fn handle_search(
    client: &DataGovClient,
    rt: &Runtime,
    query: &str,
    limit: Option<i32>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("{} '{}'...", color_cyan("Searching for"), query);

    let results = rt.block_on(client.search(query, limit, None, None, None))?;

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

/// Handle download command
fn handle_download(
    client: &DataGovClient,
    rt: &Runtime,
    dataset_id: &str,
    resource_selector: Option<ResourceSelector>,
) -> Result<(), Box<dyn std::error::Error>> {
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

    match resource_selector {
        Some(ResourceSelector::Index(index)) => {
            handle_download_by_index(client, rt, dataset_id, &resources, index)?;
        }
        Some(ResourceSelector::Name(name)) => {
            handle_download_by_name(client, rt, dataset_id, &resources, &name)?;
        }
        None => {
            handle_download_all(client, rt, dataset_id, &resources)?;
        }
    }

    Ok(())
}

/// Handle download by index
fn handle_download_by_index(
    client: &DataGovClient,
    rt: &Runtime,
    dataset_id: &str,
    resources: &[data_gov::ckan::models::Resource],
    index: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    if index >= resources.len() {
        println!(
            "{} Resource index {} is out of range (0-{})",
            color_red_bold("Error:"),
            index,
            resources.len() - 1
        );
        return Ok(());
    }

    let resource = &resources[index];
    println!("{} resource {}...", color_cyan("Downloading"), index);

    // Download to dataset-specific directory - sanitize to prevent path traversal
    let safe_dataset_id = sanitize_path_component(dataset_id);
    let dataset_dir = client.download_dir().join(&safe_dataset_id);
    let path = rt.block_on(client.download_resource(resource, Some(&dataset_dir)))?;

    println!(
        "{} Downloaded to: {}",
        color_green_bold("Success!"),
        color_blue(&path.display().to_string())
    );

    Ok(())
}

/// Handle download by name
fn handle_download_by_name(
    client: &DataGovClient,
    rt: &Runtime,
    dataset_id: &str,
    resources: &[data_gov::ckan::models::Resource],
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let name_lower = name.to_lowercase();
    let matching_resources: Vec<(usize, &data_gov::ckan::models::Resource)> = resources
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r.name
                .as_ref()
                .map(|n| n.to_lowercase().contains(&name_lower))
                .unwrap_or(false)
        })
        .collect();

    if matching_resources.is_empty() {
        println!(
            "{} No resources found matching name '{}'",
            color_yellow_bold("Warning:"),
            name
        );
        println!("\nAvailable resources:");
        for (i, r) in resources.iter().enumerate() {
            let rname = r.name.as_deref().unwrap_or("(unnamed)");
            let format = r.format.as_deref().unwrap_or("?");
            println!("  {} {} [{}]", i, rname, format);
        }
        return Ok(());
    }

    if matching_resources.len() == 1 {
        let (idx, resource) = matching_resources[0];
        println!(
            "{} resource {} matching '{}'...",
            color_cyan("Downloading"),
            idx,
            name
        );

        // Sanitize to prevent path traversal
        let safe_dataset_id = sanitize_path_component(dataset_id);
        let dataset_dir = client.download_dir().join(&safe_dataset_id);
        let path = rt.block_on(client.download_resource(resource, Some(&dataset_dir)))?;

        println!(
            "{} Downloaded to: {}",
            color_green_bold("Success!"),
            color_blue(&path.display().to_string())
        );
    } else {
        // Multiple matches - download all
        println!(
            "{} {} resources matching '{}'...",
            color_cyan("Downloading"),
            matching_resources.len(),
            name
        );

        // Sanitize to prevent path traversal
        let safe_dataset_id = sanitize_path_component(dataset_id);
        let dataset_dir = client.download_dir().join(&safe_dataset_id);
        let resources_to_download: Vec<_> = matching_resources
            .iter()
            .map(|(_, r)| (*r).clone())
            .collect();
        let results =
            rt.block_on(client.download_resources(&resources_to_download, Some(&dataset_dir)));

        print_download_summary(&results, Some(&matching_resources));
    }

    Ok(())
}

/// Handle download all resources
fn handle_download_all(
    client: &DataGovClient,
    rt: &Runtime,
    dataset_id: &str,
    resources: &[data_gov::ckan::models::Resource],
) -> Result<(), Box<dyn std::error::Error>> {
    // Sanitize to prevent path traversal
    let safe_dataset_id = sanitize_path_component(dataset_id);
    let dataset_dir = client.download_dir().join(&safe_dataset_id);
    let results = rt.block_on(client.download_resources(resources, Some(&dataset_dir)));

    print_download_summary(&results, None);

    Ok(())
}

/// Print download summary
fn print_download_summary(
    results: &[Result<std::path::PathBuf, data_gov::DataGovError>],
    resource_indices: Option<&[(usize, &data_gov::ckan::models::Resource)]>,
) {
    let mut success_count = 0;
    let mut error_count = 0;

    for (i, result) in results.iter().enumerate() {
        let idx = resource_indices.map(|ri| ri[i].0).unwrap_or(i);

        match result {
            Ok(path) => {
                success_count += 1;
                println!(
                    "  {} Resource {}: {}",
                    color_green("✓"),
                    idx,
                    color_blue(&path.display().to_string())
                );
            }
            Err(e) => {
                error_count += 1;
                println!(
                    "  {} Resource {}: {}",
                    color_red("✗"),
                    idx,
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
fn handle_info(client: &DataGovClient) {
    println!("\n{}", color_blue_bold("📊 Client Information"));
    println!(
        "Download directory: {}",
        color_blue(&client.download_dir().display().to_string())
    );
    println!(
        "CKAN endpoint: {}",
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
