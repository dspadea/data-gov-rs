//! Types that model the Catalog API response payloads.
//!
//! Every field that the upstream API omits is wrapped in [`Option`] because
//! DCAT-US 3 records vary widely across publishers. Unknown or transitional
//! fields are preserved in [`serde_json::Value`] extras where appropriate.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Envelope returned by the `/search` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    /// Datasets matching the query on this page.
    #[serde(default)]
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
    #[serde(default)]
    pub keyword: Vec<String>,
    /// DCAT-US themes (broad subject categories).
    #[serde(default)]
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
    #[serde(default)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<Publisher>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "contactPoint"
    )]
    pub contact_point: Option<ContactPoint>,
    #[serde(default)]
    pub keyword: Vec<String>,
    #[serde(default)]
    pub theme: Vec<String>,
    /// Downloadable / accessible representations of the dataset.
    #[serde(default)]
    pub distribution: Vec<Distribution>,
    /// Publisher's landing page for this dataset.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "landingPage"
    )]
    pub landing_page: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporal: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "accrualPeriodicity"
    )]
    pub accrual_periodicity: Option<String>,
    #[serde(default)]
    pub language: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "bureauCode"
    )]
    pub bureau_code: Option<Value>,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "describedByType"
    )]
    pub described_by_type: Option<String>,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "dataQuality"
    )]
    pub data_quality: Option<bool>,
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
    #[serde(default, rename = "@type", skip_serializing_if = "Option::is_none")]
    pub type_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "describedBy"
    )]
    pub described_by: Option<String>,
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
    #[serde(default, rename = "@type", skip_serializing_if = "Option::is_none")]
    pub type_hint: Option<String>,
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
    #[serde(default, rename = "@type", skip_serializing_if = "Option::is_none")]
    pub type_hint: Option<String>,
    /// Full name of the contact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fn_: Option<String>,
    /// Email URI (e.g. `mailto:ops@example.gov`).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "hasEmail")]
    pub has_email: Option<String>,
}

// `fn` is a keyword; accept it via `fn_` with a rename.
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
    #[serde(default)]
    pub organizations: Vec<Organization>,
    #[serde(default)]
    pub total: i64,
}

/// A publishing organization as the catalog knows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_count: Option<i64>,
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// Envelope returned by `/api/keywords`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordsResponse {
    #[serde(default)]
    pub keywords: Vec<KeywordCount>,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub min_count: i64,
}

/// One keyword entry with its document-frequency count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordCount {
    pub keyword: String,
    pub count: i64,
}

/// Envelope returned by `/api/locations/search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationsResponse {
    #[serde(default)]
    pub locations: Vec<Location>,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub size: i64,
}

/// A location suggestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub id: String,
    pub display_name: String,
}

/// A harvest record as returned by `/harvest_record/{id}` (metadata envelope,
/// distinct from the transformed DCAT payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarvestRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ckan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harvest_job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harvest_source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_finished: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    /// Raw upstream payload (often a large JSON object or XML string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_raw: Option<Value>,
    /// DCAT-US transformation of `source_raw`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_transform: Option<Value>,
}
