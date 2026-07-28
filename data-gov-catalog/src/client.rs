//! HTTP client and error types for the data.gov Catalog API.

use crate::models;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::Arc;

/// Configuration for the Catalog API client.
///
/// The defaults target the public data.gov endpoint. Override `base_path`
/// to point at a staging instance or at `https://api.data.gov/catalog`
/// once the announced migration lands.
#[derive(Debug, Clone)]
pub struct Configuration {
    /// Base URL for the Catalog API (e.g. `https://catalog.data.gov`).
    pub base_path: String,
    /// User-Agent header sent with every request.
    pub user_agent: Option<String>,
    /// Shared reqwest client. Cheap to clone; reuse across requests.
    pub client: reqwest::Client,
}

impl Configuration {
    /// Build a [`Configuration`] with default values.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            base_path: "https://catalog.data.gov".to_owned(),
            user_agent: Some(concat!("data-gov-rs/", env!("CARGO_PKG_VERSION")).to_owned()),
            client: reqwest::Client::new(),
        }
    }
}

/// Percent-encode a single URL path segment.
///
/// Everything outside the unreserved set is escaped, including `/` and `.`, so
/// a value containing `..`, `%2e` or a slash cannot escape its segment and
/// redirect the request to a different endpoint. `.` is escaped rather than
/// passed through precisely so that `..` cannot survive as a dot-segment.
fn encode_path_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'~' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Async client for the Catalog API.
///
/// Holds an [`Arc<Configuration>`] so it's cheap to clone and share across
/// tasks. Every method is `async` and returns [`Result<_, CatalogError>`].
pub struct CatalogClient {
    configuration: Arc<Configuration>,
}

impl std::fmt::Debug for CatalogClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CatalogClient")
            .field("base_path", &self.configuration.base_path)
            .finish()
    }
}

/// Errors returned by the Catalog API client.
#[derive(Debug)]
pub enum CatalogError {
    /// Network, TLS, or HTTP-protocol failure.
    RequestError(Box<dyn std::error::Error + Send + Sync>),
    /// JSON could not be deserialized into the expected shape.
    ParseError(serde_json::Error),
    /// The server returned a non-2xx status code.
    ApiError {
        /// HTTP status code.
        status: u16,
        /// Server-provided response body (often a JSON error document).
        message: String,
    },
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CatalogError::RequestError(e) => write!(f, "Request error: {e}"),
            CatalogError::ParseError(e) => write!(f, "Parse error: {e}"),
            CatalogError::ApiError { status, message } => {
                write!(f, "Catalog API error ({status}): {message}")
            }
        }
    }
}

impl std::error::Error for CatalogError {}

/// Parameters for [`CatalogClient::search`].
///
/// Constructed with a builder: start from [`SearchParams::new`] and chain
/// setters. All fields are optional; the server defaults apply when a field
/// is left unset.
#[derive(Debug, Default, Clone)]
pub struct SearchParams {
    /// Full-text query.
    pub q: Option<String>,
    /// Sort order (`relevance`, `popularity`, `distance`, `last_harvested_date`).
    pub sort: Option<String>,
    /// Results per page.
    pub per_page: Option<i32>,
    /// Filter by organization slug (e.g. `nasa`).
    pub org_slug: Option<String>,
    /// Filter by organization type (e.g. `Federal Government`).
    pub org_type: Option<String>,
    /// Exact-match keyword filters. Repeated on the wire.
    pub keyword: Vec<String>,
    /// `geospatial` or `non-geospatial`.
    pub spatial_filter: Option<String>,
    /// GeoJSON geometry used for bounding-box / shape queries.
    pub spatial_geometry: Option<Value>,
    /// Whether to require containment (true) vs. intersection (false).
    pub spatial_within: Option<bool>,
    /// Opaque cursor from a previous [`SearchResponse::after`](models::SearchResponse::after).
    pub after: Option<String>,
    /// Exact-match slug filter (single dataset lookup).
    pub slug: Option<String>,
}

impl SearchParams {
    /// Construct empty [`SearchParams`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the full-text query.
    pub fn q(mut self, q: impl Into<String>) -> Self {
        self.q = Some(q.into());
        self
    }

    /// Set the sort order.
    pub fn sort(mut self, sort: impl Into<String>) -> Self {
        self.sort = Some(sort.into());
        self
    }

    /// Set page size.
    pub fn per_page(mut self, per_page: i32) -> Self {
        self.per_page = Some(per_page);
        self
    }

    /// Filter by organization slug.
    pub fn org_slug(mut self, slug: impl Into<String>) -> Self {
        self.org_slug = Some(slug.into());
        self
    }

