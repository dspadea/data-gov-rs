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

/// #83: this fixture used to be a permanent SKIP, because an unfiltered
/// `/search?per_page=1` (what most of this file's other harvest fixtures come
/// from) lands on a record with no populated transform roughly 6 times out of
/// 7. `scripts/capture-fixtures.sh` now sources this one from `org_slug=census`
/// instead, which the #83 investigation found populates a transform on every
/// sampled record, so this proves the capture is a real 200 with a real
/// `Dataset`, not the stale pre-#83 file.
#[test]
fn harvest_record_transformed_is_a_dcat_dataset() {
    let entry = &manifest()["fixtures"]["harvest_record_transformed.json"];
    assert_eq!(
        entry["status"].as_u64(),
        Some(200),
        "harvest_record_transformed.json must be captured from a real 200, \
         not carried over from before #83 was resolved"
    );

    let dataset: models::Dataset = parse("harvest_record_transformed.json");
    assert!(dataset.title.is_some(), "transformed record title missing");
    assert!(
        !dataset.distribution.is_empty(),
        "transformed record has no distributions"
    );
}

/// #83: the companion negative case. Most harvest records have no populated
/// transform, and the endpoint answers that with a 404 whose body is
/// `{"error": "Not Found"}` -- a different shape from `dataset_not_found.json`,
/// which is a well-formed, empty `SearchResponse`. This is what proves
/// `harvest_record_transformed` can tell "no transform for this record" from
/// "the API is down".
#[test]
fn the_harvest_transform_not_found_capture_is_a_real_404_body() {
    let entry = &manifest()["fixtures"]["harvest_record_transformed_not_found.json"];
    assert_eq!(
        entry["status"].as_u64(),
        Some(404),
        "harvest_record_transformed_not_found.json must be captured from a real 404"
    );

    let body: serde_json::Value = parse("harvest_record_transformed_not_found.json");
    assert!(
        body.get("error").is_some(),
        "a 404 body should carry an error field; recapture before trusting this test"
    );
}

/// Guards against a regression of #61: the Catalog API sends the vCard
/// contact name as `fn`, and `ContactPoint::fn_` must carry `rename = "fn"`
/// so the name survives both directions. Losing the rename again would key
/// deserialization on the literal string `fn_` and drop every contact name
/// silently. The captured fixture contains
/// `"contactPoint": {"@type": "vcard:Contact", "fn": "CRDC Team", ...}`.
#[test]
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

    // Round-trip side: re-serializing must emit the wire name `fn`, not the
    // schema-invalid `fn_`. A rename that only worked for deserialization
    // (e.g. `#[serde(alias = "fn")]` instead of `rename`) would pass the
    // assertion above and still fail this one.
    let serialized = serde_json::to_value(contact).expect("ContactPoint serializes");
    assert_eq!(
        serialized.get("fn").and_then(|v| v.as_str()),
        raw_name,
        "re-serialized contactPoint must carry the name under the wire key `fn`"
    );
    assert!(
        serialized.get("fn_").is_none(),
        "re-serialized contactPoint must not emit the schema-invalid key `fn_`, got {serialized}"
    );
}

/// Why [`data_gov_catalog::CatalogClient::dataset_by_slug`] resolves slugs
/// only, though the endpoint behind it is `/api/dataset/{slug_or_id}`.
///
/// The endpoint does accept an OpenSearch document id -- the third element of
/// a `/search` hit's `_sort` array -- and answers 200 with the right dataset.
/// This capture is that answer. What it does not contain is the id that was
/// asked for: `_sort` arrives null on this endpoint, and the id appears
/// nowhere else in the body. Nothing in the response can therefore show the
/// dataset is the one the caller named, so the wrapper returns `None` rather
/// than a hit it cannot attribute to the request.
///
/// When this test fails because `_sort` now arrives populated, that reason has
/// expired: compare the requested value against `sort_key[2]` and accept the
/// hit when they match, alongside the existing slug comparison.
#[test]
fn exact_lookup_by_document_id_returns_the_dataset_without_echoing_the_id() {
    let name = "dataset_by_document_id.json";
    let endpoint = manifest()["fixtures"][name]["endpoint"]
        .as_str()
        .unwrap_or_else(|| panic!("MANIFEST.json records no endpoint for {name}"))
        .to_owned();
    let document_id = endpoint
        .rsplit('/')
        .next()
        .expect("an endpoint path has at least one segment");

    let response: models::SearchResponse = parse(name);
    let hit = response
        .results
        .first()
        .expect("the capture must carry the dataset the id resolved to");

    assert_ne!(
        hit.slug.as_deref(),
        Some(document_id),
        "the capture must be a lookup by document id, not by slug"
    );
    assert!(
        hit.dcat.is_some(),
        "the server really did answer with the dataset, so what the wrapper \
         drops is a correct record, not an empty one"
    );
    assert!(
        hit.sort_key.is_none(),
        "`_sort` now arrives on the exact-lookup endpoint: {:?}. The document \
         id can be verified from the response, so dataset_by_slug can accept \
         one.",
        hit.sort_key
    );
    assert!(
        !fixture(name).contains(document_id),
        "the requested document id {document_id} appears in the response body: \
         dataset_by_slug can verify a document-id lookup after all"
    );
}

