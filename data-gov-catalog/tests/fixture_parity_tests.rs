//! Parity between the models and what the Catalog API actually returns.
//!
//! Every fixture here is a verbatim capture from the live API, refreshed with
//! `scripts/capture-fixtures.sh`. These tests deserialize each capture into the
//! model it is supposed to populate and assert the fields really arrive.
//!
//! This is deliberately different from `client_tests.rs`, which serves fixtures
//! through wiremock to exercise request shaping. Those tests confirm a request
//! was *sent* correctly; these confirm a response is *understood* correctly. A
//! field that silently deserializes to `None` because its serde name does not
//! match the wire name passes every wiremock test and still loses data.
//!
//! When one of these fails, the API is right and the model is wrong. Recapture
//! the fixtures, confirm the shape, then fix the model — never the reverse.

use data_gov_catalog::models;

fn fixture(name: &str) -> String {
    let path = format!("tests/fixtures/{name}");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {path} missing: {e}"))
}

fn parse<T: serde::de::DeserializeOwned>(name: &str) -> T {
    serde_json::from_str(&fixture(name))
        .unwrap_or_else(|e| panic!("fixture {name} does not deserialize into the model: {e}"))
}

#[test]
fn search_response_populates_core_hit_fields() {
    let response: models::SearchResponse = parse("search.json");
    assert!(!response.results.is_empty(), "capture has no results");

    for hit in &response.results {
        assert!(
            hit.slug.is_some(),
            "slug missing: it identifies the dataset"
        );
        assert!(hit.title.is_some(), "title missing");
        assert!(hit.identifier.is_some(), "identifier missing");
        assert!(hit.dcat.is_some(), "dcat record missing");
    }
}

#[test]
fn search_hit_carries_the_publishing_organization() {
    let response: models::SearchResponse = parse("search.json");
    let hit = response.results.first().expect("capture has no results");
    let org = hit
        .organization
        .as_ref()
        .expect("organization missing from search hit");

    // `org_slug` on SearchParams filters by exactly this value, so if it stops
    // arriving the CLI's organization navigation breaks silently.
    assert!(org.slug.is_some(), "organization slug missing");
    assert!(org.name.is_some(), "organization name missing");
}

#[test]
fn filtered_search_returns_only_the_requested_organization() {
    let response: models::SearchResponse = parse("search_filtered.json");
    assert!(!response.results.is_empty(), "capture has no results");

    // Captured with ?org_slug=nasa. This is the fixture-side half of proving the
    // filter is honoured; the live half lives in the ignored integration tests.
    for hit in &response.results {
        let slug = hit.organization.as_ref().and_then(|o| o.slug.as_deref());
        assert_eq!(slug, Some("nasa"), "org_slug filter returned a foreign org");
    }
}

#[test]
fn organizations_response_populates_slug_and_id() {
    let response: models::OrganizationsResponse = parse("organizations.json");
    assert!(!response.organizations.is_empty(), "no organizations");

    for org in &response.organizations {
        assert!(org.id.is_some(), "organization id missing");
        assert!(org.slug.is_some(), "organization slug missing");
        assert!(org.name.is_some(), "organization name missing");
    }
}

#[test]
fn keywords_response_populates_counts() {
    let response: models::KeywordsResponse = parse("keywords.json");
    assert!(!response.keywords.is_empty(), "no keywords");
    assert!(
        response.keywords.iter().all(|k| !k.keyword.is_empty()),
        "keyword text missing"
    );
}

#[test]
fn locations_response_populates_id_and_display_name() {
    let response: models::LocationsResponse = parse("locations_search.json");
    assert!(!response.locations.is_empty(), "no locations");
    assert!(
        response
            .locations
            .iter()
            .all(|l| !l.id.is_empty() && !l.display_name.is_empty()),
        "location id or display_name missing"
    );
}

#[test]
fn harvest_record_populates_identifiers() {
    let record: models::HarvestRecord = parse("harvest_record.json");
    assert!(record.id.is_some(), "harvest record id missing");
    assert!(record.identifier.is_some(), "identifier missing");
    assert!(record.harvest_source_id.is_some(), "source id missing");
}

#[test]
fn harvest_record_raw_is_a_dcat_dataset() {
    let raw: serde_json::Value = parse("harvest_record_raw.json");
    assert!(
        raw.get("title").is_some(),
        "raw harvest payload is not a DCAT dataset"
    );
}

/// Documents a live defect rather than guarding working behaviour.
///
/// The Catalog API sends the vCard contact name as `fn`; `ContactPoint::fn_`
/// has no `rename`, so it keys on the literal string `fn_` and every contact
/// name deserializes to `None`. The captured fixture contains
/// `"contactPoint": {"@type": "vcard:Contact", "fn": "CRDC Team", ...}`.
///
/// Un-ignore this when #61 lands; it is the acceptance test for that fix.
#[test]
#[ignore = "fails until #61: ContactPoint::fn_ is missing rename = \"fn\""]
fn contact_point_name_survives_deserialization() {
    // Raw side: what the API actually sent.
    let raw: serde_json::Value = parse("search.json");
    let raw_name = raw["results"][0]["dcat"]["contactPoint"]["fn"].as_str();
    assert!(
        raw_name.is_some(),
        "fixture has no contactPoint.fn; recapture before trusting this test"
    );

    // Typed side: what the model actually kept.
    let response: models::SearchResponse = parse("search.json");
    let contact = response
        .results
        .first()
        .and_then(|h| h.dcat.as_ref())
        .and_then(|d| d.contact_point.as_ref())
        .expect("capture has no contactPoint");

    assert_eq!(
        contact.fn_.as_deref(),
        raw_name,
        "contact name was dropped: the API sends `fn`, the model expects `fn_`"
    );
}