    /// Filter by organization type.
    pub fn org_type(mut self, org_type: impl Into<String>) -> Self {
        self.org_type = Some(org_type.into());
        self
    }

    /// Append a keyword filter (exact match).
    pub fn keyword(mut self, keyword: impl Into<String>) -> Self {
        self.keyword.push(keyword.into());
        self
    }

    /// Replace the keyword list.
    pub fn keywords<I, S>(mut self, keywords: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.keyword = keywords.into_iter().map(Into::into).collect();
        self
    }

    /// Set the spatial-filter mode.
    pub fn spatial_filter(mut self, mode: impl Into<String>) -> Self {
        self.spatial_filter = Some(mode.into());
        self
    }

    /// Set the GeoJSON geometry for spatial queries.
    pub fn spatial_geometry(mut self, geometry: Value) -> Self {
        self.spatial_geometry = Some(geometry);
        self
    }

    /// Require containment vs. intersection for spatial matches.
    pub fn spatial_within(mut self, within: bool) -> Self {
        self.spatial_within = Some(within);
        self
    }

    /// Set the pagination cursor.
    pub fn after(mut self, after: impl Into<String>) -> Self {
        self.after = Some(after.into());
        self
    }

    /// Filter to an exact slug match (single-dataset lookup).
    pub fn slug(mut self, slug: impl Into<String>) -> Self {
        self.slug = Some(slug.into());
        self
    }

    /// Serialize to the repeated `(key, value)` form reqwest expects.
    fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut q: Vec<(&'static str, String)> = Vec::new();
        if let Some(v) = &self.q {
            q.push(("q", v.clone()));
        }
        if let Some(v) = &self.sort {
            q.push(("sort", v.clone()));
        }
        if let Some(v) = self.per_page {
            q.push(("per_page", v.to_string()));
        }
        if let Some(v) = &self.org_slug {
            q.push(("org_slug", v.clone()));
        }
        if let Some(v) = &self.org_type {
            q.push(("org_type", v.clone()));
        }
        for kw in &self.keyword {
            q.push(("keyword", kw.clone()));
        }
        if let Some(v) = &self.spatial_filter {
            q.push(("spatial_filter", v.clone()));
        }
        if let Some(v) = &self.spatial_geometry {
            q.push(("spatial_geometry", v.to_string()));
        }
        if let Some(v) = self.spatial_within {
            q.push(("spatial_within", v.to_string()));
        }
        if let Some(v) = &self.after {
            q.push(("after", v.clone()));
        }
        if let Some(v) = &self.slug {
            q.push(("slug", v.clone()));
        }
        q
    }
}

impl CatalogClient {
    /// Construct a new client from a shared [`Configuration`].
    pub fn new(configuration: Arc<Configuration>) -> Self {
        Self { configuration }
    }

    /// Build a URL by joining `path` onto the configured base.
    fn url(&self, path: &str) -> String {
        let base = self.configuration.base_path.trim_end_matches('/');
        format!("{base}{path}")
    }

