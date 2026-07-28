use crate::models;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::time::Duration;

/// Connect timeout [`Configuration::default`] builds [`Configuration::client`] with.
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Overall request timeout [`Configuration::default`] builds [`Configuration::client`] with.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Configuration for the CKAN client
#[derive(Clone)]
pub struct Configuration {
    /// Base URL for the CKAN API (e.g., `https://open.canada.ca/data/en/api/3`).
    ///
    /// data.gov retired its CKAN endpoint in 2026; do not point this at
    /// `catalog.data.gov`. Set it to your target CKAN-compatible portal.
    pub base_path: String,
    /// User agent string for HTTP requests
    pub user_agent: Option<String>,
    /// HTTP client instance.
    ///
    /// [`Configuration::default`] (and therefore [`Configuration::new`])
    /// builds this from [`Self::connect_timeout`] and [`Self::timeout`], so a
    /// stalled server -- one that accepts the connection but never sends
    /// response headers -- cannot hang a caller forever. Supplying your own
    /// `client` here (via struct-update syntax against a value other than
    /// `Configuration::default()`, or a bare struct literal) bypasses those
    /// two fields entirely; set your own timeouts on it directly in that
    /// case.
    pub client: reqwest::Client,
    /// Connect timeout used when [`Configuration::default`] builds [`Self::client`].
    ///
    /// Changing this field on an already-constructed `Configuration` has no
    /// effect on `client`, which was already built; see [`Self::client`].
    pub connect_timeout: Duration,
    /// Overall request timeout used when [`Configuration::default`] builds [`Self::client`].
    ///
    /// Changing this field on an already-constructed `Configuration` has no
    /// effect on `client`, which was already built; see [`Self::client`].
    pub timeout: Duration,
    /// Basic authentication credentials (username, optional password)
    pub basic_auth: Option<BasicAuth>,
    /// OAuth access token.
    ///
    /// Not currently attached to outgoing requests: CKAN's Action API
    /// defines no OAuth flow of its own, and the RFC 6750 bearer semantics
    /// an OAuth access token normally rides on over HTTP are covered by
    /// [`Self::bearer_access_token`] instead. Kept for API compatibility.
    pub oauth_access_token: Option<String>,
    /// Bearer token for authentication
    pub bearer_access_token: Option<String>,
    /// API key for CKAN authentication
    pub api_key: Option<ApiKey>,
}

/// Basic authentication credentials
pub type BasicAuth = (String, Option<String>);

/// API key configuration
#[derive(Clone)]
pub struct ApiKey {
    /// Optional prefix for the API key (e.g., "Bearer")
    pub prefix: Option<String>,
    /// The actual API key value
    pub key: String,
}

impl std::fmt::Debug for ApiKey {
    /// Redacts [`Self::key`] so an `ApiKey` never leaks a secret into a log
    /// line, an error message, or a test failure printed to a CI console.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKey")
            .field("prefix", &self.prefix)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl Configuration {
    /// Create a new configuration with default values
    pub fn new() -> Configuration {
        Configuration::default()
    }
}

/// Build a [`reqwest::Client`] with an explicit connect and request timeout.
///
/// `ClientBuilder::build()` only fails for structurally invalid client
/// configuration (a bad TLS backend or proxy setup); a builder that sets only
/// timeouts cannot fail in practice. Fall back to an untimed client rather
/// than panicking, per #48's acceptance criteria -- a client with no timeout
/// is still strictly better than no client at all.
fn build_client(connect_timeout: Duration, timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(timeout)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

impl Default for Configuration {
    fn default() -> Self {
        let connect_timeout = DEFAULT_CONNECT_TIMEOUT;
        let timeout = DEFAULT_TIMEOUT;
        let client = build_client(connect_timeout, timeout);

        Configuration {
            // catalog.data.gov (the old default) is a confirmed 404: data.gov
            // retired its CKAN endpoint in 2026. open.canada.ca is a live,
            // government-run CKAN portal -- verified responding, not
            // publicly editable -- so a caller who runs this crate's own
            // quick-start example unmodified sees it actually work (#72.3).
            // Point this at your own instance for any real use.
            base_path: "https://open.canada.ca/data/en/api/3".to_owned(),
            user_agent: Some(concat!("data-gov-rs/", env!("CARGO_PKG_VERSION")).to_owned()),
            client,
            connect_timeout,
            timeout,
            basic_auth: None,
            oauth_access_token: None,
            bearer_access_token: None,
            api_key: None,
        }
    }
}

/// `Some("[REDACTED]")` for a configured secret, `None` for an absent one.
///
/// Lets a `Configuration` be logged whole (`tracing::debug!(?config)`, a
/// failed-assertion printout, ...) without a credential ever reaching a log
/// line or a console.
fn redacted(secret: &Option<String>) -> Option<&'static str> {
    secret.as_ref().map(|_| "[REDACTED]")
}

impl std::fmt::Debug for Configuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Username is not secret; only the password half of basic auth is.
        let basic_auth = self
            .basic_auth
            .as_ref()
            .map(|(user, pass)| (user.as_str(), pass.as_deref().map(|_| "[REDACTED]")));

