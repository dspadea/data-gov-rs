//! #106: `DataGovConfig` must expose exactly one place to set the user
//! agent, so a caller who reaches it without `with_user_agent` still gets one
//! identity on the wire rather than two.
//!
//! Before the fix, `user_agent` lived on both `DataGovConfig` and its nested
//! `catalog_config`, and only `with_user_agent` kept the two in step. A
//! struct literal reaching either field directly left the other stale, so
//! metadata requests and downloads could go out under different identities.
//! `catalog_config` is the field that survives the fix, so these tests set it
//! directly -- never through `DataGovConfig::with_user_agent` -- and check
//! that both a catalog request and a download see the same agent.

use data_gov::catalog::Configuration as CatalogConfiguration;
use data_gov::catalog::models::Distribution;
use data_gov::{DataGovClient, DataGovConfig};
use std::sync::Arc;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CUSTOM_AGENT: &str = "acme-harvester/1.0";

fn distribution(url: &str) -> Distribution {
    Distribution {
        type_hint: None,
        title: Some("shared".to_string()),
        description: None,
        download_url: Some(url.to_string()),
        access_url: None,
        media_type: None,
        format: Some("CSV".to_string()),
        license: None,
        described_by: None,
        described_by_type: None,
    }
}

/// A `DataGovConfig` whose `catalog_config` was reached directly, with the
/// custom agent baked in from construction rather than applied by
/// `with_user_agent` afterward. This is the "wrong call" #106 exists for.
fn config_with_agent_set_via_catalog_config(
    base_url: &str,
    download_dir: std::path::PathBuf,
) -> DataGovConfig {
    let catalog_config = CatalogConfiguration {
        base_path: base_url.to_string(),
        user_agent: Some(CUSTOM_AGENT.to_string()),
        ..CatalogConfiguration::default()
    };
    DataGovConfig {
        catalog_config: Arc::new(catalog_config),
        base_download_dir: download_dir,
        allow_private_network_downloads: true,
        ..DataGovConfig::default()
    }
}

#[tokio::test]
async fn a_user_agent_set_without_the_builder_reaches_both_catalog_requests_and_downloads() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/organizations"))
        .and(header("User-Agent", CUSTOM_AGENT))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "organizations": [], "total": 0 })),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/files/data.csv"))
        .and(header("User-Agent", CUSTOM_AGENT))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"payload".to_vec()))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().expect("tempdir");
    let config = config_with_agent_set_via_catalog_config(&server.uri(), tmp.path().to_path_buf());
    let client = DataGovClient::with_config(config).expect("client must build");

    client
        .list_organizations(None)
        .await
        .expect("the catalog request must carry the configured agent");

    let dist = distribution(&format!("{}/files/data.csv", server.uri()));
    let outcome = client
        .download_distribution(&dist, Some(tmp.path()))
        .await;
    assert!(
        outcome.is_ok(),
        "the download request must carry the same configured agent, got {outcome:?}"
    );
}

#[tokio::test]
async fn with_user_agent_builder_reaches_both_catalog_requests_and_downloads() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/organizations"))
        .and(header("User-Agent", CUSTOM_AGENT))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "organizations": [], "total": 0 })),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/files/data.csv"))
        .and(header("User-Agent", CUSTOM_AGENT))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"payload".to_vec()))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().expect("tempdir");
    let config = DataGovConfig::new()
        .with_base_url(server.uri())
        .with_download_dir(tmp.path().to_path_buf())
        .with_private_network_downloads(true)
        .with_user_agent(CUSTOM_AGENT);
    let client = DataGovClient::with_config(config).expect("client must build");

    client
        .list_organizations(None)
        .await
        .expect("the catalog request must carry the configured agent");

    let dist = distribution(&format!("{}/files/data.csv", server.uri()));
    let outcome = client
        .download_distribution(&dist, Some(tmp.path()))
        .await;
    assert!(
        outcome.is_ok(),
        "the download request must carry the same configured agent, got {outcome:?}"
    );
}
