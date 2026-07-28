//! HTTP client and error types for the data.gov Catalog API.

use crate::models;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// Default TCP/TLS connect timeout applied by [`Configuration::default`].
///
/// Kept generous relative to [`DEFAULT_TIMEOUT`]: a slow handshake on a
/// loaded host is common and is not itself the failure this bounds. What it
/// rules out is a connection that never completes at all.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Default overall request timeout applied by [`Configuration::default`].
///
/// Matches the value in this crate's README configuration example, so the
/// documented posture and the shipped default agree.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Build a [`reqwest::Client`] with an explicit connect and overall timeout.
///
/// Falls back to [`reqwest::Client::new`] (no timeout at all) only if the
/// builder itself fails. In practice that happens for a TLS backend
/// misconfiguration, never for a timeout value -- but [`Default::default`]
/// has no `Result` to propagate through, so a fallback is the only option
/// that does not turn a working call into a panic.
fn build_client(connect_timeout: Duration, timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(timeout)
        .build()
        .unwrap_or_default()
}

/// Configuration for the Catalog API client.
///
/// The defaults target the public data.gov endpoint. Override `base_path`
/// to point at a staging instance or at `https://api.data.gov/catalog`
/// once the announced migration lands.
///
/// There is deliberately no `timeout` field here: a [`reqwest::Client`]
/// bakes its timeouts in at construction and cannot be reconfigured
/// afterward, so a field that did not also rebuild `client` would silently
/// stop applying the moment someone set it. Use
/// [`Configuration::with_timeouts`] to get a client with different bounds,
/// or build `client` yourself for anything beyond timeouts (a proxy, custom
/// headers, connection pooling).
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

    /// Build a [`Configuration`] whose client uses the given timeouts
    /// instead of [`DEFAULT_CONNECT_TIMEOUT`] and [`DEFAULT_TIMEOUT`].
    ///
    /// `base_path` and `user_agent` are left at their defaults; set them
    /// afterward (both fields are `pub`) if they need to change too.
    ///
    /// # Examples
    ///
    /// ```
    /// use data_gov_catalog::Configuration;
    /// use std::time::Duration;
    ///
    /// let config = Configuration::with_timeouts(
    ///     Duration::from_secs(5),
    ///     Duration::from_secs(15),
    /// );
    /// ```
    pub fn with_timeouts(connect_timeout: Duration, timeout: Duration) -> Self {
        Self {
            client: build_client(connect_timeout, timeout),
            ..Self::default()
        }
    }
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            base_path: "https://catalog.data.gov".to_owned(),
            user_agent: Some(concat!("data-gov-rs/", env!("CARGO_PKG_VERSION")).to_owned()),
            client: build_client(DEFAULT_CONNECT_TIMEOUT, DEFAULT_TIMEOUT),
        }
    }
}

/// Percent-encode a single URL path segment.
///
/// Everything outside the unreserved set is escaped, including `/` and `.`, so
/// a value containing a slash, a `#`, a `?`, or `..` as a substring cannot
/// escape its segment and redirect the request to a different endpoint.
///
/// Encoding is not sufficient on its own. Three values have no representation
/// as a path segment at all and are refused instead:
///
/// | Value | What the URL parser does with it |
/// |---|---|
/// | `..` | Removes the segment *and* its parent: `/harvest_record/../raw` becomes `/raw` |
/// | `.` | Removes the segment: `/harvest_record/./raw` becomes `/harvest_record/raw` |
/// | `""` | Leaves an empty segment, so the path carries `//` |
///
/// Percent-encoding does not help for the first two. The URL standard looks for
/// dot-segments *after* decoding, and treats `%2E` as a dot for that purpose, so
/// `%2E%2E` collapses exactly as `..` does. Verified against `url` 2.5: an id of
/// `..` turns `/harvest_record/{id}/transformed` into `/transformed`.
///
/// Only a value that is entirely dots is affected. `../search` is safe, because
/// the encoded slash keeps it one segment.
///
/// # Errors
///
/// [`CatalogError::InvalidPathSegment`] when `segment` is `""`, `"."`, or `".."`.
fn encode_path_segment(segment: &str) -> Result<String, CatalogError> {
    if matches!(segment, "" | "." | "..") {
        return Err(CatalogError::InvalidPathSegment(segment.to_owned()));
    }
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'~' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    Ok(out)
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
    /// A caller-supplied id or slug cannot be carried in a URL path segment.
    ///
    /// Percent-encoding is not enough for every value. A segment that is
    /// exactly `.` or `..` is removed by URL normalization even when encoded,
    /// because the URL standard treats `%2E` as a dot when it looks for
    /// dot-segments. Such a value would silently retarget the request at a
    /// different endpoint, so it is refused before any request is made.
    InvalidPathSegment(String),
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CatalogError::RequestError(e) => write!(f, "Request error: {e}"),
            CatalogError::ParseError(e) => write!(f, "Parse error: {e}"),
            CatalogError::ApiError { status, message } => {
                write!(f, "Catalog API error ({status}): {message}")
            }
            CatalogError::InvalidPathSegment(value) => write!(
                f,
                "invalid identifier {value:?}: it cannot be carried in a URL path segment"
            ),
        }
    }
}

impl std::error::Error for CatalogError {}

