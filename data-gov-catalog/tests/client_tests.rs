//! Unit tests for [`data_gov_catalog::CatalogClient`] against a mock HTTP server.
//!
//! These tests never hit the network. Fixtures live in `tests/fixtures/` and
//! are trimmed captures of real responses.

use data_gov_catalog::{CatalogClient, CatalogError, Configuration, SearchParams};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
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
async fn dataset_by_slug_returns_the_dataset() {
    let slug = "crime-data-from-2020-to-present";
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/dataset/{slug}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("dataset_by_slug.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let hit = client_for(&server)
        .dataset_by_slug(slug)
        .await
        .expect("slug lookup succeeds")
        .expect("slug matches a dataset");

    assert_eq!(hit.slug.as_deref(), Some(slug));
    assert!(hit.title.is_some());
    assert!(hit.dcat.is_some(), "the full DCAT record must come through");
}

/// The endpoint is exact, but if it ever answered with a different dataset we
/// must not hand it back: returning a plausible wrong dataset is worse than
/// returning nothing, because the caller cannot tell.
#[tokio::test]
async fn dataset_by_slug_returns_none_when_the_hit_has_a_different_slug() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/dataset/nasa-thesaurus"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total": 1,
            "results": [{
                "slug": "crime-data-from-2020-to-present",
                "title": "A completely different dataset"
            }]
        })))
        .mount(&server)
        .await;

    let result = client_for(&server)
        .dataset_by_slug("nasa-thesaurus")
        .await
        .expect("lookup succeeds");
    assert!(
        result.is_none(),
        "a hit whose slug differs from the request must not be returned, got {result:?}"
    );
}

/// A 200 carrying no results is "no such dataset", the same as a 404.
#[tokio::test]
async fn dataset_by_slug_returns_none_when_the_response_is_empty() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/dataset/nonexistent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"total": 0, "results": []})))
        .mount(&server)
        .await;

    assert!(
        client_for(&server)
            .dataset_by_slug("nonexistent")
            .await
            .expect("lookup succeeds")
            .is_none()
    );
}

/// The slug goes into a URL *path segment*, so a hostile value must not be able
/// to steer the request elsewhere. `Url` normalisation resolves `..` after
/// percent-decoding, so an unencoded slug could reach a different endpoint
/// entirely.
///
/// Asserted by mounting the *only* legitimate path and requiring the request to
/// land on it: any escape produces a different path, no mock matches, and the
/// call fails.
#[tokio::test]
async fn dataset_by_slug_percent_encodes_hostile_slugs_into_one_path_segment() {
    for hostile in [
        "../search",
        "..%2Fsearch",
        "a/b",
        "with space",
        "quote\"inside",
        "sem;colon",
        "q?uery=1",
        "frag#ment",
    ] {
        let server = MockServer::start().await;
        // Matches any path; we assert on what was actually requested.
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"total": 0, "results": []})),
            )
            .mount(&server)
            .await;

        let _ = client_for(&server).dataset_by_slug(hostile).await;

        let requests = server.received_requests().await.expect("recorded requests");
        assert_eq!(requests.len(), 1, "{hostile:?}: exactly one request");
        let path = requests[0].url.path();

        assert!(
            path.starts_with("/api/dataset/"),
            "{hostile:?} escaped the endpoint: requested {path}"
        );
        assert_eq!(
            path.matches('/').count(),
            3,
            "{hostile:?} must occupy exactly one path segment, got {path}"
        );
        assert!(
            requests[0].url.query().is_none(),
            "{hostile:?} must not introduce a query string: {}",
            requests[0].url
        );
    }
}

/// A single hostile id, probed against one endpoint template.
///
/// Shared by the four `#71.1` tests below: `harvest_record`,
/// `harvest_record_raw`, `harvest_record_transformed`, and
/// `location_geometry` all still built their path with a bare
/// `format!("/prefix/{id}/suffix")`. `Url::parse` removes dot-segments
/// *after* percent-decoding, so an unencoded `..` or `%2e` can redirect the
/// GET to a different path, and a bare `#` or `?` strands the rest of the
/// value in the fragment or query instead of the path -- which is the
/// silent-success case: an unrelated JSON object then deserializes into an
/// all-`None` model with no error at all.
struct HostilePathCase {
    /// The id supplied to the client method.
    hostile: &'static str,
    /// What the resulting request path must start with.
    prefix: &'static str,
    /// How many `/` characters the full path must contain: proof the hostile
    /// id landed in exactly one path segment rather than adding or removing
    /// segments.
    slash_count: usize,
}

