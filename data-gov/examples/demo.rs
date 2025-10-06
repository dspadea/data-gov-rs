use data_gov::DataGovClient;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("🇺🇸 Data.gov Rust Client Demo");
    println!("================================\n");

    // Create a client
    let client = DataGovClient::new()?;

    // 1. Search for datasets
    println!("🔍 Searching for 'climate' datasets...");
    let search_results = client.search("climate", Some(5), None, None, None).await?;

    println!("Found {} results:\n", search_results.count.unwrap_or(0));

    if let Some(results) = &search_results.results {
        for (i, dataset) in results.iter().enumerate() {
            println!(
                "{}. {} ({})",
                i + 1,
                dataset.title.as_deref().unwrap_or(&dataset.name),
                dataset.name
            );

            // Show resource count
            let resources = DataGovClient::get_downloadable_resources(dataset);
            println!("   📁 {} downloadable resources", resources.len());

            if let Some(notes) = &dataset.notes {
                let truncated = if notes.len() > 100 {
                    format!("{}...", &notes[..100])
                } else {
                    notes.clone()
                };
                println!("   📄 {}", truncated);
            }
            println!();
        }
    }

    // 2. Get organizations
    println!("🏛️  Listing government organizations...");
    let orgs = client.list_organizations(Some(10)).await?;
    println!("Found {} organizations:", orgs.len());
    for (i, org) in orgs.iter().enumerate().take(5) {
        println!("  {}. {}", i + 1, org);
    }
    println!();

    // 3. Autocomplete example
    println!("🔍 Autocomplete for 'energy'...");
    let suggestions = client.autocomplete_datasets("energy", Some(5)).await?;
    println!("Suggestions:");
    for suggestion in suggestions {
        println!("  • {}", suggestion);
    }
    println!();

    println!("✅ Demo completed! Try the interactive REPL with:");
    println!("   data-gov");
    println!();
    println!("Example CLI commands:");
    println!("  data-gov search \"electric vehicle\"");
    println!("  data-gov show electric-vehicle-population-data");
    println!("  data-gov download electric-vehicle-population-data 0                           # By index");
    println!("  data-gov download electric-vehicle-population-data \"Comma Separated Values File\"  # By name (quoted)");
    println!("  data-gov download electric-vehicle-population-data json                        # Partial match");
    println!("  data-gov list organizations");
    println!("  data-gov --help");

    Ok(())
}
