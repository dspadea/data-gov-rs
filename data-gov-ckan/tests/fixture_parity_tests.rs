//! Parity between the CKAN models and what live CKAN portals actually return.
//!
//! Every fixture here is a verbatim capture from a live public CKAN portal,
//! refreshed with `scripts/capture-ckan-fixtures.sh`. These tests deserialize
//! each capture into the model it is supposed to populate and assert the
//! fields really arrive.
//!
//! This is deliberately different from `unit_tests.rs`, which serves fixtures
//! (and, where noted, hand-written bodies) through wiremock to exercise
//! request shaping. Those tests confirm a request was *sent* correctly; these
//! confirm a response is *understood* correctly. A hand-written mock body
//! encodes the same assumption as the model it is meant to test — CKAN's `id`
//! column is unconstrained text, but every hand-written body in this crate
//! used a UUID-shaped id, because that is what `Package.id: Option<uuid::Uuid>`
//! expects. The suite was green and the client was broken (#102).
//!
//! When one of these fails, the API is right and the model is wrong.
//! Recapture the fixtures, confirm the shape, then fix the model — never the
//! reverse.

use data_gov_ckan::models;

fn fixture(name: &str) -> String {
    let path = format!("tests/fixtures/{name}");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {path} missing: {e}"))
}

fn parse<T: serde::de::DeserializeOwned>(name: &str) -> T {
    serde_json::from_str(&fixture(name))
        .unwrap_or_else(|e| panic!("fixture {name} does not deserialize into the model: {e}"))
}

/// The `result` payload for an action response, unwrapped the way
/// `CkanClient::call_action` unwraps it in production.
fn result_of(name: &str) -> serde_json::Value {
    let envelope: serde_json::Value = parse(name);
    envelope
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("fixture {name} has no top-level `result`"))
}

/// `true` for the canonical 8-4-4-4-12 hex-with-hyphens shape CKAN's default
/// `make_uuid()` produces. Used only to prove a fixture mixes UUID and
/// non-UUID ids in one response — not a validator, just a shape check.
fn looks_like_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&groups)
            .all(|(len, g)| g.len() == *len && g.chars().all(|c| c.is_ascii_hexdigit()))
}

// --- package_search ----------------------------------------------------

#[test]
fn package_search_populates_core_package_fields() {
    let result = result_of("package_search.json");
    let search: models::PackageSearchResult =
        serde_json::from_value(result).expect("package_search.json result does not deserialize");

    let packages = search.results.expect("capture has no results");
    assert!(!packages.is_empty(), "capture has no packages");

    let pkg = &packages[0];
    assert!(!pkg.name.is_empty(), "package name missing");
    assert!(pkg.id.is_some(), "package id missing");
    assert!(pkg.title.is_some(), "package title missing");
    assert!(pkg.organization.is_some(), "package organization missing");
}

/// The acceptance test for #62: this fixture's largest resource is a CSV
/// over 2 GiB. Before the fix (`size: Option<i32>`), deserializing the
/// containing `Package` fails entirely — one oversized file discards the
/// whole record, resources, tags, and all.
#[test]
fn package_show_resource_size_beyond_i32_max_does_not_fail_the_page() {
    let result = result_of("package_show.json");
    let pkg: models::Package =
        serde_json::from_value(result).expect("package_show.json result does not deserialize");

    let resources = pkg.resources.expect("capture has no resources");
    assert!(!resources.is_empty(), "capture has no resources");

    let oversized = resources
        .iter()
        .find(|r| r.size.is_some_and(|s| s > i64::from(i32::MAX)))
        .expect("fixture no longer has a resource over i32::MAX bytes; recapture (see #62)");
    assert_eq!(oversized.size, Some(2_290_761_766));

    // Siblings must survive too: a small resource on the very same page.
    let small = resources
        .iter()
        .find(|r| r.size.is_some_and(|s| s < 100_000))
        .expect("fixture no longer has a small sibling resource");
    assert!(small.size.unwrap() > 0);
}

/// Some portals emit `resource.size` as a human-formatted string rather than
/// a byte count (`"523 KiB"`, not `535552`). That string is not parseable as
/// a number, so it must degrade to `None` rather than failing the resource,
/// which in turn must not fail the page it belongs to.
#[test]
fn resource_size_as_unparseable_string_degrades_to_none_not_an_error() {
    let result = result_of("resource_size_as_string.json");
    let pkg: models::Package = serde_json::from_value(result)
        .expect("resource_size_as_string.json result does not deserialize");

    let resources = pkg.resources.expect("capture has no resources");
    let stringy = resources
        .iter()
        .find(|r| r.format.as_deref() == Some("CSV"))
        .expect("fixture no longer has the CSV resource with a string size");
    assert_eq!(
        stringy.size, None,
        "a non-numeric size string must degrade to None, not error"
    );
}