const HOSTILE_IDS: [&str; 8] = [
    "../search",
    "..%2Fsearch",
    "a/b",
    "with space",
    "quote\"inside",
    "sem;colon",
    "q?uery=1",
    "frag#ment",
];

/// Mount a catch-all 200 responder, issue one request, and assert it landed
/// on exactly one path segment under `case.prefix`.
async fn assert_hostile_id_stays_in_one_segment(
    case: &HostilePathCase,
    body: serde_json::Value,
    call: impl AsyncFnOnce(&CatalogClient, &str) -> (),
) {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    call(&client_for(&server), case.hostile).await;

    let requests = server.received_requests().await.expect("recorded requests");
    assert_eq!(requests.len(), 1, "{:?}: exactly one request", case.hostile);
    let path = requests[0].url.path();

    assert!(
        path.starts_with(case.prefix),
        "{:?} escaped the endpoint: requested {path}",
        case.hostile
    );
    assert_eq!(
        path.matches('/').count(),
        case.slash_count,
        "{:?} must occupy exactly one path segment, got {path}",
        case.hostile
    );
    assert!(
        requests[0].url.query().is_none(),
        "{:?} must not introduce a query string: {}",
        case.hostile,
        requests[0].url
    );
}

#[tokio::test]
async fn harvest_record_percent_encodes_hostile_ids_into_one_path_segment() {
    for hostile in HOSTILE_IDS {
        let case = HostilePathCase {
            hostile,
            prefix: "/harvest_record/",
            slash_count: 2,
        };
        assert_hostile_id_stays_in_one_segment(&case, json!({}), async |client, id| {
            let _ = client.harvest_record(id).await;
        })
        .await;
    }
}

#[tokio::test]
async fn harvest_record_raw_percent_encodes_hostile_ids_into_one_path_segment() {
    for hostile in HOSTILE_IDS {
        let case = HostilePathCase {
            hostile,
            prefix: "/harvest_record/",
            slash_count: 3,
        };
        assert_hostile_id_stays_in_one_segment(&case, json!({}), async |client, id| {
            let _ = client.harvest_record_raw(id).await;
        })
        .await;
    }
}

#[tokio::test]
async fn harvest_record_transformed_percent_encodes_hostile_ids_into_one_path_segment() {
    for hostile in HOSTILE_IDS {
        let case = HostilePathCase {
            hostile,
            prefix: "/harvest_record/",
            slash_count: 3,
        };
        assert_hostile_id_stays_in_one_segment(&case, json!({}), async |client, id| {
            let _ = client.harvest_record_transformed(id).await;
        })
        .await;
    }
}

#[tokio::test]
async fn location_geometry_percent_encodes_hostile_ids_into_one_path_segment() {
    for hostile in HOSTILE_IDS {
        let case = HostilePathCase {
            hostile,
            prefix: "/api/location/",
            slash_count: 3,
        };
        assert_hostile_id_stays_in_one_segment(&case, json!({}), async |client, id| {
            let _ = client.location_geometry(id).await;
        })
        .await;
    }
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

/// The Catalog API returns `null` for empty repeated DCAT-US 3 fields
/// (observed: `references`, `rights`, also seen elsewhere on `keyword`,
/// `theme`, etc.). With plain `#[serde(default)]` only the *missing*
/// case is covered; an explicit `null` value blows up with
/// `invalid type: null, expected a sequence`. Make sure every `Vec<T>`
/// field tolerates `null` and treats it as empty.
#[tokio::test]
async fn search_tolerates_null_for_repeated_fields() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "slug": "with-nulls",
                "title": "Has nulls everywhere a Vec is expected",
                "keyword": null,
                "theme": null,
                "distribution_titles": null,
                "dcat": {
                    "@type": "dcat:Dataset",
                    "title": "Inner",
                    "keyword": null,
                    "theme": null,
                    "distribution": null,
                    "language": null,
                    "references": null
                }
            }],
            "sort": "relevance"
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let page = client
        .search(SearchParams::new().q("anything"))
        .await
        .expect("null Vec fields should not panic on parse");
    assert_eq!(page.results.len(), 1);
    let hit = &page.results[0];
    assert_eq!(hit.slug.as_deref(), Some("with-nulls"));
    assert!(hit.keyword.is_empty());
    assert!(hit.theme.is_empty());
    assert!(hit.distribution_titles.is_empty());
    let dcat = hit.dcat.as_ref().expect("dcat present");
    assert!(dcat.keyword.is_empty());
    assert!(dcat.theme.is_empty());
    assert!(dcat.distribution.is_empty());
    assert!(dcat.language.is_empty());
    assert!(dcat.references.is_empty());
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

