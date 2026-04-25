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
