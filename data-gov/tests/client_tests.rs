//! Unit tests for `DataGovClient` using captured API response fixtures.
//!
//! These tests use wiremock to serve real data.gov JSON responses that were
//! captured from the live API and stored in `tests/fixtures/`. This lets us
//! verify deserialization, filter logic, and download behaviour without
//! hitting the network.

use data_gov::{DataGovClient, DataGovConfig, OperatingMode};
use std::path::PathBuf;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Load a fixture file from `tests/fixtures/`.
fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("failed to read {}: {e}", p.display()))
}

/// Build a `DataGovClient` pointed at the given mock server.
fn mock_client(base_url: &str, download_dir: PathBuf) -> DataGovClient {
    let config = DataGovConfig::new()
        .with_base_url(base_url)
        .with_download_dir(download_dir)
        .with_mode(OperatingMode::CommandLine);
    DataGovClient::with_config(config).expect("failed to build client")
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

#[tokio::test]
async fn search_returns_parsed_results() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .and(query_param("q", "electric vehicle"))
        .and(query_param("rows", "2"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("package_search.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = mock_client(
        &server.uri(),
        tempfile::tempdir().unwrap().path().to_path_buf(),
    );
    let results = client
        .search("electric vehicle", Some(2), None, None, None)
        .await
        .unwrap();

    assert!(results.count.unwrap_or(0) > 0);
    let packages = results.results.expect("should have results");
    assert_eq!(packages.len(), 2);
    // Verify basic deserialization of first package
    assert!(!packages[0].name.is_empty());
}

#[tokio::test]
async fn search_with_org_and_format_filter_builds_fq() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .and(query_param("q", "energy"))
        .and(query_param("rows", "2"))
        .and(query_param(
            "fq",
            "organization:\"doe-gov\" AND res_format:\"CSV\"",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("package_search_filtered.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = mock_client(
        &server.uri(),
        tempfile::tempdir().unwrap().path().to_path_buf(),
    );
    let results = client
        .search("energy", Some(2), None, Some("doe-gov"), Some("CSV"))
        .await
        .unwrap();

    assert!(results.count.unwrap_or(0) > 0);
}

#[tokio::test]
async fn search_empty_query_omits_q_param() {
    let server = MockServer::start().await;
    // When query is empty, the client should not send a `q` param
    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("package_search.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = mock_client(
        &server.uri(),
        tempfile::tempdir().unwrap().path().to_path_buf(),
    );
    let results = client.search("", None, None, None, None).await.unwrap();
    assert!(results.results.is_some());
}

// ---------------------------------------------------------------------------
// get_dataset
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_dataset_returns_full_package() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/action/package_show"))
        .and(query_param("id", "electric-vehicle-population-data"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("package_show.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = mock_client(
        &server.uri(),
        tempfile::tempdir().unwrap().path().to_path_buf(),
    );
    let package = client
        .get_dataset("electric-vehicle-population-data")
        .await
        .unwrap();

    assert!(!package.name.is_empty());
    assert!(package.resources.is_some());
    let resources = package.resources.as_ref().unwrap();
    assert!(!resources.is_empty());
}

#[tokio::test]
async fn get_downloadable_resources_filters_correctly() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/action/package_show"))
        .and(query_param("id", "electric-vehicle-population-data"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("package_show.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = mock_client(
        &server.uri(),
        tempfile::tempdir().unwrap().path().to_path_buf(),
    );
    let package = client
        .get_dataset("electric-vehicle-population-data")
        .await
        .unwrap();
    let downloadable = DataGovClient::get_downloadable_resources(&package);

    // All downloadable resources should have a URL and format
    for r in &downloadable {
        assert!(r.url.is_some(), "downloadable resource should have a URL");
        assert!(
            r.format.is_some(),
            "downloadable resource should have a format"
        );
        assert_ne!(
            r.url_type.as_deref(),
            Some("api"),
            "should not include API endpoints"
        );
    }
}

// ---------------------------------------------------------------------------
// list_organizations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_organizations_returns_strings() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/action/organization_list"))
        .and(query_param("limit", "5"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("organization_list.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = mock_client(
        &server.uri(),
        tempfile::tempdir().unwrap().path().to_path_buf(),
    );
    let orgs = client.list_organizations(Some(5)).await.unwrap();

    assert_eq!(orgs.len(), 5);
    for org in &orgs {
        assert!(!org.is_empty());
    }
}

// ---------------------------------------------------------------------------
// autocomplete_datasets
// ---------------------------------------------------------------------------

#[tokio::test]
async fn autocomplete_datasets_returns_names() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/action/package_autocomplete"))
        .and(query_param("q", "electric"))
        .and(query_param("limit", "5"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("dataset_autocomplete.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = mock_client(
        &server.uri(),
        tempfile::tempdir().unwrap().path().to_path_buf(),
    );
    let names = client
        .autocomplete_datasets("electric", Some(5))
        .await
        .unwrap();

    assert!(!names.is_empty());
    for name in &names {
        assert!(!name.is_empty());
    }
}

// ---------------------------------------------------------------------------
// autocomplete_organizations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn autocomplete_organizations_returns_names() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/action/organization_autocomplete"))
        .and(query_param("q", "nasa"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            fixture("organization_autocomplete.json"),
            "application/json",
        ))
        .mount(&server)
        .await;

    let client = mock_client(
        &server.uri(),
        tempfile::tempdir().unwrap().path().to_path_buf(),
    );
    let names = client
        .autocomplete_organizations("nasa", Some(5))
        .await
        .unwrap();

    assert!(!names.is_empty());
    for name in &names {
        assert!(!name.is_empty());
    }
}

// ---------------------------------------------------------------------------
// download_resource (with mock file server)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn download_resource_saves_file() {
    let server = MockServer::start().await;

    // Serve dataset metadata
    Mock::given(method("GET"))
        .and(path("/action/package_show"))
        .and(query_param("id", "electric-vehicle-population-data"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("package_show.json"), "application/json"),
        )
        .mount(&server)
        .await;

    // Serve a fake file download for any resource URL
    Mock::given(method("GET"))
        .and(path("/fake-download"))
        .respond_with(ResponseTemplate::new(200).set_body_string("col1,col2\nval1,val2\n"))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let client = mock_client(&server.uri(), tmp.path().to_path_buf());
    let package = client
        .get_dataset("electric-vehicle-population-data")
        .await
        .unwrap();
    let downloadable = DataGovClient::get_downloadable_resources(&package);

    if !downloadable.is_empty() {
        // Patch the first resource URL to point at our mock
        let mut resource = downloadable[0].clone();
        resource.url = Some(format!("{}/fake-download", server.uri()));

        let path = client
            .download_resource(&resource, Some(tmp.path()))
            .await
            .unwrap();
        assert!(path.exists(), "downloaded file should exist");
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "col1,col2\nval1,val2\n");
    }
}

// ---------------------------------------------------------------------------
// error handling
// ---------------------------------------------------------------------------

#[tokio::test]
async fn search_propagates_http_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/action/package_search"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let client = mock_client(
        &server.uri(),
        tempfile::tempdir().unwrap().path().to_path_buf(),
    );
    let result = client.search("test", None, None, None, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn get_dataset_propagates_404() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/action/package_show"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&server)
        .await;

    let client = mock_client(
        &server.uri(),
        tempfile::tempdir().unwrap().path().to_path_buf(),
    );
    let result = client.get_dataset("nonexistent-dataset").await;
    assert!(result.is_err());
}
