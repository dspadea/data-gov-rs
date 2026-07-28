//! #77: `DataGovClient::catalog_client` had zero callers, zero tests, and
//! zero examples anywhere in the workspace.
//!
//! Kept rather than removed: `.github/instructions/target-info.
//! instructions.md` documents it as the escape hatch for advanced
//! `SearchParams` filters that `DataGovClient::search`'s four-argument
//! shortcut does not expose (org type, spatial filters, and the rest).
//! Searched by symbol name, by the `data_gov::catalog` re-export (the axis
//! that matters here, since a caller could reach `CatalogClient` that way
//! without ever calling this method), by manifest (no workspace member
//! depends on `data-gov-catalog` directly), and by git history: nothing in
//! the workspace calls it or reaches a `CatalogClient` another way. This
//! test proves the escape hatch actually delivers what the docs promise --
//! a filter `search` alone cannot express -- rather than, say, a client
//! wired to the wrong configuration.

use data_gov::catalog::SearchParams;
use data_gov::{DataGovClient, DataGovConfig};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn catalog_client_reaches_search_params_that_the_shortcut_does_not_expose() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("org_type", "Federal Government"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [],
            "sort": "relevance"
        })))
        .mount(&server)
        .await;

    let config = DataGovConfig::new().with_base_url(server.uri());
    let client = DataGovClient::with_config(config).expect("client must build");

    // `org_type` has no equivalent on `DataGovClient::search`'s four
    // arguments; reaching it at all proves the escape hatch works, not just
    // that it type-checks.
    let params = SearchParams::new().org_type("Federal Government");
    let response = client
        .catalog_client()
        .search(params)
        .await
        .expect("the advanced filter must reach the mocked endpoint");

    assert!(
        response.results.is_empty(),
        "the mock answers only the org_type-filtered query; a mismatched \
         request would 404 rather than return this empty page"
    );
}

#[tokio::test]
async fn catalog_client_shares_the_configured_base_url_with_the_shortcut_methods() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/organizations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "organizations": [],
            "total": 0
        })))
        .mount(&server)
        .await;

    let config = DataGovConfig::new().with_base_url(server.uri());
    let client = DataGovClient::with_config(config).expect("client must build");

    let via_escape_hatch = client.catalog_client().organizations().await;
    assert!(
        via_escape_hatch.is_ok(),
        "the escape hatch must be wired to the same configured base URL as \
         DataGovClient's own shortcuts, got {via_escape_hatch:?}"
    );
}
