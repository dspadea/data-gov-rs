//! Concurrency and partial-failure tests for [`DataGovClient::download_resources`].
//!
//! Each test constructs a `DataGovClient` pointed at a `wiremock` server and
//! supplies a slice of [`Resource`] records whose URLs target the mock. Tests
//! assert on caller-observable behavior:
//!
//! - Return-vector length and ordering match the input
//! - Partial failures surface as per-resource `Err` without short-circuiting
//! - Filenames for resources with identical names are disambiguated by index
//! - The `max_concurrent_downloads` limit is actually enforced

use std::time::{Duration, Instant};

use data_gov::ckan::models::Resource;
use data_gov::{DataGovClient, DataGovConfig, DataGovError, OperatingMode};
use tempfile::TempDir;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Build a client configured for predictable test behavior.
fn test_client(download_dir: std::path::PathBuf, max_concurrent: usize) -> DataGovClient {
    let config = DataGovConfig::default()
        .with_mode(OperatingMode::Interactive)
        .with_download_dir(download_dir)
        .with_max_concurrent_downloads(max_concurrent)
        .with_download_timeout(10);
    DataGovClient::with_config(config).expect("test client must build")
}

/// Create a Resource whose `url` points at the given mock path.
fn mock_resource(mock_uri: &str, file_path: &str, name: &str, format: &str) -> Resource {
    Resource {
        name: Some(name.to_string()),
        format: Some(format.to_string()),
        url: Some(format!("{mock_uri}{file_path}")),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Boundary: empty and single-element input
// ---------------------------------------------------------------------------

#[tokio::test]
async fn download_resources_with_empty_slice_returns_empty_vec() {
    let tmp = TempDir::new().expect("tempdir");
    let client = test_client(tmp.path().to_path_buf(), 3);

    let results = client.download_resources(&[], None).await;

    assert!(
        results.is_empty(),
        "empty input must produce empty output, got {} items",
        results.len()
    );
}

#[tokio::test]
async fn download_resources_with_single_resource_returns_single_result() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/files/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = test_client(tmp.path().to_path_buf(), 3);

    let resources = vec![mock_resource(&server.uri(), "/files/one.csv", "one", "CSV")];
    let results = client
        .download_resources(&resources, Some(tmp.path()))
        .await;

    assert_eq!(results.len(), 1, "one input must produce one result");
    let path = results.into_iter().next().unwrap().expect("should succeed");
    assert!(path.exists(), "downloaded file must exist at {path:?}");
}

// ---------------------------------------------------------------------------
// Happy path: all succeed, results line up one-to-one
// ---------------------------------------------------------------------------

#[tokio::test]
async fn download_resources_returns_one_result_per_input_in_order() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/files/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"payload".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = test_client(tmp.path().to_path_buf(), 3);

    let resources = vec![
        mock_resource(&server.uri(), "/files/a.csv", "a", "CSV"),
        mock_resource(&server.uri(), "/files/b.csv", "b", "CSV"),
        mock_resource(&server.uri(), "/files/c.csv", "c", "CSV"),
    ];
    let results = client
        .download_resources(&resources, Some(tmp.path()))
        .await;

    assert_eq!(results.len(), 3, "result count must match input count");
    for (i, r) in results.iter().enumerate() {
        assert!(r.is_ok(), "resource {i} should have succeeded: {r:?}");
    }
}

// ---------------------------------------------------------------------------
// Partial failure: a resource with no URL must not short-circuit the batch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn download_resources_with_mixed_url_and_no_url_returns_mixed_results() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/files/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"payload".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = test_client(tmp.path().to_path_buf(), 3);

    let resources = vec![
        mock_resource(&server.uri(), "/files/ok1.csv", "ok1", "CSV"),
        Resource {
            name: Some("no-url".to_string()),
            format: Some("CSV".to_string()),
            url: None,
            ..Default::default()
        },
        mock_resource(&server.uri(), "/files/ok2.csv", "ok2", "CSV"),
    ];

    let results = client
        .download_resources(&resources, Some(tmp.path()))
        .await;

    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok(), "first resource must succeed");
    match &results[1] {
        Err(DataGovError::ResourceNotFound { message }) => {
            assert!(
                message.contains("no URL") || message.contains("URL"),
                "error message should explain missing URL, got: {message}"
            );
        }
        other => panic!("expected ResourceNotFound, got {other:?}"),
    }
    assert!(results[2].is_ok(), "third resource must still succeed");
}

