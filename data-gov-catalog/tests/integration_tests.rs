//! Live integration tests for [`data_gov_catalog`] against the real API.
//!
//! These tests hit `https://catalog.data.gov` and will fail if the network is
//! unavailable or the service is degraded. They are gated behind `--ignored`
//! so the default `cargo test` run stays hermetic.
//!
//! ```bash
//! cargo test -p data-gov-catalog --test integration_tests -- --ignored
//! ```

use data_gov_catalog::{CatalogClient, Configuration, SearchParams, SortOrder, SpatialFilter};
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

/// #77: `org_type` had never been probed against the live API. `"State
/// Government"` is sourced from a live `/api/organizations` response (not
/// invented -- CLAUDE.md's own worked example is `org_slug=noaa-gov`
/// looking broken because it was guessed rather than sourced).
#[tokio::test]
#[ignore = "hits the live data.gov Catalog API"]
async fn live_org_type_filters_to_the_requested_type() {
    let client = live_client();
    let page = client
        .search(
            SearchParams::new()
                .org_type("State Government")
                .per_page(10),
        )
        .await
        .expect("org_type-filtered search succeeds");

    assert!(!page.results.is_empty(), "expected at least one result");
    for hit in &page.results {
        let org_type = hit
            .organization
            .as_ref()
            .and_then(|o| o.organization_type.as_deref());
        assert_eq!(
            org_type,
            Some("State Government"),
            "org_type=State Government returned a foreign organization type: {:?} ({:?})",
            org_type,
            hit.slug
        );
    }
}

/// `SearchParams::sort` was a bare `String`; the client-side probe that
/// motivated `SortOrder` (see its doc comment) showed a typo silently
/// falling back to relevance ranking with HTTP 200. This is the live half:
/// two orders that differ from the relevance baseline must also differ from
/// each other, and -- to rule out ranking noise rather than a real,
/// order-dependent sort -- each order's result set must be identical across
/// two separate calls before the cross-order comparison means anything.
#[tokio::test]
#[ignore = "hits the live data.gov Catalog API"]
async fn live_sort_orders_produce_different_stable_result_sets() {
    let client = live_client();

    async fn slugs_for(client: &CatalogClient, sort: SortOrder) -> Vec<String> {
        let page = client
            .search(SearchParams::new().q("climate").sort(sort).per_page(5))
            .await
            .unwrap_or_else(|e| panic!("sort={sort:?} search succeeds: {e}"));
        page.results.into_iter().filter_map(|h| h.slug).collect()
    }

    let popularity_first = slugs_for(&client, SortOrder::Popularity).await;
    let popularity_second = slugs_for(&client, SortOrder::Popularity).await;
    assert_eq!(
        popularity_first, popularity_second,
        "sort=popularity returned a different order on a repeated, identical call -- \
         this is ranking noise, not a stable sort, and invalidates the comparison below"
    );
    assert!(
        !popularity_first.is_empty(),
        "expected at least one result for sort=popularity"
    );

    let recency_first = slugs_for(&client, SortOrder::LastHarvestedDate).await;
    let recency_second = slugs_for(&client, SortOrder::LastHarvestedDate).await;
    assert_eq!(
        recency_first, recency_second,
        "sort=last_harvested_date returned a different order on a repeated, identical call -- \
         this is ranking noise, not a stable sort, and invalidates the comparison below"
    );
    assert!(
        !recency_first.is_empty(),
        "expected at least one result for sort=last_harvested_date"
    );

    assert_ne!(
        popularity_first, recency_first,
        "sort=popularity and sort=last_harvested_date returned identical, stable result sets; \
         sort may be silently ignored"
    );
}

