//! Concurrency and partial-failure tests for
//! [`DataGovClient::download_distributions`].
//!
//! Each test constructs a `DataGovClient` pointed at a `wiremock` server and
//! supplies a slice of [`Distribution`] records whose URLs target the mock.
//! Tests assert on caller-observable behavior:
//!
//! - Return-vector length and ordering match the input
//! - Partial failures surface as per-distribution `Err` without short-circuiting
//! - Filenames for duplicate titles are disambiguated by index
//! - The `max_concurrent_downloads` limit is actually enforced

use std::time::{Duration, Instant};

use data_gov::catalog::models::Distribution;
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

/// Create a Distribution whose `downloadURL` points at the given mock path.
fn mock_distribution(mock_uri: &str, file_path: &str, title: &str, format: &str) -> Distribution {
    Distribution {
        type_hint: None,
        title: Some(title.to_string()),
        description: None,
        download_url: Some(format!("{mock_uri}{file_path}")),
        access_url: None,
        media_type: None,
        format: Some(format.to_string()),
        license: None,
        described_by: None,
        described_by_type: None,
    }
}

#[tokio::test]
async fn empty_slice_returns_empty_vec() {
    let tmp = TempDir::new().expect("tempdir");
    let client = test_client(tmp.path().to_path_buf(), 3);

    let results = client.download_distributions(&[], None).await;

    assert!(
        results.is_empty(),
        "empty input must produce empty output, got {} items",
        results.len()
    );
}

#[tokio::test]
async fn single_distribution_returns_single_result() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/files/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = test_client(tmp.path().to_path_buf(), 3);

    let distributions = vec![mock_distribution(
        &server.uri(),
        "/files/one.csv",
        "one",
        "CSV",
    )];
    let results = client
        .download_distributions(&distributions, Some(tmp.path()))
        .await;

    assert_eq!(results.len(), 1);
    let path = results.into_iter().next().unwrap().expect("should succeed");
    assert!(path.exists(), "downloaded file must exist at {path:?}");
}

#[tokio::test]
async fn returns_one_result_per_input_in_order() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/files/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"payload".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = test_client(tmp.path().to_path_buf(), 3);

    let distributions = vec![
        mock_distribution(&server.uri(), "/files/a.csv", "a", "CSV"),
        mock_distribution(&server.uri(), "/files/b.csv", "b", "CSV"),
        mock_distribution(&server.uri(), "/files/c.csv", "c", "CSV"),
    ];
    let results = client
        .download_distributions(&distributions, Some(tmp.path()))
        .await;

    assert_eq!(results.len(), 3);
    for (i, r) in results.iter().enumerate() {
        assert!(r.is_ok(), "distribution {i} should have succeeded: {r:?}");
    }
}

#[tokio::test]
async fn mixed_url_and_no_url_returns_mixed_results() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/files/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"payload".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = test_client(tmp.path().to_path_buf(), 3);

    let distributions = vec![
        mock_distribution(&server.uri(), "/files/ok1.csv", "ok1", "CSV"),
        Distribution {
            type_hint: None,
            title: Some("no-url".to_string()),
            description: None,
            download_url: None,
            access_url: None,
            media_type: None,
            format: Some("CSV".to_string()),
            license: None,
            described_by: None,
            described_by_type: None,
        },
        mock_distribution(&server.uri(), "/files/ok2.csv", "ok2", "CSV"),
    ];

    let results = client
        .download_distributions(&distributions, Some(tmp.path()))
        .await;

    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok(), "first distribution must succeed");
    match &results[1] {
        Err(DataGovError::ResourceNotFound { message }) => {
            assert!(
                message.contains("downloadURL"),
                "error message should explain missing URL, got: {message}"
            );
        }
        other => panic!("expected ResourceNotFound, got {other:?}"),
    }
    assert!(results[2].is_ok(), "third distribution must still succeed");
}

#[tokio::test]
async fn propagates_per_distribution_http_errors() {
    let server = MockServer::start().await;
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

    let distributions = vec![
        mock_distribution(&server.uri(), "/good/a.csv", "a", "CSV"),
        mock_distribution(&server.uri(), "/bad/b.csv", "b", "CSV"),
        mock_distribution(&server.uri(), "/good/c.csv", "c", "CSV"),
    ];

    let results = client
        .download_distributions(&distributions, Some(tmp.path()))
        .await;

    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    match &results[1] {
        Err(DataGovError::DownloadError { message }) => {
            assert!(
                message.contains("500"),
                "error must mention HTTP status, got: {message}"
            );
        }
        other => panic!("expected DownloadError with 500, got {other:?}"),
    }
    assert!(results[2].is_ok());
}

#[tokio::test]
async fn disambiguates_duplicate_filenames_by_index() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/same/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"content".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = test_client(tmp.path().to_path_buf(), 3);

    let distributions = vec![
        mock_distribution(&server.uri(), "/same/1.csv", "report", "CSV"),
        mock_distribution(&server.uri(), "/same/2.csv", "report", "CSV"),
        mock_distribution(&server.uri(), "/same/3.csv", "report", "CSV"),
    ];

    let results = client
        .download_distributions(&distributions, Some(tmp.path()))
        .await;

    let paths: Vec<_> = results
        .into_iter()
        .map(|r| r.expect("all downloads must succeed"))
        .collect();

    assert_eq!(paths.len(), 3);
    assert_ne!(paths[0], paths[1]);
    assert_ne!(paths[1], paths[2]);
    assert_ne!(paths[0], paths[2]);

    for (i, p) in paths.iter().enumerate() {
        assert!(p.exists(), "path {i} ({p:?}) must exist on disk");
    }
}

#[tokio::test]
async fn honors_max_concurrent_downloads_cap() {
    let server = MockServer::start().await;
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

    let distributions: Vec<Distribution> = (0..4)
        .map(|i| {
            mock_distribution(
                &server.uri(),
                &format!("/slow/{i}.csv"),
                &format!("res-{i}"),
                "CSV",
            )
        })
        .collect();

    let start = Instant::now();
    let results = client
        .download_distributions(&distributions, Some(tmp.path()))
        .await;
    let elapsed = start.elapsed();

    for r in &results {
        assert!(r.is_ok(), "every download must succeed: {r:?}");
    }

    let min_expected = per_request_delay.mul_f32(1.5);
    assert!(
        elapsed >= min_expected,
        "with max_concurrent=2 and 4 slow requests, elapsed must be >= {min_expected:?} \
         (proves concurrency is bounded), got {elapsed:?}"
    );

    let max_expected = per_request_delay.mul_f32(3.5);
    assert!(
        elapsed < max_expected,
        "elapsed must be < {max_expected:?} (proves downloads aren't fully serial), \
         got {elapsed:?}"
    );
}