// --- Fixture provenance -----------------------------------------------------
//
// A fixture with no recorded origin is indistinguishable from one somebody
// hand-wrote to match the code, which is the failure mode the whole file exists
// to prevent. `MANIFEST.json` records the endpoint, status, and capture date for
// every file, and these tests keep it honest: a fixture added without an entry
// fails the build instead of quietly joining the set.

fn manifest() -> serde_json::Value {
    serde_json::from_str(&fixture("MANIFEST.json")).expect("MANIFEST.json does not parse")
}

fn fixture_files_on_disk() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir("tests/fixtures")
        .expect("fixtures directory missing")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json") && name != "MANIFEST.json")
        .collect();
    names.sort();
    names
}

#[test]
fn every_fixture_has_recorded_provenance() {
    let manifest = manifest();
    let captured = &manifest["fixtures"];
    let unverified = &manifest["unverified"];

    let undocumented: Vec<String> = fixture_files_on_disk()
        .into_iter()
        .filter(|name| captured.get(name).is_none() && unverified.get(name).is_none())
        .collect();

    assert!(
        undocumented.is_empty(),
        "these fixtures have no provenance in MANIFEST.json: {undocumented:?}.\n\
         Capture them with scripts/capture-fixtures.sh, or record why they cannot \
         be captured under `unverified` with a reason."
    );
}

#[test]
fn manifest_lists_no_fixture_that_is_missing_from_disk() {
    let manifest = manifest();
    let on_disk = fixture_files_on_disk();

    let mut recorded: Vec<String> = Vec::new();
    for section in ["fixtures", "unverified"] {
        if let Some(entries) = manifest[section].as_object() {
            recorded.extend(entries.keys().cloned());
        }
    }

    let missing: Vec<&String> = recorded
        .iter()
        .filter(|name| !on_disk.contains(name))
        .collect();

    assert!(
        missing.is_empty(),
        "MANIFEST.json records fixtures that are not on disk: {missing:?}"
    );
}

#[test]
fn every_captured_fixture_records_its_endpoint_status_and_date() {
    let manifest = manifest();
    let entries = manifest["fixtures"]
        .as_object()
        .expect("MANIFEST.json has no `fixtures` object");

    assert!(!entries.is_empty(), "no fixtures recorded");

    for (name, entry) in entries {
        assert!(
            entry["endpoint"]
                .as_str()
                .is_some_and(|s| s.starts_with('/')),
            "{name}: `endpoint` must be the request path that produced it"
        );
        assert!(
            entry["status"].as_u64().is_some(),
            "{name}: `status` must record the HTTP status captured"
        );
        assert!(
            entry["source"]
                .as_str()
                .is_some_and(|s| s.starts_with("http")),
            "{name}: `source` must record the host captured from"
        );
        // ISO date, so a stale fixture set is visible without reading git log.
        let captured = entry["captured"].as_str().unwrap_or_default();
        assert!(
            captured.len() == 10 && captured.split('-').count() == 3,
            "{name}: `captured` must be an ISO date, got {captured:?}"
        );
    }
}

#[test]
fn unverified_fixtures_state_why_they_could_not_be_captured() {
    let manifest = manifest();
    let Some(entries) = manifest["unverified"].as_object() else {
        return; // Nothing unverified is the good case.
    };

    for (name, entry) in entries {
        let reason = entry["reason"].as_str().unwrap_or_default();
        assert!(
            reason.len() > 30,
            "{name}: `reason` must explain why this cannot be captured, got {reason:?}"
        );
    }
}

#[test]
fn the_not_found_capture_is_a_real_404_body() {
    // dataset_by_slug distinguishes "no such dataset" from "the API is down" by
    // status alone: the 404 body is a well-formed, empty SearchResponse. If that
    // ever stops being true, the Ok(None) path needs revisiting.
    let entry = &manifest()["fixtures"]["dataset_not_found.json"];
    assert_eq!(
        entry["status"].as_u64(),
        Some(404),
        "dataset_not_found.json must be captured from a real 404"
    );

    let response: models::SearchResponse = parse("dataset_not_found.json");
    assert!(
        response.results.is_empty(),
        "a 404 body must carry no results"
    );
}

#[test]
fn an_empty_search_deserializes_even_though_it_omits_total_and_cursor() {
    // /search and /api/dataset/{slug} return different envelopes: the empty
    // search body is {"results": [], "sort": "relevance"} with no `total` and no
    // cursor at all. Every envelope field must therefore stay optional.
    let raw: serde_json::Value = parse("search_no_matches.json");
    assert!(
        raw.get("total").is_none() && raw.get("after").is_none(),
        "capture no longer omits those fields; re-check the envelope assumptions"
    );

    let response: models::SearchResponse = parse("search_no_matches.json");
    assert!(response.results.is_empty());
    assert!(response.after.is_none(), "no cursor on an empty page");
}
