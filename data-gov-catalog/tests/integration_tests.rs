//! Live integration tests for [`data_gov_catalog`] against the real API.
//!
//! These tests hit `https://catalog.data.gov` and will fail if the network is
//! unavailable or the service is degraded. They are gated behind `--ignored`
//! so the default `cargo test` run stays hermetic.
//!
//! ```bash
//! cargo test -p data-gov-catalog --test integration_tests -- --ignored
//! ```

use data_gov_catalog::{CatalogClient, Configuration, SearchParams};
use std::sync::Arc;

fn live_client() -> CatalogClient {
    CatalogClient::new(Arc::new(Configuration::default()))
}

#[tokio::test]
#[ignore]
async fn live_search_returns_results() {
    let client = live_client();
    let page = client
        .search(SearchParams::new().q("climate").per_page(3))
        .await
        .expect("live search succeeds");
    assert!(!page.results.is_empty(), "expected at least one result");
    let hit = &page.results[0];
    assert!(hit.title.is_some());
    assert!(hit.slug.is_some());
}

#[tokio::test]
#[ignore]
async fn live_organizations_has_federal_entries() {
    let client = live_client();
    let orgs = client.organizations().await.expect("orgs succeed");
    assert!(orgs.total > 0);
    assert!(
        orgs.organizations
            .iter()
            .any(|o| { matches!(o.organization_type.as_deref(), Some("Federal Government")) })
    );
}

#[tokio::test]
#[ignore]
async fn live_keywords_returns_counts() {
    let client = live_client();
    let kw = client
        .keywords(Some(5), None)
        .await
        .expect("keywords succeed");
    assert!(!kw.keywords.is_empty());
    assert!(kw.keywords.iter().all(|k| k.count > 0));
}

#[tokio::test]
#[ignore]
async fn live_pagination_advances_with_after_cursor() {
    let client = live_client();
    let first = client
        .search(SearchParams::new().q("census").per_page(2))
        .await
        .expect("page 1");
    let after = first.after.clone().expect("first page has a cursor");
    let second = client
        .search(SearchParams::new().q("census").per_page(2).after(after))
        .await
        .expect("page 2");
    assert!(!second.results.is_empty());
    let first_ids: Vec<_> = first
        .results
        .iter()
        .filter_map(|h| h.slug.as_ref())
        .collect();
    let second_ids: Vec<_> = second
        .results
        .iter()
        .filter_map(|h| h.slug.as_ref())
        .collect();
    assert!(
        first_ids.iter().all(|id| !second_ids.contains(id)),
        "pages should not overlap"
    );
}

/// Live contract test: a slug that exists resolves, including slugs that
/// full-text search cannot recall.
///
/// Each case below is a real data.gov slug that the previous `q=<slug>`
/// implementation returned `None` for, chosen to span three distinct causes:
/// 90-character mid-word truncation, punctuation collapse during slugification,
/// and simple rank overflow past the page cutoff. Measured 1/6 before the fix,
/// 6/6 after.
///
/// Network-bound, so `#[ignore]`d and run by the opt-in Live API Tests job.
#[tokio::test]
#[ignore = "hits the live data.gov Catalog API"]
async fn live_dataset_by_slug_resolves_slugs_full_text_search_cannot_recall() {
    let client = CatalogClient::new(Arc::new(Configuration::default()));

    for (cause, slug) in [
        (
            "90-char truncation mid-word",
            "advancing-the-automation-of-plant-nucleic-acid-extraction-for-rapid-diagnosis-of-plant-dis",
        ),
        (
            "90-char truncation mid-word",
            "artemis-p2-ephemeris-heliocentric-trajectories-heliographic-heliographic-inertial-and-sola",
        ),
        ("punctuation collapse (Drugs@FDA)", "drugsfda-database"),
        ("rank overflow past the cutoff", "horizons"),
        ("rank overflow past the cutoff", "water-quality-data"),
        ("plain slug, control", "crime-data-from-2020-to-present"),
    ] {
        let hit = client
            .dataset_by_slug(slug)
            .await
            .unwrap_or_else(|e| panic!("{cause}: lookup errored for {slug}: {e}"))
            .unwrap_or_else(|| panic!("{cause}: {slug} exists on data.gov but did not resolve"));

        assert_eq!(
            hit.slug.as_deref(),
            Some(slug),
            "{cause}: resolved a different dataset than requested"
        );
    }
}

/// A slug that does not exist must be `Ok(None)`, and the endpoint must not
/// prefix-match: `nasa-pat` is a prefix of real slugs and must still miss.
#[tokio::test]
#[ignore = "hits the live data.gov Catalog API"]
async fn live_dataset_by_slug_does_not_prefix_match() {
    let client = CatalogClient::new(Arc::new(Configuration::default()));

    for absent in ["nasa-pat", "no-such-dataset-anywhere-12345"] {
        let result = client
            .dataset_by_slug(absent)
            .await
            .unwrap_or_else(|e| panic!("{absent}: lookup errored: {e}"));
        assert!(
            result.is_none(),
            "{absent} does not exist; got {:?}",
            result.and_then(|h| h.slug)
        );
    }
}
