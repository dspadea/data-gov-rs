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

/// The builder every client in this module is finished from.
fn client_builder(connect_timeout: Duration, timeout: Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(timeout)
}

/// Finish a [`reqwest::ClientBuilder`], reporting a build failure as an error.
///
/// # Errors
///
/// [`CatalogError::RequestError`], carrying reqwest's own message, when no
/// client can be constructed. In reqwest 0.13 `ClientBuilder::build` fails
/// only for TLS backend, proxy, or DNS resolver setup -- never for a timeout
/// value.
fn try_finish_client(builder: reqwest::ClientBuilder) -> Result<reqwest::Client, CatalogError> {
    builder
        .build()
        .map_err(|e| CatalogError::RequestError(Box::new(e)))
}

/// Finish a [`reqwest::ClientBuilder`] for a constructor that has no `Result`
/// to propagate through.
///
/// There is deliberately no fallback client. `reqwest::Client::new` and
/// `<reqwest::Client as Default>::default` both call `ClientBuilder::build`
/// and `expect` its result, so on a host whose TLS backend, proxy, or DNS
/// resolver cannot be set up they fail exactly as this call did -- and panic
/// with the bare text `Client::new()`, which names neither the crate nor the
/// cause. A fallback would therefore not avoid the panic; it would only hide
/// where it came from. Worse, for a build failure reqwest's own default does
/// not share, it silently returns a client with no timeouts at all.
///
/// # Panics
///
/// When reqwest cannot construct a client at all. [`Configuration::try_new`]
/// and [`Configuration::try_with_timeouts`] return that failure as a
/// [`CatalogError`] instead.
fn finish_client(builder: reqwest::ClientBuilder) -> reqwest::Client {
    match builder.build() {
        Ok(client) => client,
        Err(e) => panic!(
            "data-gov-catalog: no HTTP client could be constructed. reqwest's \
             client builder failed, which happens for TLS backend, proxy, or \
             DNS resolver setup - never for a timeout value. \
             reqwest::Client::new() fails the same way, so there is no \
             fallback client to return. Use Configuration::try_new or \
             Configuration::try_with_timeouts to handle this as an error. \
             Cause: {e}"
        ),
    }
}