/// Whether [`SearchParams::spatial_filter`] restricts results to datasets
/// that advertise spatial coverage, or to ones that don't.
///
/// The Catalog API accepts only these two tokens on the wire and silently
/// ignores anything else -- confirmed live: `spatial_filter=BOGUS` returns a
/// full unfiltered page with HTTP 200, the same as omitting the parameter,
/// with no error at all. Modelling the set as an enum makes an invalid
/// value a compile error instead of a request the server quietly discards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialFilter {
    /// Only datasets with `has_spatial: true`.
    Geospatial,
    /// Only datasets with `has_spatial: false`.
    NonGeospatial,
}

impl SpatialFilter {
    /// The literal token the Catalog API expects on the wire.
    fn as_query_value(self) -> &'static str {
        match self {
            SpatialFilter::Geospatial => "geospatial",
            SpatialFilter::NonGeospatial => "non-geospatial",
        }
    }
}

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
    /// Restrict to datasets with (or without) spatial coverage.
    pub spatial_filter: Option<SpatialFilter>,
    /// GeoJSON geometry used for bounding-box / shape queries.
    pub spatial_geometry: Option<Value>,
    /// Whether a dataset's shape must be contained by
    /// [`spatial_geometry`](Self::spatial_geometry) (`true`) or merely
    /// intersect it (`false`).
    ///
    /// Has no observable effect set on its own -- confirmed live: a search
    /// with only `spatial_within=true` and no `spatial_geometry` returns
    /// results identical to the unfiltered baseline. It only changes
    /// anything alongside `spatial_geometry`. Confirmed live there too: a
    /// tiny query geometry over Antarctica (nowhere any dataset's shape sits)
    /// combined with `spatial_within=true` returns zero results, while the
    /// same geometry with `spatial_within=false` returns the datasets whose
    /// shape merely intersects it (global-coverage datasets such as
    /// `world-ocean-atlas-2023`) -- proof the parameter is a real modifier
    /// of `spatial_geometry`, not a phantom.
    pub spatial_within: Option<bool>,
    /// Opaque cursor from a previous [`SearchResponse::after`](models::SearchResponse::after).
    pub after: Option<String>,
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
    pub fn spatial_filter(mut self, mode: SpatialFilter) -> Self {
        self.spatial_filter = Some(mode);
        self
    }

    /// Set the GeoJSON geometry for spatial queries.
    pub fn spatial_geometry(mut self, geometry: Value) -> Self {
        self.spatial_geometry = Some(geometry);
        self
    }

    /// Require containment vs. intersection for spatial matches. See
    /// [`SearchParams::spatial_within`] for what this does and does not
    /// affect on its own.
    pub fn spatial_within(mut self, within: bool) -> Self {
        self.spatial_within = Some(within);
        self
    }

    /// Set the pagination cursor.
    pub fn after(mut self, after: impl Into<String>) -> Self {
        self.after = Some(after.into());
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
        if let Some(v) = self.spatial_filter {
            q.push(("spatial_filter", v.as_query_value().to_owned()));
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
        let path = format!("/api/dataset/{}", encode_path_segment(slug)?);
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
        let path = format!("/api/location/{}", encode_path_segment(id)?);
        self.get_json(&path, &[(); 0]).await
    }

    /// Retrieve a harvest record's metadata envelope.
    pub async fn harvest_record(&self, id: &str) -> Result<models::HarvestRecord, CatalogError> {
        let path = format!("/harvest_record/{}", encode_path_segment(id)?);
        self.get_json(&path, &[(); 0]).await
    }

    /// Retrieve the raw (pre-transform) payload a harvester ingested.
    ///
    /// The payload is not constrained to a single shape — agencies post JSON,
    /// XML fragments, and DCAT records through the same surface — so the
    /// result is returned as [`serde_json::Value`].
    pub async fn harvest_record_raw(&self, id: &str) -> Result<Value, CatalogError> {
        let path = format!("/harvest_record/{}/raw", encode_path_segment(id)?);
        self.get_json(&path, &[(); 0]).await
    }

    /// Retrieve the DCAT-US 3 transform of a harvest record.
    pub async fn harvest_record_transformed(
        &self,
        id: &str,
    ) -> Result<models::Dataset, CatalogError> {
        let path = format!("/harvest_record/{}/transformed", encode_path_segment(id)?);
        self.get_json(&path, &[(); 0]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the values the README's configuration section documents (#48):
    /// a request against an unresponsive host must fail well inside the
    /// overall timeout, and the connect timeout is comfortably shorter than
    /// the overall one so a stalled handshake is distinguishable from a
    /// stalled response. `reqwest::Client` exposes no getter for the
    /// timeouts it was built with, so the enforcement itself is proven by
    /// `client_tests::a_short_configured_timeout_bounds_a_stalled_request`,
    /// which exercises the same `build_client` path through
    /// `Configuration::with_timeouts`; this test guards the constants that
    /// path feeds `Configuration::default` with.
    #[test]
    fn default_timeouts_are_finite_and_match_the_documented_values() {
        assert_eq!(DEFAULT_CONNECT_TIMEOUT, Duration::from_secs(10));
        assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(30));
        assert!(
            DEFAULT_CONNECT_TIMEOUT < DEFAULT_TIMEOUT,
            "connect timeout must be shorter than the overall request timeout"
        );
    }

    /// `Configuration::default()` and `Configuration::new()` must build a
    /// client without panicking or requiring a runtime. Cheap, and the only
    /// exercise of `Default::default()` in the offline suite -- the live
    /// integration tests construct one too, but only `#[ignore]`d.
    #[test]
    fn configuration_default_and_new_build_without_panicking() {
        let default_config = Configuration::default();
        assert_eq!(default_config.base_path, "https://catalog.data.gov");

        let new_config = Configuration::new();
        assert_eq!(new_config.base_path, default_config.base_path);
    }
}
