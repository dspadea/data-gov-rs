use data_gov_ckan::apis::configuration::Configuration;
use std::sync::Arc;
use serde_json::Value;

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
    
    // Make the request manually to see the actual structure
    let url = format!("{}/action/package_search?q=climate&rows=1", config.base_path);
    
    let response = config.client
        .get(&url)
        .send()
        .await?;
    
    let json: Value = response.json().await?;
    println!("Raw JSON structure:");
    println!("{}", serde_json::to_string_pretty(&json)?);
    
    Ok(())
}