/// The contract nothing in this suite previously pinned: **a slug that exists
/// resolves**, regardless of whether full-text search recalls it.
///
/// The shipped implementation queried `q=<slug>` and scanned the page. A slug
/// is a lossy derivation of the title — truncated at 90 characters mid-word,
/// punctuation collapsed, `U.S.` flattened to `u-s` — so its tokens are
/// frequently absent from the indexed text and the query returns nothing.
/// Measured against live data.gov: 15% of datasets on a uniform sample, 27%
/// past cursor depth 400.
///
/// This test makes `/search` return zero hits, which is exactly what the live
/// API does for those slugs. Any implementation that resolves slugs by
/// full-text search fails here; only one that calls the documented exact-lookup
/// endpoint passes.
#[tokio::test]
async fn dataset_by_slug_resolves_a_slug_that_full_text_search_cannot_recall() {
    let slug = "advancing-the-automation-of-plant-nucleic-acid-extraction-for-rapid-diagnosis-of-plant-dis";
    let server = MockServer::start().await;

    // Zero recall, as the live API genuinely returns for this slug: the final
    // token `dis` is a fragment of "diseases" and appears nowhere in the text.
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [], "sort": "relevance"
        })))
        .mount(&server)
        .await;

    // The documented exact-lookup endpoint, captured verbatim from live.
    Mock::given(method("GET"))
        .and(path(format!("/api/dataset/{slug}")))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            fixture("dataset_by_slug_truncated.json"),
            "application/json",
        ))
        .mount(&server)
        .await;

    let hit = client_for(&server)
        .dataset_by_slug(slug)
        .await
        .expect("slug lookup succeeds")
        .expect("the dataset exists, so it must resolve");

    assert_eq!(
        hit.slug.as_deref(),
        Some(slug),
        "must return the exact slug"
    );
    assert!(hit.title.is_some(), "the resolved hit must carry its title");
}

/// A slug that genuinely does not exist must be `Ok(None)`, not an error and
/// not a false positive. The endpoint answers 404 with `{"total":0,...}`, and
/// it does no prefix or substring matching: `nasa-pat` and `nasa-patents-extra`
/// both 404 against live.
#[tokio::test]
async fn dataset_by_slug_returns_none_for_a_slug_that_does_not_exist() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/dataset/no-such-dataset-anywhere-12345"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "detail": {}, "message": "Not Found"
        })))
        .mount(&server)
        .await;

    let outcome = client_for(&server)
        .dataset_by_slug("no-such-dataset-anywhere-12345")
        .await;

    assert!(
        matches!(outcome, Ok(None)),
        "a missing dataset is Ok(None), not an error: got {outcome:?}"
    );
}

/// A 404 means "no such dataset"; every other failure is an error. Reporting a
/// data.gov outage as a missing dataset is how `cd` came to print
/// "dataset not found" when the network was simply down.
#[tokio::test]
async fn dataset_by_slug_distinguishes_a_server_error_from_a_missing_dataset() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/dataset/some-slug"))
        .respond_with(ResponseTemplate::new(503).set_body_string("upstream unavailable"))
        .mount(&server)
        .await;

    match client_for(&server).dataset_by_slug("some-slug").await {
        Err(CatalogError::ApiError { status, .. }) => assert_eq!(status, 503),
        other => panic!("503 must surface as an ApiError, not as a missing dataset: {other:?}"),
    }
}

/// #48: a host that accepts the connection but never finishes the response
/// must not hang the caller forever. `Configuration::with_timeouts` lets a
/// caller pick a short bound; wiremock's delay (5s) is far longer than the
/// configured timeout (100ms), so a client that honours the timeout returns
/// promptly and a client that does not (a bare `reqwest::Client::new()`)
/// would still be waiting when this test's own harness gives up.
#[tokio::test]
async fn a_short_configured_timeout_bounds_a_stalled_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;

    let mut config =
        Configuration::with_timeouts(Duration::from_millis(50), Duration::from_millis(100));
    config.base_path = server.uri();
    let client = CatalogClient::new(Arc::new(config));

    let start = Instant::now();
    let result = client.search(SearchParams::new().q("x")).await;
    let elapsed = start.elapsed();

    assert!(
        matches!(result, Err(CatalogError::RequestError(_))),
        "an unresponsive server must produce a RequestError, not hang or succeed: got {result:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "took {elapsed:?} against a 100ms configured timeout and a 5s server delay; \
         the timeout is not being applied to the client"
    );
}