    /// Issue a GET where a 404 means "no such thing" rather than a failure.
    ///
    /// Distinct from [`Self::get_json`], which treats every non-2xx as an
    /// error. Collapsing the two is how a data.gov outage came to be reported
    /// to users as a missing dataset.
    async fn get_json_optional<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<Option<T>, CatalogError> {
        let mut req = self.configuration.client.get(self.url(path));
        if let Some(ua) = &self.configuration.user_agent {
            req = req.header(reqwest::header::USER_AGENT, ua);
        }
        let response = req
            .send()
            .await
            .map_err(|e| CatalogError::RequestError(Box::new(e)))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "<no body>".to_string());
            return Err(CatalogError::ApiError { status, message });
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| CatalogError::RequestError(Box::new(e)))?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(CatalogError::ParseError)
    }

    /// Issue a GET with optional query parameters and deserialize the JSON body.
    async fn get_json<T: DeserializeOwned, Q: serde::Serialize + ?Sized>(
        &self,
        path: &str,
        params: &Q,
    ) -> Result<T, CatalogError> {
        let mut req = self.configuration.client.get(self.url(path)).query(params);
        if let Some(ua) = &self.configuration.user_agent {
            req = req.header(reqwest::header::USER_AGENT, ua);
        }
        let response = req
            .send()
            .await
            .map_err(|e| CatalogError::RequestError(Box::new(e)))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "<no body>".to_string());
            return Err(CatalogError::ApiError { status, message });
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| CatalogError::RequestError(Box::new(e)))?;
        serde_json::from_slice(&bytes).map_err(CatalogError::ParseError)
    }

    /// Search datasets. See the module docs for parameters.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::ApiError`] if the server returns non-2xx,
    /// [`CatalogError::RequestError`] for network/TLS failure, and
    /// [`CatalogError::ParseError`] if the response isn't a valid
    /// [`SearchResponse`](models::SearchResponse).
    pub async fn search(
        &self,
        params: SearchParams,
    ) -> Result<models::SearchResponse, CatalogError> {
        let query = params.to_query();
        self.get_json("/search", &query).await
    }

    /// Fetch a single dataset by its data.gov slug.
    ///
    /// Returns `Ok(None)` if no dataset with that slug exists. The returned
    /// [`SearchHit`](models::SearchHit) carries the denormalized fields and a
    /// nested `dcat` record with the full DCAT-US 3 metadata.
    ///
    /// Uses `GET /api/dataset/{slug_or_id}`, the exact-lookup endpoint declared
    /// in the API's own OpenAPI document at `/openapi.json`. Lookup is exact:
    /// the endpoint does no prefix or substring matching, so a near-miss slug
    /// returns 404 rather than a plausible wrong dataset.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::ApiError`] for any non-2xx status other than
    /// 404, [`CatalogError::RequestError`] for network or TLS failure, and
    /// [`CatalogError::ParseError`] if the body is not a valid
    /// [`SearchResponse`](models::SearchResponse). A 404 is *not* an error — it
    /// is the "no such dataset" answer and yields `Ok(None)`.
    pub async fn dataset_by_slug(
        &self,
        slug: &str,
    ) -> Result<Option<models::SearchHit>, CatalogError> {
        let path = format!("/api/dataset/{}", encode_path_segment(slug));
        let response: Option<models::SearchResponse> = self.get_json_optional(&path).await?;

        // Belt and braces: the endpoint is exact, but a hit whose slug differs
        // from the request would mean the server matched something else, and
        // returning it would be the silent-wrong-dataset failure this call
        // exists to avoid.
        Ok(response
            .and_then(|r| r.results.into_iter().next())
            .filter(|hit| hit.slug.as_deref() == Some(slug)))
    }

    /// List all organizations known to the catalog.
    ///
    /// The endpoint returns the full list in one response; there is no
    /// pagination today.
    pub async fn organizations(&self) -> Result<models::OrganizationsResponse, CatalogError> {
        self.get_json("/api/organizations", &[(); 0]).await
    }

    /// Return the top keywords ranked by document frequency.
    ///
    /// `size` caps the number of rows (server default 100, max 1000).
    /// `min_count` drops keywords with fewer than that many datasets.
    pub async fn keywords(
        &self,
        size: Option<i32>,
        min_count: Option<i32>,
    ) -> Result<models::KeywordsResponse, CatalogError> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(s) = size {
            params.push(("size", s.to_string()));
        }
        if let Some(m) = min_count {
            params.push(("min_count", m.to_string()));
        }
        self.get_json("/api/keywords", &params).await
    }

    /// Autocomplete against known locations.
    pub async fn locations_search(
        &self,
        q: &str,
        size: Option<i32>,
    ) -> Result<models::LocationsResponse, CatalogError> {
        let mut params: Vec<(&str, String)> = vec![("q", q.to_string())];
        if let Some(s) = size {
            params.push(("size", s.to_string()));
        }
        self.get_json("/api/locations/search", &params).await
    }

    /// Fetch the GeoJSON geometry for a given location id.
    ///
    /// The response is returned as a raw [`serde_json::Value`] because the
    /// shape is unconstrained GeoJSON and callers typically hand it straight
    /// to a mapping library.
    pub async fn location_geometry(&self, id: &str) -> Result<Value, CatalogError> {
        let path = format!("/api/location/{id}");
        self.get_json(&path, &[(); 0]).await
    }

    /// Retrieve a harvest record's metadata envelope.
    pub async fn harvest_record(&self, id: &str) -> Result<models::HarvestRecord, CatalogError> {
        let path = format!("/harvest_record/{id}");
        self.get_json(&path, &[(); 0]).await
    }

    /// Retrieve the raw (pre-transform) payload a harvester ingested.
    ///
    /// The payload is not constrained to a single shape — agencies post JSON,
    /// XML fragments, and DCAT records through the same surface — so the
    /// result is returned as [`serde_json::Value`].
    pub async fn harvest_record_raw(&self, id: &str) -> Result<Value, CatalogError> {
        let path = format!("/harvest_record/{id}/raw");
        self.get_json(&path, &[(); 0]).await
    }

    /// Retrieve the DCAT-US 3 transform of a harvest record.
    pub async fn harvest_record_transformed(
        &self,
        id: &str,
    ) -> Result<models::Dataset, CatalogError> {
        let path = format!("/harvest_record/{id}/transformed");
        self.get_json(&path, &[(); 0]).await
    }
}
