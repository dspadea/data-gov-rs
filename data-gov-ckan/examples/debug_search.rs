use data_gov_ckan::{CkanClient, Configuration};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Arc::new(Configuration {
        base_path: "https://catalog.data.gov/api/3".to_string(),
        user_agent: Some("debug-test/1.0".to_string()),
        client: reqwest::Client::new(),
        basic_auth: None,
        oauth_access_token: None,
        bearer_access_token: None,
        api_key: None,
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

            if let Some(results) = &result.results && let Some(first) = results.first() {
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