        f.debug_struct("Configuration")
            .field("base_path", &self.base_path)
            .field("user_agent", &self.user_agent)
            .field("client", &self.client)
            .field("connect_timeout", &self.connect_timeout)
            .field("timeout", &self.timeout)
            .field("basic_auth", &basic_auth)
            .field("oauth_access_token", &redacted(&self.oauth_access_token))
            .field("bearer_access_token", &redacted(&self.bearer_access_token))
            .field("api_key", &self.api_key)
            .finish()
    }
}

/// Async CKAN client focused on data.gov's read APIs.
///
/// `CkanClient` wraps the generated request glue and exposes ergonomic async
/// methods for popular CKAN endpoints such as `package_search`,
/// `package_show`, organization listings, and autocomplete helpers. The client
/// is cheap to clone and share across tasks because it only holds an
/// [`Arc<Configuration>`].
///
/// ```rust,no_run
/// use data_gov_ckan::{CkanClient, Configuration};
/// use std::sync::Arc;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = CkanClient::new(Arc::new(Configuration::default()));
/// let results = client.package_search(Some("climate"), Some(5), None, None).await?;
/// println!("{} datasets", results.count.unwrap_or(0));
/// # Ok(()) }
/// ```
pub struct CkanClient {
    configuration: Arc<Configuration>,
}

impl std::fmt::Debug for CkanClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CkanClient")
            .field("base_path", &self.configuration.base_path)
            .finish()
    }
}

/// Errors that can occur when interacting with the CKAN API
///
/// This enum provides detailed error information for different types of failures
/// that can occur during CKAN API operations.
///
/// # Examples
///
/// ```rust
/// # use data_gov_ckan::{CkanClient, CkanError};
/// # async fn example() {
/// match some_api_call().await {
///     Ok(result) => println!("Success: {:?}", result),
///     Err(CkanError::RequestError(e)) => {
///         eprintln!("Network or HTTP error: {}", e);
///     },
///     Err(CkanError::ParseError(e)) => {
///         eprintln!("Failed to parse API response: {}", e);
///     },
///     Err(CkanError::ApiError { status, message }) => {
///         eprintln!("CKAN API returned error {}: {}", status, message);
///     }
/// }
/// # async fn some_api_call() -> Result<(), CkanError> { Ok(()) }
/// # }
/// ```
#[derive(Debug)]
pub enum CkanError {
    /// Network, HTTP, or other request-level errors
    ///
    /// This includes connection failures, timeouts, DNS resolution issues,
    /// and HTTP protocol errors (like 500 Internal Server Error).
    RequestError(Box<dyn std::error::Error + Send + Sync>),

    /// JSON parsing or deserialization errors
    ///
    /// Occurs when the CKAN API returns data that doesn't match expected schema,
    /// invalid JSON, or when response format has changed.
    ParseError(serde_json::Error),

