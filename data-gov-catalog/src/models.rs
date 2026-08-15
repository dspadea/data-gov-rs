//! Types that model the Catalog API response payloads.
//!
//! Every field that the upstream API omits is wrapped in [`Option`] because
//! DCAT-US 3 records vary widely across publishers. Unknown or transitional
//! fields are preserved in [`serde_json::Value`] extras where appropriate.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Deserialize a JSON `null` as `T::default()`.
///
/// `#[serde(default)]` alone covers the *missing field* case but doesn't
/// help when the field is *present and explicitly null*. The Catalog API
/// returns `null` for empty repeated DCAT-US 3 fields like `references`,
/// `keyword`, etc., so every `Vec<T>` field on these models needs this
/// extra hop to avoid `invalid type: null, expected a sequence` panics.
fn deserialize_null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// Envelope returned by the `/search` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    /// Datasets matching the query on this page.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub results: Vec<SearchHit>,
    /// Opaque cursor for the next page. Absent on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Sort mode echoed back by the server (e.g. `"relevance"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
}

/// A single search hit.
///
/// Denormalized top-level fields duplicate the most common DCAT-US 3 fields
/// for convenience; the full canonical record is nested under [`Self::dcat`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    /// Publisher-assigned identifier (often a URL or URN).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    /// URL-friendly slug for this dataset in the data.gov UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// Human-readable title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Plain-text description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Short name of the publishing source (may be a domain or agency code).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// Publishing organization record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<Organization>,
    /// Free-form tags.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub keyword: Vec<String>,
    /// DCAT-US themes (broad subject categories).
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub theme: Vec<String>,
    /// Whether this dataset advertises spatial coverage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_spatial: Option<bool>,
    /// Opaque popularity score used for ranking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub popularity: Option<i64>,
    /// Timestamp of the most recent successful harvest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_harvested_date: Option<String>,
    /// Distribution titles listed out for convenience (may be empty).
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub distribution_titles: Vec<String>,
    /// URL of the harvest record for this dataset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harvest_record: Option<String>,
    /// URL of the raw (pre-transform) harvest payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harvest_record_raw: Option<String>,
    /// GeoJSON centroid if `has_spatial` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial_centroid: Option<Value>,
    /// GeoJSON shape if `has_spatial` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial_shape: Option<Value>,
    /// Canonical DCAT-US 3 record for this dataset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dcat: Option<Dataset>,
    /// Ranking score (present when `sort=relevance`).
    #[serde(default, rename = "_score", skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// Cursor components that generated this hit's position.
    #[serde(default, rename = "_sort", skip_serializing_if = "Option::is_none")]
    pub sort_key: Option<Value>,
}

/// DCAT-US 3 dataset record.
///
/// Also the payload returned by `/harvest_record/{id}/transformed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    /// DCAT type hint, typically `"dcat:Dataset"`.
    #[serde(default, rename = "@type", skip_serializing_if = "Option::is_none")]
    pub type_hint: Option<String>,
    /// Human-readable name of the dataset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Human-readable description of the dataset, as plain text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Publisher-assigned identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    /// `public`, `restricted public`, or `non-public`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "accessLevel"
    )]
    pub access_level: Option<String>,
    /// ISO 8601 date the record was last modified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    /// ISO 8601 date the record was first issued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued: Option<String>,
    /// Organization that published the dataset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<Publisher>,
    /// Person or role to contact about the dataset.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "contactPoint"
    )]
    pub contact_point: Option<ContactPoint>,
    /// Publisher-assigned tags. Free-form, and not drawn from any
    /// controlled vocabulary.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub keyword: Vec<String>,
    /// Broad subject categories the publisher assigned.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub theme: Vec<String>,
    /// Downloadable / accessible representations of the dataset.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub distribution: Vec<Distribution>,
    /// Publisher's landing page for this dataset.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "landingPage"
    )]
    pub landing_page: Option<String>,
    /// URL of the licence the dataset is released under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Explanation of who may access the dataset, where
    /// [`Self::access_level`] is not `public`. Publishers send both
    /// prose and single tokens such as `otherRestrictions` here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights: Option<String>,
    /// Geographic coverage, as a place name or as a comma-separated
    /// bounding box (`west,south,east,north`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial: Option<String>,
    /// Period the data covers, as an ISO 8601 interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporal: Option<String>,
    /// How often the dataset is updated, as an ISO 8601 duration.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "accrualPeriodicity"
    )]
    pub accrual_periodicity: Option<String>,
    /// Languages the dataset is available in, as IETF language tags.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub language: Vec<String>,
    /// OMB bureau codes for the publishing agency, in `NNN:NN` form.
    ///
    /// Untyped because publishers are not consistent about whether
    /// this arrives as a single string or an array of them, and a
    /// strict type here would fail the whole record over one
    /// publisher's choice.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "bureauCode"
    )]
    pub bureau_code: Option<Value>,
    /// OMB program codes, in `NNN:NNN` form. Untyped for the same
    /// reason as [`Self::bureau_code`].
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "programCode"
    )]
    pub program_code: Option<Value>,
    /// Metadata describing the record's schema.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "describedBy"
    )]
    pub described_by: Option<String>,
    /// Media type of the resource at [`Self::described_by`].
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "describedByType"
    )]
    pub described_by_type: Option<String>,
    /// URLs of related documents.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub references: Vec<String>,
    /// Whether the publisher asserts the dataset meets its agency's
    /// information-quality guidelines.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "dataQuality"
    )]
    pub data_quality: Option<bool>,
    /// URL of the Privacy Act system-of-records notice covering the
    /// dataset, where one applies.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "systemOfRecords"
    )]
    pub system_of_records: Option<String>,
}