// --- package_show --------------------------------------------------------

#[test]
fn package_show_populates_organization_and_resources() {
    let result = result_of("package_show.json");
    let pkg: models::Package =
        serde_json::from_value(result).expect("package_show.json result does not deserialize");

    assert!(!pkg.name.is_empty(), "package name missing");
    assert!(pkg.title.is_some(), "package title missing");

    let org = pkg.organization.expect("organization missing");
    assert_eq!(org.name, "tbs-sct", "organization name missing or wrong");

    let resources = pkg.resources.expect("no resources");
    assert!(
        resources.iter().any(|r| r.format.as_deref() == Some("CSV")),
        "expected at least one CSV resource"
    );
}

// --- organization_list ----------------------------------------------------

#[test]
fn organization_list_is_a_plain_name_array() {
    // This is what `CkanClient::organization_list` actually calls: no
    // `all_fields`, so the result is a bare array of org names/slugs. There
    // is no id to widen here — this fixture exists to prove the request
    // shape the client uses actually round-trips, not to exercise #63.
    let result = result_of("organization_list.json");
    let names: Vec<String> =
        serde_json::from_value(result).expect("organization_list.json result is not [String]");

    assert!(!names.is_empty(), "capture has no organizations");
    assert!(names.iter().all(|n| !n.is_empty()));
}

/// The acceptance test for #63: this fixture mixes CKAN's default UUID ids
/// with organizations created with an explicit slug id
/// (`central-statistics-office`, `an-garda-siochana`, ...). Before the fix
/// (`Group.id: Option<uuid::Uuid>`), the slug-id records fail UUID parsing
/// and take the whole `organization_list` response down with them — not just
/// the offending organization.
#[test]
fn organization_list_with_non_uuid_ids_does_not_fail_the_whole_response() {
    let result = result_of("organization_list_slug_ids.json");
    let orgs: Vec<models::Group> = serde_json::from_value(result)
        .expect("organization_list_slug_ids.json result does not deserialize");

    assert!(!orgs.is_empty(), "capture has no organizations");

    let non_uuid = orgs
        .iter()
        .find(|o| o.id.as_deref() == Some("an-garda-siochana"))
        .expect("fixture no longer has the an-garda-siochana org; recapture (see #63)");
    assert_eq!(non_uuid.name, "an-garda-siochana");

    // The mix matters: a fixture with only slug ids would not prove the two
    // id styles can coexist in one response.
    let uuid_shaped = orgs
        .iter()
        .find(|o| o.id.as_deref().is_some_and(looks_like_uuid))
        .expect("fixture no longer has any UUID-shaped org id to contrast against");
    assert_ne!(uuid_shaped.id, non_uuid.id);
}

/// The other half of #63's acceptance case: a non-UUID organization id
/// arriving *nested* inside a real `package_search` result — the exact
/// production path (`PackageSearchResult` -> `Package` -> `organization` ->
/// `Group.id`) that took the entire results page down before the fix.
#[test]
fn package_search_with_non_uuid_organization_id_does_not_fail_the_page() {
    let result = result_of("package_search_non_uuid_org_id.json");
    let search: models::PackageSearchResult = serde_json::from_value(result)
        .expect("package_search_non_uuid_org_id.json result does not deserialize");

    let packages = search.results.expect("capture has no results");
    assert!(!packages.is_empty(), "capture has no packages");

    for pkg in &packages {
        let org = pkg
            .organization
            .as_ref()
            .expect("package has no organization");
        assert_eq!(
            org.id.as_deref(),
            Some("central-statistics-office"),
            "expected every result to belong to the pinned non-UUID organization"
        );
        // The package itself keeps its normal UUID id — only the nested
        // organization id is non-UUID on this portal. Both must parse.
        assert!(pkg.id.is_some(), "package id missing");
    }
}

// --- autocomplete endpoints (#102) ------------------------------------------
//
// unit_tests.rs's own header comment used to justify hand-writing the four
// `*_autocomplete` wiremock bodies by claiming they "return either a bare
// array of strings or a 3-4 field struct with no id or numeric field of the
// kind #63/#62 affected." Half of that was never checked against a live
// portal: `OrganizationAutocomplete` and `GroupAutocomplete` both declare
// `id: Option<String>`, and data.gov.ie -- already in this crate's capture
// set -- returns a real one for both. These fixtures are the correction;
// see the updated note in unit_tests.rs.