    /// CKAN-specific API errors with status codes
    ///
    /// These are semantic errors from CKAN itself, like:
    /// - 404: Dataset not found
    /// - 403: Insufficient permissions
    /// - 400: Invalid parameters
    /// - 409: Resource conflicts
    ApiError {
        /// HTTP status code from the CKAN API
        status: u16,
        /// Human-readable error message from CKAN
        message: String,
    },
}

impl std::fmt::Display for CkanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CkanError::RequestError(e) => write!(f, "Request error: {}", e),
            CkanError::ParseError(e) => write!(f, "Parse error: {}", e),
            CkanError::ApiError { status, message } => {
                write!(f, "CKAN API error ({}): {}", status, message)
            }
        }
    }
}

impl std::error::Error for CkanError {}

impl CkanClient {
    /// Create a new CKAN client instance
    ///
    /// Creates a client configured to work with a specific CKAN instance.
    /// data.gov no longer exposes a CKAN API; point this at your own
    /// CKAN-compatible portal, e.g. `https://open.canada.ca/data/en/api/3`.
    ///
    /// # Arguments
    ///
    /// * `configuration` - API configuration including base URL, user agent, and credentials
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use data_gov_ckan::{CkanClient, Configuration, ApiKey};
    /// # use std::sync::Arc;
    ///
    /// // Basic client for read-only operations
    /// let config = Arc::new(Configuration {
    ///     base_path: "https://open.canada.ca/data/en/api/3".to_string(),
    ///     user_agent: Some("my-rust-app/1.0".to_string()),
    ///     ..Configuration::default()
    /// });
    ///
    /// let client = CkanClient::new(config);
    ///
    /// // Client with API key for write operations
    /// let authenticated_config = Arc::new(Configuration {
    ///     base_path: "https://open.canada.ca/data/en/api/3".to_string(),
    ///     user_agent: Some("my-rust-app/1.0".to_string()),
    ///     api_key: Some(ApiKey {
    ///         prefix: None,
    ///         key: "your-api-key-here".to_string(),
    ///     }),
    ///     ..Configuration::default()
    /// });
    ///
    /// let auth_client = CkanClient::new(authenticated_config);
    /// ```
    pub fn new(configuration: Arc<Configuration>) -> Self {
        Self { configuration }
    }

    /// Call a CKAN action API endpoint and deserialize the result.
    ///
    /// All CKAN action endpoints follow the same pattern: GET a URL under
    /// `/action/<name>` with query parameters, receive a JSON wrapper with
    /// `{ success: bool, result: ... }`, and extract the `result` field.
    /// This helper encapsulates that entire flow.
    async fn call_action<T: DeserializeOwned>(
        &self,
        action: &str,
        params: &[(&str, &str)],
    ) -> Result<T, CkanError> {
        let url = format!("{}/action/{}", self.configuration.base_path, action);

        let mut req = self.configuration.client.get(&url).query(params);
        if let Some(ua) = &self.configuration.user_agent {
            req = req.header(reqwest::header::USER_AGENT, ua);
        }
        req = self.apply_credentials(req);

        let response = req
            .send()
            .await
            .map_err(|e| CkanError::RequestError(Box::new(e)))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            // A dropped connection while reading the body is a transport
            // failure and must surface as RequestError, not be folded into
            // this branch's own ApiError -- `.text()` failing here means we
            // never got CKAN's error body at all, so there is no CKAN error
            // to report (#72.2). A body that arrived whole but isn't valid
            // JSON, or isn't the error envelope's shape, stays a decode
            // concern handled below by falling back to the raw text.
            let body_text = response
                .text()
                .await
                .map_err(|e| CkanError::RequestError(Box::new(e)))?;
            let message = serde_json::from_str::<serde_json::Value>(&body_text)
                .ok()
                .and_then(|v| v.get("error").cloned())
                .map(|error| error_envelope_message(&error))
                .unwrap_or(body_text);
            return Err(CkanError::ApiError { status, message });
        }