// ---------------------------------------------------------------------------
// Partial failure: per-resource HTTP errors surface without aborting the batch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn download_resources_propagates_per_resource_http_errors() {
    let server = MockServer::start().await;

    // /good/* returns 200; /bad/* returns 500
    Mock::given(method("GET"))
        .and(path_regex(r"^/good/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"ok".to_vec()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/bad/.*"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = test_client(tmp.path().to_path_buf(), 3);

    let resources = vec![
        mock_resource(&server.uri(), "/good/a.csv", "a", "CSV"),
        mock_resource(&server.uri(), "/bad/b.csv", "b", "CSV"),
        mock_resource(&server.uri(), "/good/c.csv", "c", "CSV"),
    ];

    let results = client
        .download_resources(&resources, Some(tmp.path()))
        .await;

    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok(), "good resource must succeed");
    match &results[1] {
        Err(DataGovError::DownloadError { message }) => {
            assert!(
                message.contains("500"),
                "error must mention HTTP status, got: {message}"
            );
        }
        other => panic!("expected DownloadError with 500, got {other:?}"),
    }
    assert!(results[2].is_ok(), "third resource must not be affected");
}

// ---------------------------------------------------------------------------
// Filename disambiguation: duplicate names must not overwrite each other
// ---------------------------------------------------------------------------

#[tokio::test]
async fn download_resources_disambiguates_duplicate_filenames_by_index() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/same/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"content".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = test_client(tmp.path().to_path_buf(), 3);

    // Three resources with the same name + format — filenames would collide
    // without index insertion.
    let resources = vec![
        mock_resource(&server.uri(), "/same/1.csv", "report", "CSV"),
        mock_resource(&server.uri(), "/same/2.csv", "report", "CSV"),
        mock_resource(&server.uri(), "/same/3.csv", "report", "CSV"),
    ];

    let results = client
        .download_resources(&resources, Some(tmp.path()))
        .await;

    let paths: Vec<_> = results
        .into_iter()
        .map(|r| r.expect("all downloads must succeed"))
        .collect();

    // All three paths must be distinct, otherwise one download overwrote another.
    assert_eq!(paths.len(), 3);
    assert_ne!(paths[0], paths[1]);
    assert_ne!(paths[1], paths[2]);
    assert_ne!(paths[0], paths[2]);

    for (i, p) in paths.iter().enumerate() {
        assert!(p.exists(), "path {i} ({p:?}) must exist on disk");
    }
}

// ---------------------------------------------------------------------------
// Concurrency cap is enforced: a small max_concurrent_downloads with many
// slow responses must serialize into batches.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn download_resources_honors_max_concurrent_downloads_cap() {
    let server = MockServer::start().await;
    // Each response is delayed ~150ms. With max_concurrent=2 and 4 resources,
    // the wall-clock total must be at least ~300ms (two batches) — far above
    // the unlimited-parallel lower bound of ~150ms.
    let per_request_delay = Duration::from_millis(150);
    Mock::given(method("GET"))
        .and(path_regex(r"^/slow/.*"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"data".to_vec())
                .set_delay(per_request_delay),
        )
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = test_client(tmp.path().to_path_buf(), 2);

    let resources: Vec<Resource> = (0..4)
        .map(|i| {
            mock_resource(
                &server.uri(),
                &format!("/slow/{i}.csv"),
                &format!("res-{i}"),
                "CSV",
            )
        })
        .collect();

    let start = Instant::now();
    let results = client
        .download_resources(&resources, Some(tmp.path()))
        .await;
    let elapsed = start.elapsed();

    for r in &results {
        assert!(r.is_ok(), "every download must succeed: {r:?}");
    }

    // Lower bound: two sequential batches of 2 = ~2x the per-request delay,
    // minus a safety margin for scheduling. Use 1.5x to stay robust.
    let min_expected = per_request_delay.mul_f32(1.5);
    assert!(
        elapsed >= min_expected,
        "with max_concurrent=2 and 4 slow requests, elapsed must be >= {min_expected:?} \
         (proves concurrency is bounded), got {elapsed:?}"
    );

    // Upper bound: well under full serialization (4x delay). Allows generous
    // headroom for CI jitter while still catching a regression that forces
    // serial execution.
    let max_expected = per_request_delay.mul_f32(3.5);
    assert!(
        elapsed < max_expected,
        "elapsed must be < {max_expected:?} (proves downloads aren't fully serial), \
         got {elapsed:?}"
    );
}
