use data_gov_ckan::{CkanClient, Configuration};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Arc::new(Configuration {
        // data.gov retired its CKAN endpoint in 2026; open.canada.ca is a
        // live, government-run CKAN portal used here so the example works
        // unmodified. Point this at your own instance for real use.
        base_path: "https://open.canada.ca/data/en/api/3".to_string(),
        user_agent: Some("debug-test/1.0".to_string()),
        ..Configuration::default()
    });

    let client = CkanClient::new(config);

    println!("Testing basic search...");

    match client
        .package_search(Some("climate"), Some(1), Some(0), None)
        .await
    {
        Ok(result) => {
            println!("Success! Count: {:?}", result.count);
            println!(
                "Results length: {:?}",
                result.results.as_ref().map(|r| r.len())
            );

            if let Some(results) = &result.results
                && let Some(first) = results.first()
            {
                println!("First result title: {:?}", first.title);
                println!("First result name: {}", first.name);
            }
        }
        Err(e) => {
            println!("Error: {}", e);
        }
    }

    Ok(())
}
