//! Integration tests exercising Solr query syntax features against data.gov.
//!
//! These tests perform real network calls and are marked `#[ignore]` by default.
//! Run them with:
//!
//! ```bash
//! cargo test -p data-gov-ckan --test solr_syntax_tests -- --ignored
//! ```
//!
use data_gov_ckan::{CkanClient, Configuration};
use std::sync::Arc;

fn create_test_client() -> CkanClient {
    let config = Arc::new(Configuration {
        base_path: "https://catalog.data.gov/api/3".to_string(),
        user_agent: Some("data-gov-ckan-solr-test/1.0".to_string()),
        client: reqwest::Client::new(),
        basic_auth: None,
        oauth_access_token: None,
        bearer_access_token: None,
        api_key: None,
    });

    CkanClient::new(config)
}

#[tokio::test]
#[ignore]
async fn test_wildcard_query() {
    let client = create_test_client();

    // Wildcard: should not error and return some results structure
    let res = client
        .package_search(Some("climat*"), Some(5), Some(0), None)
        .await
        .expect("Wildcard query should succeed");

    // Sanity: response parsed
    assert!(res.count.is_some() || res.results.is_some());
}

#[tokio::test]
#[ignore]
async fn test_phrase_query() {
    let client = create_test_client();

    // Phrase search
    let res = client
        .package_search(Some("\"air quality\""), Some(5), Some(0), None)
        .await
        .expect("Phrase query should succeed");

    assert!(res.count.is_some() || res.results.is_some());
}

#[tokio::test]
#[ignore]
async fn test_boolean_and_filter() {
    let client = create_test_client();

    // Fielded filter using boolean AND
    let fq = r#"organization:epa-gov AND res_format:CSV"#;
    let res = client
        .package_search(None, Some(5), Some(0), Some(fq))
        .await
        .expect("Boolean/fq filter should succeed");

    assert!(res.count.is_some() || res.results.is_some());
}

#[tokio::test]
#[ignore]
async fn test_range_filter() {
    let client = create_test_client();

    // Range query on metadata_modified (may be broad)
    let fq = r#"metadata_modified:[2020-01-01T00:00:00Z TO NOW]"#;
    let res = client
        .package_search(None, Some(5), Some(0), Some(fq))
        .await
        .expect("Range filter should succeed");

    assert!(res.count.is_some() || res.results.is_some());
}
