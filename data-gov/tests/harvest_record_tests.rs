//! #77: `DataGovClient::get_dataset_by_harvest_record` had zero callers,
//! zero tests, and zero examples anywhere in the workspace -- unproven
//! public surface, the exact class of item that issue is about.
//!
//! Kept rather than removed: the README and `.github/instructions/
//! target-info.instructions.md` both document it as the paired lookup for
//! harvest-record UUIDs, alongside `get_dataset(slug)`. Its underlying
//! `data_gov_catalog::CatalogClient::harvest_record_transformed` is already
//! covered by that crate's own tests; these prove the thin `data-gov`
//! wrapper delegates correctly and propagates both outcomes a caller sees.

use data_gov::{DataGovClient, DataGovConfig, DataGovError, OperatingMode};
use std::path::{Path, PathBuf};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const HARVEST_RECORD_ID: &str = "c1d2faad-b413-41a8-934d-119f7c50d8ab";

/// `data-gov-catalog`'s own captured-and-manifest-tracked fixture (see that
/// crate's `tests/fixtures/MANIFEST.json`). Reused rather than hand-written,
/// per CLAUDE.md's "prefer real captured data".
fn harvest_record_transformed_fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../data-gov-catalog/tests/fixtures/harvest_record_transformed.json")
}

fn test_client(base_url: &str) -> DataGovClient {
    let config = DataGovConfig::new()
        .with_base_url(base_url)
        .with_mode(OperatingMode::CommandLine);
    DataGovClient::with_config(config).expect("test client must build")
}

#[tokio::test]
async fn get_dataset_by_harvest_record_deserializes_the_transformed_dataset() {
    let server = MockServer::start().await;
    let body = std::fs::read_to_string(harvest_record_transformed_fixture_path())
        .expect("harvest_record_transformed.json fixture must exist");
    // The fixture is not pinned to a specific long-lived record (unlike the
    // slug-addressed fixtures), so its content changes on every recapture.
    // Read the title out of the raw body rather than hardcoding a value that
    // would go stale the next time scripts/capture-fixtures.sh runs.
    let raw: serde_json::Value = serde_json::from_str(&body).expect("fixture must be valid JSON");
    let expected_title = raw["title"]
        .as_str()
        .expect("fixture has no title; recapture before trusting this test");

    Mock::given(method("GET"))
        .and(path(format!(
            "/harvest_record/{HARVEST_RECORD_ID}/transformed"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body.clone(), "application/json"))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let dataset = client
        .get_dataset_by_harvest_record(HARVEST_RECORD_ID)
        .await
        .expect("a 200 response must deserialize into a Dataset");

    assert_eq!(
        dataset.title.as_deref(),
        Some(expected_title),
        "the wrapper must hand back the real field, not drop it in translation"
    );
}

/// #83: `/harvest_record/{id}/transformed` 404s when the base record's
/// `source_transform` is null, which is the common case, not an edge case --
/// roughly 87% of a 752-record sample across 18 organizations. The catalog
/// layer turns that into `Ok(None)` (see
/// `CatalogClient::harvest_record_transformed`); this wrapper keeps its
/// `Result<Dataset>` signature by mapping the missing transform onto
/// `ResourceNotFound`, the same shape `get_dataset` uses for its own
/// not-found case.
#[tokio::test]
async fn get_dataset_by_harvest_record_maps_a_missing_transform_to_resource_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/harvest_record/{HARVEST_RECORD_ID}/transformed"
        )))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let err = client
        .get_dataset_by_harvest_record(HARVEST_RECORD_ID)
        .await
        .expect_err("a missing transform must not be reported as a successful empty dataset");

    assert!(
        matches!(err, DataGovError::ResourceNotFound { .. }),
        "a 404 (no populated transform) must surface the same not-found shape \
         get_dataset uses for a missing slug, not the raw catalog error: got {err:?}"
    );
}