/// One downloadable or API-accessible representation of a dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distribution {
    /// DCAT type hint, typically `"dcat:Distribution"`.
    #[serde(default, rename = "@type", skip_serializing_if = "Option::is_none")]
    pub type_hint: Option<String>,
    /// Human-readable name of this distribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Human-readable description of this distribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Direct download URL for the distribution file.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "downloadURL"
    )]
    pub download_url: Option<String>,
    /// Access URL for APIs or web-based views.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "accessURL")]
    pub access_url: Option<String>,
    /// IANA media type (e.g. `text/csv`).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "mediaType")]
    pub media_type: Option<String>,
    /// Short format label (e.g. `CSV`, `JSON`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// URL of the licence this distribution is released under, where
    /// it differs from the dataset's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// URL of a schema or data dictionary for this distribution.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "describedBy"
    )]
    pub described_by: Option<String>,
    /// Media type of the resource at [`Self::described_by`].
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "describedByType"
    )]
    pub described_by_type: Option<String>,
}

/// DCAT publisher object (`org:Organization`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Publisher {
    /// DCAT type hint, typically `"org:Organization"`.
    #[serde(default, rename = "@type", skip_serializing_if = "Option::is_none")]
    pub type_hint: Option<String>,
    /// Name of the organization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Nested publisher (parent organization).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "subOrganizationOf"
    )]
    pub sub_organization_of: Option<Box<Publisher>>,
}

/// DCAT contact point (`vcard:Contact`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactPoint {
    /// DCAT type hint, typically `"vcard:Contact"`.
    #[serde(default, rename = "@type", skip_serializing_if = "Option::is_none")]
    pub type_hint: Option<String>,
    /// Full name of the contact.
    #[serde(default, rename = "fn", skip_serializing_if = "Option::is_none")]
    pub fn_: Option<String>,
    /// Email URI (e.g. `mailto:ops@example.gov`).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "hasEmail")]
    pub has_email: Option<String>,
}

// `fn` is a keyword; the field is named `fn_` and mapped back to the wire
// name `fn` with `rename = "fn"` above.
impl ContactPoint {
    /// Create a [`ContactPoint`] with the DCAT `fn` field populated.
    pub fn with_name(name: impl Into<String>) -> Self {
        Self {
            type_hint: None,
            fn_: Some(name.into()),
            has_email: None,
        }
    }
}

/// Envelope returned by `/api/organizations`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationsResponse {
    /// Every organization the catalog knows about. This endpoint
    /// returns the full list in one response.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub organizations: Vec<Organization>,
    /// Number of organizations in [`Self::organizations`].
    #[serde(default)]
    pub total: i64,
}

