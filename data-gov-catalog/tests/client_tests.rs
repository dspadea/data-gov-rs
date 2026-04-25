//! Unit tests for [`data_gov_catalog::CatalogClient`] against a mock HTTP server.
//!
//! These tests never hit the network. Fixtures live in `tests/fixtures/` and
//! are trimmed captures of real responses.

use data_gov_catalog::{CatalogClient, CatalogError, Configuration, SearchParams};
use serde_json::json;
use std::sync::Arc;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> String {
    let path = format!("tests/fixtures/{name}");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {path} missing: {e}"))
}

fn client_for(server: &MockServer) -> CatalogClient {
    CatalogClient::new(Arc::new(Configuration {
        base_path: server.uri(),
        user_agent: Some("data-gov-catalog-tests/1.0".to_string()),
        client: reqwest::Client::new(),
    }))
}

#[tokio::test]
async fn search_builds_query_string_and_parses_results() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("q", "climate"))
        .and(query_param("per_page", "2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(fixture("search.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    let page = client
        .search(SearchParams::new().q("climate").per_page(2))
        .await
        .expect("search succeeds");

    assert!(!page.results.is_empty());
    let hit = &page.results[0];
    assert!(hit.title.is_some());
    assert!(hit.dcat.is_some(), "hit should carry a nested dcat record");
    assert!(
        page.after.is_some(),
        "non-empty page returns an `after` cursor"
    );
}

#[tokio::test]
async fn search_sends_repeated_keyword_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("keyword", "climate"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("search_filtered.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    let _ = client
        .search(SearchParams::new().keyword("climate").keyword("noaa"))
        .await
        .expect("search succeeds");
}

#[tokio::test]
async fn search_with_org_slug_passes_filter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("org_slug", "nasa"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("search_filtered.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    client
        .search(SearchParams::new().q("climate").org_slug("nasa"))
        .await
        .expect("filtered search succeeds");
}

#[tokio::test]
async fn dataset_by_slug_returns_first_hit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("slug", "crime-data-from-2020-to-present"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("search_by_slug.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    let hit = client
        .dataset_by_slug("crime-data-from-2020-to-present")
        .await
        .expect("slug lookup succeeds")
        .expect("slug matches a dataset");

    assert!(hit.title.is_some());
    assert!(hit.dcat.is_some());
}

#[tokio::test]
async fn dataset_by_slug_returns_none_when_empty() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [],
            "sort": "relevance"
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let result = client.dataset_by_slug("nonexistent").await.unwrap();
    assert!(result.is_none());
}

/// The live Catalog API silently ignores unmatched `slug=` values and
/// returns the top result by relevance instead of an empty page. Make
/// sure the client guards against that and returns `None` rather than
/// the wrong dataset.
#[tokio::test]
async fn dataset_by_slug_returns_none_when_top_hit_has_a_different_slug() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "slug": "crime-data-from-2020-to-present",
                "title": "Crime Data from 2020 to 2024"
            }],
            "sort": "relevance"
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let result = client.dataset_by_slug("nasa-thesaurus").await.unwrap();
    assert!(
        result.is_none(),
        "expected None when API returns a hit with a different slug, got Some(_)"
    );
}

#[tokio::test]
async fn organizations_parses_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/organizations"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("organizations.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    let orgs = client.organizations().await.expect("orgs succeed");
    assert!(orgs.total > 0);
    assert!(!orgs.organizations.is_empty());
    assert!(orgs.organizations[0].slug.is_some());
}

#[tokio::test]
async fn keywords_passes_size_and_min_count() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/keywords"))
        .and(query_param("size", "10"))
        .and(query_param("min_count", "5"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(fixture("keywords.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    let kw = client
        .keywords(Some(10), Some(5))
        .await
        .expect("keywords succeed");
    assert_eq!(kw.size, 10);
    assert!(!kw.keywords.is_empty());
    assert!(kw.keywords[0].count > 0);
}

#[tokio::test]
async fn locations_search_returns_suggestions() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/locations/search"))
        .and(query_param("q", "Colorado"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("locations_search.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    let locs = client
        .locations_search("Colorado", Some(3))
        .await
        .expect("locations succeed");
    assert!(!locs.locations.is_empty());
}

#[tokio::test]
async fn harvest_record_transformed_parses_dataset() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/harvest_record/c1d2faad-b413-41a8-934d-119f7c50d8ab/transformed",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            fixture("harvest_record_transformed.json"),
            "application/json",
        ))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let ds = client
        .harvest_record_transformed("c1d2faad-b413-41a8-934d-119f7c50d8ab")
        .await
        .expect("transformed record parses");
    assert!(ds.title.is_some());
    assert!(!ds.distribution.is_empty());
}

#[tokio::test]
async fn api_error_status_is_preserved() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(
            ResponseTemplate::new(503).set_body_string("{\"message\":\"Service Unavailable\"}"),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    let err = client.search(SearchParams::new().q("x")).await.unwrap_err();
    match err {
        CatalogError::ApiError { status, .. } => assert_eq!(status, 503),
        other => panic!("expected ApiError, got {other:?}"),
    }
}

#[tokio::test]
async fn parse_error_surfaces_bad_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/organizations"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let err = client.organizations().await.unwrap_err();
    assert!(matches!(err, CatalogError::ParseError(_)));
}