/// Live evidence for #102: `organization_autocomplete` returns a real,
/// non-UUID slug id, so `OrganizationAutocomplete::id` deserializing was
/// never actually exercised by any hand-written test body that omitted it.
#[test]
fn organization_autocomplete_returns_a_real_slug_id() {
    let result = result_of("organization_autocomplete.json");
    let orgs: Vec<models::OrganizationAutocomplete> = serde_json::from_value(result)
        .expect("organization_autocomplete.json result does not deserialize");

    assert!(!orgs.is_empty(), "capture has no organization suggestions");
    let central = orgs
        .iter()
        .find(|o| o.name.as_deref() == Some("central-statistics-office"))
        .expect(
            "fixture no longer suggests the Central Statistics Office for q=central; recapture",
        );
    assert_eq!(central.id.as_deref(), Some("central-statistics-office"));
}

/// The same acceptance case for `group_autocomplete`: data.gov.ie's one
/// group carries CKAN's default UUID id -- still a real, present `id`, not
/// the absence the pre-fix policy note claimed for this endpoint.
#[test]
fn group_autocomplete_returns_a_real_id() {
    let result = result_of("group_autocomplete.json");
    let groups: Vec<models::GroupAutocomplete> = serde_json::from_value(result)
        .expect("group_autocomplete.json result does not deserialize");

    assert!(!groups.is_empty(), "capture has no group suggestions");
    let hale = groups
        .iter()
        .find(|g| g.name.as_deref() == Some("haleandhearty"))
        .expect("fixture no longer suggests the haleandhearty group for q=hale; recapture");
    assert!(hale.id.is_some(), "group id missing");
}

// --- error responses -------------------------------------------------------

#[test]
fn the_not_found_capture_is_a_real_404_with_ckans_error_envelope() {
    let entry = &manifest()["fixtures"]["package_show_not_found.json"];
    assert_eq!(
        entry["status"].as_u64(),
        Some(404),
        "package_show_not_found.json must be captured from a real 404"
    );

    let envelope: models::ErrorResponse = parse("package_show_not_found.json");
    assert!(!envelope.success);
    assert_eq!(envelope.error.message, "Not found");
    assert_eq!(envelope.error.__type, "Not Found Error");
}

/// CKAN's validation-error shape replaces the documented `message` field with
/// per-field arrays (`{"name_or_id": ["Missing value"], "__type": "..."}`),
/// so it does *not* deserialize into `ErrorResponseError` (whose `message` is
/// required, not optional). `ValidationErrorResponseError` is the model that
/// actually matches this shape.
#[test]
fn the_validation_error_capture_has_no_fixed_message_field() {
    let entry = &manifest()["fixtures"]["package_show_validation_error.json"];
    assert_eq!(
        entry["status"].as_u64(),
        Some(409),
        "package_show_validation_error.json must be captured from a real validation failure"
    );

    let raw: serde_json::Value = parse("package_show_validation_error.json");
    assert!(
        raw["error"].get("message").is_none(),
        "capture now has a message field; the two-shape assumption needs re-checking"
    );

    let envelope: models::ValidationErrorResponse = parse("package_show_validation_error.json");
    assert!(!envelope.success);
    assert_eq!(
        envelope.error.__type,
        Some(models::validation_error_response_error::Type::ValidationError)
    );
}

// --- Fixture provenance -----------------------------------------------------
//
// A fixture with no recorded origin is indistinguishable from one somebody
// hand-wrote to match the code, which is the failure mode this whole file
// exists to prevent. `MANIFEST.json` records the endpoint, status, source
// portal, and capture date for every file, and these tests keep it honest: a
// fixture added without an entry fails the build instead of quietly joining
// the set.

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
         Capture them with scripts/capture-ckan-fixtures.sh, or record why they cannot \
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
fn every_captured_fixture_records_its_endpoint_status_source_and_date() {
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
            "{name}: `source` must record the portal captured from"
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
        return; // Nothing unverified is the good case, and it is the case today.
    };

    for (name, entry) in entries {
        let reason = entry["reason"].as_str().unwrap_or_default();
        assert!(
            reason.len() > 30,
            "{name}: `reason` must explain why this cannot be captured, got {reason:?}"
        );
    }
}

/// At least one fixture must come from a portal that was chosen specifically
/// because it does *not* generate UUID ids by default. Losing this would mean
/// #63's acceptance case silently reverted to a UUID-only, self-confirming
/// capture set — exactly the failure mode #102 exists to prevent.
#[test]
fn the_fixture_set_includes_a_non_uuid_id_portal() {
    let manifest = manifest();
    let sources: Vec<&str> = manifest["fixtures"]
        .as_object()
        .expect("no fixtures recorded")
        .values()
        .filter_map(|entry| entry["source"].as_str())
        .collect();

    assert!(
        sources.contains(&"https://data.gov.ie"),
        "expected a capture from data.gov.ie, the portal with non-UUID entity ids (#63)"
    );
}