        // `.text()` failing is a transport problem (RequestError); the text
        // arriving but not parsing as the expected envelope shape is a
        // decode problem (ParseError). `response.json()` used to collapse
        // both into RequestError, which is documented as covering
        // "connection failures, timeouts, DNS resolution issues" -- not
        // deserialization, which ParseError is documented as covering (#72.2).
        let body_text = response
            .text()
            .await
            .map_err(|e| CkanError::RequestError(Box::new(e)))?;
        let wrapper: models::ActionResponse =
            serde_json::from_str(&body_text).map_err(CkanError::ParseError)?;

        if !wrapper.success {
            let message = wrapper
                .error
                .as_ref()
                .map(error_envelope_message)
                .unwrap_or_else(|| "CKAN API reported failure".to_string());
            return Err(CkanError::ApiError {
                status: 400,
                message,
            });
        }

        match wrapper.result {
            Some(value) => serde_json::from_value(value).map_err(CkanError::ParseError),
            None => Err(CkanError::ApiError {
                status: 500,
                message: "No result data in API response".to_string(),
            }),
        }
    }

    /// Attach whichever credential is configured, in priority order: an
    /// explicit CKAN API key, then a bearer token, then HTTP basic auth.
    ///
    /// A CKAN API key is sent as `Authorization: <prefix> <key>` (or just
    /// `<key>` with no configured prefix) -- the mechanism CKAN's own API
    /// documents natively, distinct from RFC 6750 bearer tokens. Only one
    /// credential is ever attached per request, so a `Configuration` with
    /// more than one set is not an error, just resolved by this order.
    ///
    /// `oauth_access_token` is not attached here: CKAN's Action API defines
    /// no OAuth flow of its own, and RFC 6750 bearer semantics -- which is
    /// what an OAuth access token normally rides on over HTTP -- are already
    /// covered by `bearer_access_token`.
    fn apply_credentials(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let config = &self.configuration;
        if let Some(api_key) = &config.api_key {
            let value = match &api_key.prefix {
                Some(prefix) => format!("{prefix} {}", api_key.key),
                None => api_key.key.clone(),
            };
            return req.header(reqwest::header::AUTHORIZATION, value);
        }
        if let Some(token) = &config.bearer_access_token {
            return req.bearer_auth(token);
        }
        if let Some((username, password)) = &config.basic_auth {
            return req.basic_auth(username, password.as_deref());
        }
        req
    }

    /// Search for datasets (packages) with advanced filtering and faceting
    ///
    /// This is the primary method for discovering datasets in the CKAN catalog.
    /// It provides powerful search capabilities with full-text search, filtering,
    /// faceting, and pagination.
    ///
    /// # Arguments
    ///
    /// * `q` - Search query string (searches title, description, tags, etc.)
    /// * `rows` - Maximum number of results to return (default: 10, max typically: 1000)
    /// * `start` - Starting offset for pagination (0-based)
    /// * `fq` - Additional filter queries in Solr format
    ///
    /// # Returns
    ///
    /// Returns search results with datasets, facets, and pagination information.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use data_gov_ckan::{CkanClient, Configuration};
    /// # use std::sync::Arc;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = CkanClient::new(Arc::new(Configuration {
    /// #     base_path: "https://open.canada.ca/data/en/api/3".to_string(),
    /// #     user_agent: Some("test".to_string()),
    /// #     ..Configuration::default()
    /// # }));
    ///
    /// // Basic text search
    /// let results = client.package_search(
    ///     Some("climate change"),
    ///     Some(20),
    ///     Some(0),
    ///     None
    /// ).await?;
    ///
    /// println!("Found {} total datasets", results.count.unwrap_or(0));
    /// for package in results.results.unwrap_or_default() {
    ///     println!("Title: {}", package.title.unwrap_or_default());
    ///     println!("Organization: {}", package.organization.as_ref()
    ///         .and_then(|org| org.title.as_deref())
    ///         .unwrap_or("Unknown"));
    /// }
    ///
    /// // Search with organization filter
    /// let epa_datasets = client.package_search(
    ///     Some("water quality"),
    ///     Some(10),
    ///     Some(0),
    ///     Some("organization:epa-gov")
    /// ).await?;
    ///
    /// // Search with multiple filters
    /// let recent_climate_data = client.package_search(
    ///     Some("climate"),
    ///     Some(15),
    ///     Some(0),
    ///     Some("res_format:CSV AND metadata_modified:[2020-01-01T00:00:00Z TO NOW]")
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Pagination
    ///
    /// ```rust,ignore
    /// // Paginate through all results
    /// let mut start = 0;
    /// let page_size = 50;
    ///
    /// loop {
    ///     let results = client.package_search(
    ///         Some("energy"),
    ///         Some(page_size),
    ///         Some(start),
    ///         None
    ///     ).await?;
    ///
    ///     let packages = results.results.unwrap_or_default();
    ///     if packages.is_empty() {
    ///         break;
    ///     }
    ///
    ///     // Process this page of results
    ///     for package in packages {
    ///         println!("Processing: {}", package.name);
    ///     }
    ///
    ///     start += page_size;
    /// }
    /// ```
    ///
    /// # Advanced Filtering
    ///
    /// The `fq` parameter supports Solr query syntax for advanced filtering. The
    /// `q` and `fq` parameters are passed through to CKAN's Solr-backed
    /// package_search endpoint, so you can use familiar Solr constructs such as:
    ///
    /// - `organization:epa-gov` - Filter by organization
    /// - `res_format:CSV` - Filter by resource format
    /// - `tags:healthcare` - Filter by tags
    /// - `metadata_modified:[2020-01-01T00:00:00Z TO NOW]` - Date ranges
    /// - Combine with `AND`, `OR`, `NOT` operators
    ///
    /// Examples:
    ///
    /// - Text search with wildcard: `q=climat*`
    /// - Phrase search: `q="air quality"`
    /// - Complex filter: `fq=organization:epa-gov AND res_format:CSV`
    pub async fn package_search(
        &self,
        q: Option<&str>,
        rows: Option<i32>,
        start: Option<i32>,
        fq: Option<&str>,
    ) -> Result<models::PackageSearchResult, CkanError> {
        let rows_str = rows.map(|r| r.to_string());
        let start_str = start.map(|s| s.to_string());

        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(q) = q {
            params.push(("q", q));
        }
        if let Some(ref r) = rows_str {
            params.push(("rows", r));
        }
        if let Some(ref s) = start_str {
            params.push(("start", s));
        }
        if let Some(fq) = fq {
            params.push(("fq", fq));
        }

        self.call_action("package_search", &params).await
    }

    /// Retrieve a specific dataset by its ID or name
    ///
    /// Fetches complete metadata for a single dataset, including all resources,
    /// tags, organization details, and custom metadata fields.
    ///
    /// # Arguments
    ///
    /// * `id` - Dataset ID (UUID) or name (URL-friendly slug)
    ///
    /// # Returns
    ///
    /// Returns the complete dataset record with all metadata and resources.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use data_gov_ckan::{CkanClient, Configuration};
    /// # use std::sync::Arc;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = CkanClient::new(Arc::new(Configuration {
    /// #     base_path: "https://open.canada.ca/data/en/api/3".to_string(),
    /// #     user_agent: Some("test".to_string()),
    /// #     ..Configuration::default()
    /// # }));
    ///
    /// // Get dataset by name
    /// let dataset = client.package_show("consumer-complaint-database").await?;
    ///
    /// println!("Dataset: {}", dataset.title.unwrap_or_default());
    /// println!("Description: {}", dataset.notes.unwrap_or_default());
    ///
    /// // List all resources in the dataset
    /// println!("Resources:");
    /// for resource in dataset.resources.unwrap_or_default() {
    ///     println!("  - {} ({})",
    ///         resource.name.as_deref().unwrap_or("Unnamed"),
    ///         resource.format.as_deref().unwrap_or("Unknown format")
    ///     );
    ///
    ///     if let Some(url) = resource.url {
    ///         println!("    URL: {}", url);
    ///     }
    ///
    ///     if let Some(size) = resource.size {
    ///         println!("    Size: {} bytes", size);
    ///     }
    /// }
    ///
    /// // Show dataset tags
    /// if let Some(tags) = dataset.tags {
    ///     let tag_names: Vec<String> = tags.into_iter()
    ///         .filter_map(|tag| tag.display_name)
    ///         .collect();
    ///     println!("Tags: {}", tag_names.join(", "));
    /// }
    ///
    /// // Show organization
    /// if let Some(org) = dataset.organization {
    ///     println!("Organization: {}", org.title.unwrap_or_default());
    /// }
    ///
    /// // Get dataset by UUID
    /// let dataset_by_id = client.package_show("a1b2c3d4-e5f6-7890-abcd-ef1234567890").await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Error Handling
    ///
    /// ```rust,ignore
    /// match client.package_show("nonexistent-dataset").await {
    ///     Ok(dataset) => {
    ///         println!("Found dataset: {}", dataset.title.unwrap_or_default());
    ///     },
    ///     Err(CkanError::ApiError { status: 404, .. }) => {
    ///         println!("Dataset not found");
    ///     },
    ///     Err(e) => {
    ///         println!("Other error: {}", e);
    ///     }
    /// }
    /// ```
    ///
    /// # Dataset Metadata
    ///
    /// The returned dataset includes rich metadata:
    ///
    /// - **Basic Info**: Title, description, notes, license
    /// - **Resources**: Files, APIs, documentation associated with dataset
    /// - **Organization**: Publishing agency/department information
    /// - **Tags**: Subject tags and keywords for discovery
    /// - **Temporal**: Creation date, modification date, temporal coverage
    /// - **Spatial**: Geographic coverage and bounding boxes
    /// - **Custom Fields**: Agency-specific metadata extensions
    pub async fn package_show(&self, id: &str) -> Result<models::Package, CkanError> {
        self.call_action("package_show", &[("id", id)]).await
    }

    /// List all organizations in the CKAN instance
    pub async fn organization_list(
        &self,
        sort: Option<&str>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<Vec<String>, CkanError> {
        let limit_str = limit.map(|l| l.to_string());
        let offset_str = offset.map(|o| o.to_string());

        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(sort) = sort {
            params.push(("sort", sort));
        }
        if let Some(ref l) = limit_str {
            params.push(("limit", l));
        }
        if let Some(ref o) = offset_str {
            params.push(("offset", o));
        }

        self.call_action("organization_list", &params).await
    }

    /// List all groups in the CKAN instance
    pub async fn group_list(
        &self,
        sort: Option<&str>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<Vec<String>, CkanError> {
        let limit_str = limit.map(|l| l.to_string());
        let offset_str = offset.map(|o| o.to_string());

        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(sort) = sort {
            params.push(("sort", sort));
        }
        if let Some(ref l) = limit_str {
            params.push(("limit", l));
        }
        if let Some(ref o) = offset_str {
            params.push(("offset", o));
        }

        self.call_action("group_list", &params).await
    }

    /// Get dataset autocomplete suggestions for type-ahead functionality
    ///
    /// Provides quick dataset name/title suggestions as the user types, perfect for
    /// implementing search boxes with autocomplete dropdowns.
    ///
    /// # Arguments
    ///
    /// * `incomplete` - Partial dataset name or title to search for (e.g., "climat")
    /// * `limit` - Maximum number of suggestions to return (default: 10, reasonable max: 20)
    ///
    /// # Returns
    ///
    /// Returns suggestions containing dataset names and titles that match the input.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use data_gov_ckan::{CkanClient, Configuration};
    /// # use std::sync::Arc;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = CkanClient::new(Arc::new(Configuration {
    /// #     base_path: "https://open.canada.ca/data/en/api/3".to_string(),
    /// #     user_agent: Some("test".to_string()),
    /// #     ..Configuration::default()
    /// # }));
    ///
    /// // Get suggestions as user types "elect"
    /// let suggestions = client.dataset_autocomplete(Some("elect"), Some(5)).await?;
    ///
    /// for suggestion in &suggestions {
    ///     println!("Dataset: {} - {}",
    ///         suggestion.name.as_deref().unwrap_or("Unknown"),
    ///         suggestion.title.as_deref().unwrap_or("No title"));
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # UI Integration
    ///
    /// This is designed for real-time search suggestions:
    ///
    /// ```rust,ignore
    /// // In your web frontend or CLI app
    /// async fn on_search_input_change(input: &str, client: &CkanClient) -> Result<(), Box<dyn std::error::Error>> {
    ///     if input.len() >= 2 { // Start suggesting after 2+ characters
    ///         let suggestions = client.dataset_autocomplete(Some(input), Some(10)).await?;
    ///         // Display suggestions in dropdown/list
    ///         for suggestion in &suggestions {
    ///             println!("Suggestion: {}", suggestion.title.as_deref().unwrap_or("Unknown"));
    ///         }
    ///     }
    ///     Ok(())
    /// }
    /// ```
    pub async fn dataset_autocomplete(
        &self,
        incomplete: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<models::DatasetAutocomplete>, CkanError> {
        let limit_str = limit.map(|l| l.to_string());

        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(q) = incomplete {
            params.push(("q", q));
        }
        if let Some(ref l) = limit_str {
            params.push(("limit", l));
        }

        self.call_action("package_autocomplete", &params).await
    }

    /// Get tag autocomplete suggestions
    pub async fn tag_autocomplete(
        &self,
        incomplete: Option<&str>,
        limit: Option<i32>,
        vocabulary_id: Option<&str>,
    ) -> Result<Vec<String>, CkanError> {
        let limit_str = limit.map(|l| l.to_string());

        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(q) = incomplete {
            params.push(("q", q));
        }
        if let Some(ref l) = limit_str {
            params.push(("limit", l));
        }
        if let Some(vid) = vocabulary_id {
            params.push(("vocabulary_id", vid));
        }

        self.call_action("tag_autocomplete", &params).await
    }

    /// Get user autocomplete suggestions
    pub async fn user_autocomplete(
        &self,
        q: Option<&str>,
        limit: Option<i32>,
        ignore_self: Option<bool>,
    ) -> Result<Vec<models::UserAutocomplete>, CkanError> {
        let limit_str = limit.map(|l| l.to_string());
        let ignore_self_str = ignore_self.map(|b| b.to_string());

        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(q) = q {
            params.push(("q", q));
        }
        if let Some(ref l) = limit_str {
            params.push(("limit", l));
        }
        if let Some(ref i) = ignore_self_str {
            params.push(("ignore_self", i));
        }

        self.call_action("user_autocomplete", &params).await
    }

    /// Get group autocomplete suggestions for filtering and organization
    ///
    /// Groups in CKAN represent thematic collections of datasets. This endpoint
    /// provides autocomplete functionality for group names and titles.
    ///
    /// # Arguments
    ///
    /// * `q` - Partial group name or title to search for (e.g., "agri")
    /// * `limit` - Maximum number of suggestions to return (default: 10)
    ///
    /// # Returns
    ///
    /// Returns group suggestions with names, display names, and metadata.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use data_gov_ckan::{CkanClient, Configuration};
    /// # use std::sync::Arc;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = CkanClient::new(Arc::new(Configuration {
    /// #     base_path: "https://open.canada.ca/data/en/api/3".to_string(),
    /// #     user_agent: Some("test".to_string()),
    /// #     ..Configuration::default()
    /// # }));
    ///
    /// // Find agriculture-related groups
    /// let groups = client.group_autocomplete(Some("agri"), Some(5)).await?;
    ///
    /// println!("Found {} groups", groups.len());
    /// for group in groups {
    ///     println!("Group: {} ({})",
    ///         group.title.as_deref().unwrap_or("Unknown"),
    ///         group.name.as_deref().unwrap_or("Unknown"));
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Common Use Cases
    ///
    /// ```rust,ignore
    /// // Building category filters for search UI
    /// let science_groups = client.group_autocomplete(Some("science"), Some(10)).await?;
    ///
    /// // Finding groups for dataset categorization
    /// let energy_groups = client.group_autocomplete(Some("energy"), Some(5)).await?;
    /// ```
    pub async fn group_autocomplete(
        &self,
        q: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<models::GroupAutocomplete>, CkanError> {
        let limit_str = limit.map(|l| l.to_string());

        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(q) = q {
            params.push(("q", q));
        }
        if let Some(ref l) = limit_str {
            params.push(("limit", l));
        }

        self.call_action("group_autocomplete", &params).await
    }

    /// Get organization autocomplete suggestions
    pub async fn organization_autocomplete(
        &self,
        q: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<models::OrganizationAutocomplete>, CkanError> {
        let limit_str = limit.map(|l| l.to_string());

        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(q) = q {
            params.push(("q", q));
        }
        if let Some(ref l) = limit_str {
            params.push(("limit", l));
        }

        self.call_action("organization_autocomplete", &params).await
    }

    /// Get resource format autocomplete suggestions
    pub async fn resource_format_autocomplete(
        &self,
        incomplete: Option<&str>,
        limit: Option<i32>,
    ) -> Result<Vec<String>, CkanError> {
        let limit_str = limit.map(|l| l.to_string());

        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(q) = incomplete {
            params.push(("q", q));
        }
        if let Some(ref l) = limit_str {
            params.push(("limit", l));
        }

        self.call_action("format_autocomplete", &params).await
    }
}

/// Extract a human-readable message from a CKAN `error` envelope value.
///
/// Three tiers, most specific first:
///
/// 1. The documented shape, `{ "__type": ..., "message": ... }`
///    ([`models::ErrorResponseError`]).
/// 2. A bare `message` key without the rest of the documented shape (some
///    CKAN-compatible backends may omit `__type`).
/// 3. CKAN's validation-error shape replaces `message` with per-field arrays
///    instead (`{"name_or_id": ["Missing value"], "__type": "Validation
///    Error"}`), which has no message to extract at all -- render the raw
///    error object as text rather than losing it.
fn error_envelope_message(error: &serde_json::Value) -> String {
    if let Ok(envelope) = serde_json::from_value::<models::ErrorResponseError>(error.clone()) {
        return envelope.message;
    }
    if let Some(message) = error.get("message").and_then(serde_json::Value::as_str) {
        return message.to_string();
    }
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// `Configuration::default()`'s own timeout fields
    /// (`default_configuration_sets_explicit_connect_and_request_timeouts` in
    /// `tests/unit_tests.rs`) do not prove `build_client` actually wires them
    /// into the `reqwest::Client` it returns -- a client field set to the
    /// right `Duration` and a client that was never told about it are
    /// indistinguishable by inspecting the fields alone. This calls
    /// `build_client` directly, the same function `Configuration::default()`
    /// calls with the production 10s/30s values, with a timeout short enough
    /// to prove within milliseconds rather than by waiting out 30 real
    /// seconds in the test suite.
    #[tokio::test]
    async fn build_client_applies_the_given_request_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(10)))
            .mount(&server)
            .await;

        let client = build_client(Duration::from_secs(10), Duration::from_millis(100));

        let started = std::time::Instant::now();
        let result = client.get(server.uri()).send().await;
        let elapsed = started.elapsed();

        assert!(
            result.is_err(),
            "a request past the configured timeout must error"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "expected the 100ms timeout to fire well before the mock's 10s \
             delay; took {elapsed:?}"
        );
    }
}
