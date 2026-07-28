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
/// crate's `tests/fixtures/MANIFEST.json`). It is recorded there as
/// `unverified`: the live endpoint 404s for every sampled record today
/// (#83), so this is the last capture from when it still answered. Reused
/// rather than hand-written, per CLAUDE.md's "prefer real captured data".
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

    Mock::given(method("GET"))
        .and(path(format!(
            "/harvest_record/{HARVEST_RECORD_ID}/transformed"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let dataset = client
        .get_dataset_by_harvest_record(HARVEST_RECORD_ID)
        .await
        .expect("a 200 response must deserialize into a Dataset");

    assert_eq!(
        dataset.title.as_deref(),
        Some(
            "TIGER/Line Shapefile, 2022, Nation, U.S., 2020 Census 5-Digit ZIP Code Tabulation Area (ZCTA5)"
        ),
        "the wrapper must hand back the real field, not drop it in translation"
    );
}

/// #83: the live endpoint currently 404s for every sampled record, so this
/// is the common case a real caller hits today, not an edge case.
#[tokio::test]
async fn get_dataset_by_harvest_record_propagates_a_not_found_as_an_error() {
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
        .expect_err("a 404 must not be reported as a successful empty dataset");

    assert!(
        matches!(err, DataGovError::CatalogError(_)),
        "a harvest-record lookup has no not-found case of its own to collapse into, \
         unlike dataset_by_slug -- it must surface the catalog error as-is, got {err:?}"
    );
}
