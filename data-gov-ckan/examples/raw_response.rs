use data_gov_ckan::Configuration;
use serde_json::Value;
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

    // Make the request manually to see the actual structure
    let url = format!(
        "{}/action/package_search?q=climate&rows=1",
        config.base_path
    );

    let response = config.client.get(&url).send().await?;

    let json: Value = response.json().await?;
    println!("Raw JSON structure:");
    println!("{}", serde_json::to_string_pretty(&json)?);

    Ok(())
}