/// #77: `spatial_filter` was probed with an *invalid* value in the issue
/// that flagged it as a phantom, which is exactly the mistake CLAUDE.md
/// calls out ("spatial_filter is a phantom filter that does nothing" turned
/// out to mean "was probed with an invalid value"). With valid values it
/// filters on `has_spatial` in both directions.
#[tokio::test]
#[ignore = "hits the live data.gov Catalog API"]
async fn live_spatial_filter_matches_the_has_spatial_flag() {
    let client = live_client();

    let geospatial = client
        .search(
            SearchParams::new()
                .spatial_filter(SpatialFilter::Geospatial)
                .per_page(10),
        )
        .await
        .expect("geospatial-filtered search succeeds");
    assert!(!geospatial.results.is_empty());
    for hit in &geospatial.results {
        assert_eq!(
            hit.has_spatial,
            Some(true),
            "spatial_filter=geospatial returned a non-spatial dataset: {:?}",
            hit.slug
        );
    }

    let non_geospatial = client
        .search(
            SearchParams::new()
                .spatial_filter(SpatialFilter::NonGeospatial)
                .per_page(10),
        )
        .await
        .expect("non-geospatial-filtered search succeeds");
    assert!(!non_geospatial.results.is_empty());
    for hit in &non_geospatial.results {
        assert_eq!(
            hit.has_spatial,
            Some(false),
            "spatial_filter=non-geospatial returned a spatial dataset: {:?}",
            hit.slug
        );
    }
}

/// #77: `spatial_within` set alone has no observable effect (a separate,
/// deliberately unasserted probe: `spatial_within=true` with no
/// `spatial_geometry` returns the unfiltered baseline). This test is the
/// live half of the claim that it is a real modifier of `spatial_geometry`
/// rather than a phantom -- the finding "no observable effect" was correct
/// but incomplete, since the original probe never tried it alongside the
/// geometry it modifies.
///
/// The query geometry is a 1-degree box over Antarctica, chosen because no
/// US federal, state, or local dataset's coverage sits there, so any hit
/// under `within=false` has to come from a genuinely global-scope dataset
/// (e.g. an ocean or climate reanalysis product) whose shape intersects
/// everywhere -- not from anything specific to this geometry.
#[tokio::test]
#[ignore = "hits the live data.gov Catalog API"]
async fn live_spatial_within_changes_results_alongside_geometry() {
    let client = live_client();
    let antarctic_box = serde_json::json!({
        "type": "Polygon",
        "coordinates": [[[10.0, -85.0], [11.0, -85.0], [11.0, -84.0], [10.0, -84.0], [10.0, -85.0]]]
    });

    let contained = client
        .search(
            SearchParams::new()
                .spatial_filter(SpatialFilter::Geospatial)
                .spatial_geometry(antarctic_box.clone())
                .spatial_within(true)
                .per_page(10),
        )
        .await
        .expect("within=true search succeeds");
    assert!(
        contained.results.is_empty(),
        "expected no dataset shape to be fully contained by a 1-degree Antarctic box, got {:?}",
        contained
            .results
            .iter()
            .filter_map(|h| h.slug.as_deref())
            .collect::<Vec<_>>()
    );

    let intersecting = client
        .search(
            SearchParams::new()
                .spatial_filter(SpatialFilter::Geospatial)
                .spatial_geometry(antarctic_box)
                .spatial_within(false)
                .per_page(10),
        )
        .await
        .expect("within=false search succeeds");
    assert!(
        !intersecting.results.is_empty(),
        "expected globally-scoped datasets to intersect an Antarctic box under within=false"
    );
}

/// #77: `location_geometry` had never been called against the live API.
/// Location id `5` is California, sourced from a live
/// `/api/locations/search?q=california` response (`locations_search.json`),
/// and has been stable across the fixture-capture history in this repo.
#[tokio::test]
#[ignore = "hits the live data.gov Catalog API"]
async fn live_location_geometry_returns_californias_boundary() {
    let client = live_client();
    let geometry = client
        .location_geometry("5")
        .await
        .expect("location_geometry succeeds");

    let shape = geometry
        .get("geometry")
        .and_then(|v| v.as_str())
        .expect("expected a `geometry` string field");
    assert!(
        shape.contains("Polygon"),
        "expected a GeoJSON polygon shape, got {shape}"
    );
    assert_eq!(geometry.get("id").and_then(|v| v.as_str()), Some("5"));
}