/// Build a [`reqwest::Client`] with an explicit connect and overall timeout.
///
/// # Panics
///
/// See [`finish_client`].
fn build_client(connect_timeout: Duration, timeout: Duration) -> reqwest::Client {
    finish_client(client_builder(connect_timeout, timeout))
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
    /// Assemble a [`Configuration`] around an already-built client.
    ///
    /// Every constructor funnels through here so the default `base_path` and
    /// `user_agent` are written once, and so the fallible constructors never
    /// reach [`Default::default`] -- which builds a client of its own, and
    /// panics if it cannot.
    fn with_client(client: reqwest::Client) -> Self {
        Self {
            base_path: "https://catalog.data.gov".to_owned(),
            user_agent: Some(concat!("data-gov-rs/", env!("CARGO_PKG_VERSION")).to_owned()),
            client,
        }
    }

    /// Build a [`Configuration`] with default values.
    ///
    /// # Panics
    ///
    /// If reqwest cannot construct an HTTP client at all -- a TLS backend,
    /// proxy, or DNS resolver that will not start. Nothing can be returned in
    /// that case: `reqwest::Client::new` fails the same way. Use
    /// [`Configuration::try_new`] to receive it as an error instead.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a [`Configuration`] with default values, reporting a client
    /// construction failure instead of panicking.
    ///
    /// # Errors
    ///
    /// [`CatalogError::RequestError`] when reqwest cannot construct an HTTP
    /// client: TLS backend, proxy, or DNS resolver setup. There is no
    /// degraded client to fall back to, so a consumer that must not panic
    /// should report this and stop.
    ///
    /// # Examples
    ///
    /// ```
    /// use data_gov_catalog::Configuration;
    ///
    /// let config = Configuration::try_new()?;
    /// assert_eq!(config.base_path, "https://catalog.data.gov");
    /// # Ok::<(), data_gov_catalog::CatalogError>(())
    /// ```
    pub fn try_new() -> Result<Self, CatalogError> {
        Self::try_with_timeouts(DEFAULT_CONNECT_TIMEOUT, DEFAULT_TIMEOUT)
    }

    /// Build a [`Configuration`] with the given timeouts, reporting a client
    /// construction failure instead of panicking.
    ///
    /// The fallible counterpart of [`Configuration::with_timeouts`].
    ///
    /// # Errors
    ///
    /// [`CatalogError::RequestError`] when reqwest cannot construct an HTTP
    /// client. Timeout values themselves are never rejected.
    pub fn try_with_timeouts(
        connect_timeout: Duration,
        timeout: Duration,
    ) -> Result<Self, CatalogError> {
        Ok(Self::with_client(try_finish_client(client_builder(
            connect_timeout,
            timeout,
        ))?))
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
    ///
    /// # Panics
    ///
    /// If reqwest cannot construct an HTTP client at all. See
    /// [`Configuration::new`]; [`Configuration::try_with_timeouts`] returns
    /// that failure as an error.
    pub fn with_timeouts(connect_timeout: Duration, timeout: Duration) -> Self {
        Self::with_client(build_client(connect_timeout, timeout))
    }
}

impl Default for Configuration {
    /// # Panics
    ///
    /// If reqwest cannot construct an HTTP client at all. See
    /// [`Configuration::new`], whose [`Configuration::try_new`] counterpart
    /// returns that failure as an error.
    fn default() -> Self {
        Self::with_client(build_client(DEFAULT_CONNECT_TIMEOUT, DEFAULT_TIMEOUT))
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
    /// A caller-supplied [`SearchParams::per_page`] is outside the range the
    /// Catalog API accepts.
    ///
    /// The API's own OpenAPI schema (`https://catalog.data.gov/openapi.json`,
    /// `/search`'s `per_page` parameter) declares `minimum: 1, maximum:
    /// 1000` and rejects anything else with a bare `400`. Checked here so a
    /// bad value fails locally, with a message naming the valid range,
    /// instead of a round trip to the network.
    InvalidPerPage(i32),
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
            CatalogError::InvalidPerPage(value) => {
                write!(f, "invalid per_page {value}: must be between 1 and 1000")
            }
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

/// Sort order for [`SearchParams::sort`].
///
/// The Catalog API accepts only these four tokens on the wire -- the exact
/// set declared by the `sort` parameter's `enum` in the API's own OpenAPI
/// schema (`https://catalog.data.gov/openapi.json`, `/search`) -- and
/// silently ignores anything else. Confirmed live against `q=climate`,
/// `per_page=5`:
///
/// | Query | Echoed `sort` | Result |
/// |---|---|---|
/// | omitted | `relevance` | baseline, 5 slugs |
/// | `sort=BOGUS_NOT_A_SORT` | `relevance` | **identical** to baseline, HTTP 200, no error |
/// | `sort=relevance` | `relevance` | identical to baseline |
/// | `sort=popularity` | `popularity` | different 5 slugs, stable across repeated calls |
/// | `sort=distance` | `relevance` | identical to baseline -- needs a location context this crate does not supply |
/// | `sort=last_harvested_date` | `last_harvested_date` | different 5 slugs, stable across repeated calls, disjoint from `popularity`'s |
///
/// A typo therefore does not fail; it silently falls back to relevance
/// ranking with HTTP 200, the same failure mode `SpatialFilter` closes for
/// `spatial_filter`. Modelling the set as an enum makes an invalid value a
/// compile error instead of a page of plausible, wrong results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    /// Default relevance ranking against the query text.
    Relevance,
    /// Most popular first.
    Popularity,
    /// Nearest first. Requires a location context this crate does not
    /// currently supply a parameter for; without one it falls back to
    /// [`SortOrder::Relevance`] (confirmed live, see the table above).
    Distance,
    /// Most recently harvested first.
    LastHarvestedDate,
}

impl SortOrder {
    /// The literal token the Catalog API expects on the wire.
    fn as_query_value(self) -> &'static str {
        match self {
            SortOrder::Relevance => "relevance",
            SortOrder::Popularity => "popularity",
            SortOrder::Distance => "distance",
            SortOrder::LastHarvestedDate => "last_harvested_date",
        }
    }
}

/// Valid range for [`SearchParams::per_page`].
///
/// Declared by the Catalog API's own OpenAPI schema
/// (`https://catalog.data.gov/openapi.json`, `/search`'s `per_page`
/// parameter: `minimum: 1, maximum: 1000`) and confirmed live: `per_page=0`,
/// `per_page=1001`, and `per_page=-5` each return `400
/// {"error":"Search failed","message":"per_page must be between 1 and
/// 1000"}`, while `per_page=1000` returns `200`.
const VALID_PER_PAGE: std::ops::RangeInclusive<i32> = 1..=1000;

/// Parameters for [`CatalogClient::search`].
///
/// Constructed with a builder: start from [`SearchParams::new`] and chain
/// setters. All fields are optional; the server defaults apply when a field
/// is left unset.
#[derive(Debug, Default, Clone)]
pub struct SearchParams {
    /// Full-text query.
    pub q: Option<String>,
    /// Sort order.
    pub sort: Option<SortOrder>,
    /// Results per page. Must be in `1..=1000`; [`CatalogClient::search`]
    /// rejects anything outside that range with
    /// [`CatalogError::InvalidPerPage`] before making a request.
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
    pub fn sort(mut self, sort: SortOrder) -> Self {
        self.sort = Some(sort);
        self
    }

    /// Set page size.
    ///
    /// Not validated here -- [`CatalogClient::search`] rejects a value
    /// outside `1..=1000` before making a request. See
    /// [`CatalogError::InvalidPerPage`].
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
        if let Some(v) = self.sort {
            q.push(("sort", v.as_query_value().to_owned()));
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

    /// Check the parameters this crate can validate locally, before any of
    /// them reach the network.
    ///
    /// # Errors
    ///
    /// [`CatalogError::InvalidPerPage`] when
    /// [`per_page`](Self::per_page) is set outside `1..=1000`.
    fn validate(&self) -> Result<(), CatalogError> {
        if let Some(per_page) = self.per_page
            && !VALID_PER_PAGE.contains(&per_page)
        {
            return Err(CatalogError::InvalidPerPage(per_page));
        }
        Ok(())
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
    /// Returns [`CatalogError::InvalidPerPage`] if
    /// [`params.per_page`](SearchParams::per_page) is set outside `1..=1000`
    /// -- checked locally, before any request is made. Returns
    /// [`CatalogError::ApiError`] if the server returns non-2xx,
    /// [`CatalogError::RequestError`] for network/TLS failure, and
    /// [`CatalogError::ParseError`] if the response isn't a valid
    /// [`SearchResponse`](models::SearchResponse).
    pub async fn search(
        &self,
        params: SearchParams,
    ) -> Result<models::SearchResponse, CatalogError> {
        params.validate()?;
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
    /// **Slugs only, though the endpoint takes more.** The `_id` half of
    /// `{slug_or_id}` is the OpenSearch document id, the third element of a
    /// hit's `_sort` array on `/search`. The endpoint resolves one and answers
    /// 200 with the right dataset, but this call still returns `Ok(None)` for
    /// it: the response carries no copy of the id it was asked for -- `_sort`
    /// arrives null on this endpoint -- so nothing in the body can show the
    /// dataset is the one the caller named. Returning it would mean trusting
    /// the server's match unverified, which is what the slug comparison below
    /// exists to prevent. Nothing else resolves at all: the harvest_record
    /// UUID, the CKAN id, and the DCAT `identifier` each 404.
    ///
    /// To go from a document id to a dataset, take the `slug` from the
    /// `/search` hit the id came from and pass that instead.
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

        // The slug is the only thing in the response that can be checked
        // against what was asked for, so a hit whose slug differs is a hit
        // this call cannot attribute to the request -- either the server
        // matched something else, or the caller passed a document id, and the
        // body cannot tell the two apart. Returning it either way would be the
        // silent-wrong-dataset failure this call exists to avoid.
        Ok(response
            .and_then(|r| r.results.into_iter().next())
            .filter(|hit| hit.slug.as_deref() == Some(slug)))
    }

    /// List all organizations known to the catalog.
    ///
    /// The endpoint returns the full list in one response; there is no
    /// pagination today.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::RequestError`] if the request cannot be sent
    /// or its body cannot be read, [`CatalogError::ApiError`] for any
    /// non-2xx status, and [`CatalogError::ParseError`] if the body is not
    /// the shape this endpoint returns.
    pub async fn organizations(&self) -> Result<models::OrganizationsResponse, CatalogError> {
        self.get_json("/api/organizations", &[(); 0]).await
    }

    /// Return the top keywords ranked by document frequency.
    ///
    /// `size` caps the number of rows (server default 100, max 1000).
    /// `min_count` drops keywords with fewer than that many datasets.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::RequestError`] if the request cannot be sent
    /// or its body cannot be read, [`CatalogError::ApiError`] for any
    /// non-2xx status, and [`CatalogError::ParseError`] if the body is not
    /// the shape this endpoint returns.
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
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::RequestError`] if the request cannot be sent
    /// or its body cannot be read, [`CatalogError::ApiError`] for any
    /// non-2xx status, and [`CatalogError::ParseError`] if the body is not
    /// the shape this endpoint returns.
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
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidPathSegment`] before any request is
    /// made if `id` cannot be carried safely in a URL path segment - a
    /// value that normalizes away would silently retarget the request.
    ///
    /// Returns [`CatalogError::RequestError`] if the request cannot be sent
    /// or its body cannot be read, [`CatalogError::ApiError`] for any
    /// non-2xx status, and [`CatalogError::ParseError`] if the body is not
    /// the shape this endpoint returns.
    pub async fn location_geometry(&self, id: &str) -> Result<Value, CatalogError> {
        let path = format!("/api/location/{}", encode_path_segment(id)?);
        self.get_json(&path, &[(); 0]).await
    }

    /// Retrieve a harvest record's metadata envelope.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidPathSegment`] before any request is
    /// made if `id` cannot be carried safely in a URL path segment - a
    /// value that normalizes away would silently retarget the request.
    ///
    /// Returns [`CatalogError::RequestError`] if the request cannot be sent
    /// or its body cannot be read, [`CatalogError::ApiError`] for any
    /// non-2xx status, and [`CatalogError::ParseError`] if the body is not
    /// the shape this endpoint returns.
    pub async fn harvest_record(&self, id: &str) -> Result<models::HarvestRecord, CatalogError> {
        let path = format!("/harvest_record/{}", encode_path_segment(id)?);
        self.get_json(&path, &[(); 0]).await
    }

    /// Retrieve the raw (pre-transform) payload a harvester ingested.
    ///
    /// The payload is not constrained to a single shape — agencies post JSON,
    /// XML fragments, and DCAT records through the same surface — so the
    /// result is returned as [`serde_json::Value`].
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidPathSegment`] before any request is
    /// made if `id` cannot be carried safely in a URL path segment - a
    /// value that normalizes away would silently retarget the request.
    ///
    /// Returns [`CatalogError::RequestError`] if the request cannot be sent
    /// or its body cannot be read, [`CatalogError::ApiError`] for any
    /// non-2xx status, and [`CatalogError::ParseError`] if the body is not
    /// the shape this endpoint returns.
    pub async fn harvest_record_raw(&self, id: &str) -> Result<Value, CatalogError> {
        let path = format!("/harvest_record/{}/raw", encode_path_segment(id)?);
        self.get_json(&path, &[(); 0]).await
    }

    /// Retrieve the DCAT-US 3 transform of a harvest record.
    ///
    /// Returns `Ok(None)` when the harvest record's `source_transform` is
    /// null -- the endpoint 404s in that case, and it is the common answer,
    /// not a failure: across a 752-record sample spanning 18 organizations,
    /// two (census, noaa) had a transform on every record sampled and the
    /// other 16 had none on any (#83). Which organizations populate a
    /// transform looks like a property of the harvest source, not something
    /// callers can predict from the record alone.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::ApiError`] for any non-2xx status other than
    /// 404, [`CatalogError::RequestError`] for network or TLS failure, and
    /// [`CatalogError::ParseError`] if a 200 body is not a valid
    /// [`Dataset`](models::Dataset).
    pub async fn harvest_record_transformed(
        &self,
        id: &str,
    ) -> Result<Option<models::Dataset>, CatalogError> {
        let path = format!("/harvest_record/{}/transformed", encode_path_segment(id)?);
        self.get_json_optional(&path).await
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

    /// A builder that cannot be built, so the no-client-at-all paths can be
    /// exercised on a host whose TLS backend works.
    ///
    /// `\n` is not a legal header value, so `ClientBuilder::user_agent`
    /// stores the error and `build()` returns it. That is the same `Err` a
    /// TLS backend failure produces, and it is the only one reachable from a
    /// test: this crate's builder sets nothing but timeouts, and a timeout
    /// value cannot fail.
    fn unbuildable_client() -> reqwest::ClientBuilder {
        reqwest::Client::builder().user_agent("\n")
    }

    /// When reqwest can build no client, neither can this crate: `Client::new`
    /// and `Client::default` call the same failing builder. The panic is
    /// therefore unavoidable, and what the caller reads must name the crate,
    /// the causes to look at, and the fallible constructor that returns the
    /// failure instead -- not reqwest's bare `Client::new()`, and never a
    /// silently substituted client with no timeouts.
    #[test]
    fn client_construction_failure_panics_with_a_message_naming_the_cause() {
        let payload = std::panic::catch_unwind(|| finish_client(unbuildable_client()))
            .expect_err("a builder that cannot build must not yield a client");
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("<non-string panic payload>");

        for expected in ["data-gov-catalog", "TLS", "try_new"] {
            assert!(
                message.contains(expected),
                "panic message must name {expected:?}, got {message:?}"
            );
        }
    }

    /// The fallible constructors exist so a consumer can report the failure
    /// and exit cleanly rather than take a panic from a library.
    #[test]
    fn try_finish_client_reports_a_construction_failure_as_a_request_error() {
        let error = try_finish_client(unbuildable_client())
            .expect_err("a builder that cannot build must fail");

        assert!(
            matches!(error, CatalogError::RequestError(_)),
            "expected RequestError, got {error:?}"
        );
        let rendered = error.to_string();
        assert!(
            rendered.len() > "Request error: ".len(),
            "the error must carry reqwest's own reason, got {rendered:?}"
        );
    }

    /// On a host that can build a client at all, both fallible constructors
    /// leave every non-timeout field at the value the panicking constructors
    /// use.
    ///
    /// The timeouts themselves are out of reach here: a built
    /// [`reqwest::Client`] does not expose the values it was configured with,
    /// so nothing in this test can distinguish a client that honours its
    /// argument from one that ignored it. That property is behavioural and is
    /// proved against a stalled server by
    /// `try_with_timeouts_bounds_a_stalled_request_to_the_given_timeout` in
    /// `tests/client_tests.rs`.
    #[test]
    fn try_new_and_try_with_timeouts_leave_the_non_timeout_fields_at_their_defaults() {
        let config = Configuration::try_new().expect("a client builds on this host");
        assert_eq!(config.base_path, Configuration::new().base_path);
        assert_eq!(config.user_agent, Configuration::new().user_agent);

        let timed =
            Configuration::try_with_timeouts(Duration::from_secs(1), Duration::from_secs(2))
                .expect("a client builds on this host");
        assert_eq!(timed.base_path, config.base_path);
        assert_eq!(timed.user_agent, config.user_agent);
    }
}
