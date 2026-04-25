use data_gov::DataGovClient;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("🇺🇸 Data.gov Rust Client Demo");
    println!("================================\n");

    let client = DataGovClient::new()?;

    println!("🔍 Searching for 'climate' datasets...");
    let page = client.search("climate", Some(5), None, None).await?;

    if page.after.is_some() {
        println!(
            "Found {} results on this page (more available):\n",
            page.results.len()
        );
    } else {
        println!("Found {} results:\n", page.results.len());
    }

    for (i, hit) in page.results.iter().enumerate() {
        let slug = hit.slug.as_deref().unwrap_or("(no-slug)");
        let title = hit.title.as_deref().unwrap_or(slug);
        println!("{}. {title} ({slug})", i + 1);

        let distribution_count = hit
            .dcat
            .as_ref()
            .map(DataGovClient::get_downloadable_distributions)
            .map(|d| d.len())
            .unwrap_or(0);
        println!("   📁 {distribution_count} downloadable distributions");

        if let Some(description) = &hit.description {
            let truncated = if description.chars().count() > 100 {
                let s: String = description.chars().take(100).collect();
                format!("{s}...")
            } else {
                description.clone()
            };
            println!("   📄 {truncated}");
        }
        println!();
    }

    println!("🏛️  Listing government organizations...");
    let orgs = client.list_organizations(Some(10)).await?;
    println!("Found {} organization slugs:", orgs.len());
    for (i, org) in orgs.iter().enumerate().take(5) {
        println!("  {}. {org}", i + 1);
    }
    println!();

    println!("🔍 Dataset title suggestions for 'energy'...");
    let suggestions = client.autocomplete_datasets("energy", Some(5)).await?;
    println!("Suggestions:");
    for suggestion in suggestions {
        println!("  • {suggestion}");
    }
    println!();

    println!("✅ Demo completed! Try the interactive REPL with:");
    println!("   data-gov");
    println!();
    println!("Example CLI commands:");
    println!("  data-gov search \"electric vehicle\"");
    println!("  data-gov show electric-vehicle-population-data");
    println!("  data-gov download electric-vehicle-population-data 0");
    println!("  data-gov list organizations");
    println!("  data-gov --help");

    Ok(())
}
