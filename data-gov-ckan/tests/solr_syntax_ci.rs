use data_gov_ckan::{CkanClient, Configuration};
use std::sync::Arc;

fn create_test_client() -> CkanClient {
    let config = Arc::new(Configuration {
        base_path: "https://catalog.data.gov/api/3".to_string(),
        user_agent: Some("data-gov-ckan-ci-test/1.0".to_string()),
        client: reqwest::Client::new(),
        basic_auth: None,
        oauth_access_token: None,
        bearer_access_token: None,
        api_key: None,
    });

    CkanClient::new(config)
}

#[tokio::test]
async fn test_ckan_wildcard_and_fq() {
    let client = create_test_client();

    // Wildcard q
    let _ = client
        .package_search(Some("climat*"), Some(3), Some(0), None)
        .await
        .expect("wildcard query should succeed");

    // Fielded fq
    let fq = r#"organization:epa-gov AND res_format:CSV"#;
    let _ = client
        .package_search(None, Some(3), Some(0), Some(fq))
        .await
        .expect("fq filter should succeed");
}