/// A publishing organization as the catalog knows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    /// Catalog-internal identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Display name of the organization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// URL-friendly identifier, and the value
    /// [`SearchParams::org_slug`](crate::SearchParams::org_slug)
    /// filters on.
    ///
    /// Read it from here rather than guessing it from the name: the
    /// slug for NOAA is `noaa`, not `noaa-gov`, and the API answers a
    /// wrong slug with an empty result rather than an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// Human-readable description of the organization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// URL of the organization's logo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    /// Category the catalog assigns to the organization, such as the
    /// level of government it belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_type: Option<String>,
    /// Number of datasets the organization publishes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_count: Option<i64>,
    /// Number of harvest sources the organization operates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_count: Option<i64>,
    /// Other names the organization is known by.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub aliases: Vec<String>,
}

/// Envelope returned by `/api/keywords`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordsResponse {
    /// Keywords ranked by the number of datasets carrying them,
    /// most frequent first.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub keywords: Vec<KeywordCount>,
    /// Number of keywords in [`Self::keywords`].
    #[serde(default)]
    pub total: i64,
    /// Row cap the server applied, echoing the requested `size`.
    #[serde(default)]
    pub size: i64,
    /// Minimum dataset count a keyword needed to be included,
    /// echoing the requested `min_count`.
    #[serde(default)]
    pub min_count: i64,
}

/// One keyword entry with its document-frequency count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordCount {
    /// The keyword itself.
    pub keyword: String,
    /// Number of datasets carrying this keyword.
    pub count: i64,
}

/// Envelope returned by `/api/locations/search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationsResponse {
    /// Matching locations, best match first.
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub locations: Vec<Location>,
    /// Number of locations in [`Self::locations`].
    #[serde(default)]
    pub total: i64,
    /// Row cap the server applied, echoing the requested `size`.
    #[serde(default)]
    pub size: i64,
}

/// A location suggestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    /// Identifier to pass to
    /// [`CatalogClient::location_geometry`](crate::CatalogClient::location_geometry)
    /// to fetch this location's GeoJSON.
    pub id: String,
    /// Human-readable place name.
    pub display_name: String,
}

/// A harvest record as returned by `/harvest_record/{id}` (metadata envelope,
/// distinct from the transformed DCAT payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarvestRecord {
    /// Identifier of this harvest record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Identifier this dataset carried in the retired CKAN catalog,
    /// where it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ckan_id: Option<String>,
    /// Publisher-assigned identifier of the harvested dataset, which
    /// is the same value as the DCAT record's `identifier`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    /// Identifier of the parent record, for a dataset harvested as
    /// part of a collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_identifier: Option<String>,
    /// Identifier of the harvest job that produced this record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harvest_job_id: Option<String>,
    /// Identifier of the harvest source this record came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harvest_source_id: Option<String>,
    /// What the harvest did with this record, such as `update`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Outcome the harvester recorded for this record, such as
    /// `success`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Timestamp the harvest of this record began, ISO 8601 without
    /// a zone offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_created: Option<String>,
    /// Timestamp the harvest of this record finished, ISO 8601
    /// without a zone offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_finished: Option<String>,
    /// Hex digest of the upstream payload, used to detect whether it
    /// changed since the last harvest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    /// Raw upstream payload (often a large JSON object or XML string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_raw: Option<Value>,
    /// DCAT-US transformation of `source_raw`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_transform: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ContactPoint::with_name` had never been called or tested (#77): its
    /// only claim of correctness was its own rustdoc. Prove it actually
    /// populates the field the DCAT `fn` rename reads from, and that the
    /// value it produces serializes under the real wire key.
    #[test]
    fn with_name_populates_the_fn_field_and_serializes_under_the_wire_key() {
        let contact = ContactPoint::with_name("Jane Doe");

        assert_eq!(contact.fn_.as_deref(), Some("Jane Doe"));
        assert_eq!(contact.type_hint, None);
        assert_eq!(contact.has_email, None);

        let serialized = serde_json::to_value(&contact).expect("ContactPoint serializes");
        assert_eq!(
            serialized.get("fn").and_then(Value::as_str),
            Some("Jane Doe"),
            "with_name's value must round-trip through the `fn` wire key: got {serialized}"
        );
        assert!(
            serialized.get("fn_").is_none(),
            "must not emit the schema-invalid key `fn_`: got {serialized}"
        );
    }
}
