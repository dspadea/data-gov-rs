//! The high-level data.gov client.
//!
//! [`DataGovClient`] is the entry point for the crate: it searches the
//! catalog, resolves datasets by slug or harvest record, and downloads
//! distributions to disk. It wraps [`CatalogClient`] and adds what a consumer
//! of the catalog needs but the transport should not carry - concurrency
//! limits, filename derivation, path containment, and progress reporting
//! through [`crate::ui::StatusReporter`].
//!
//! Construct one with [`DataGovClient::new`] for defaults, or
//! [`DataGovClient::with_config`] to supply a [`DataGovConfig`].

use futures::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use url::Url;

use crate::config::DataGovConfig;
use crate::error::{DataGovError, Result};
use crate::ui::{
    DownloadBatch, DownloadFailed, DownloadFinished, DownloadProgress, DownloadStarted,
    StatusReporter,
};
use crate::util;
use data_gov_catalog::{
    CatalogClient, SearchParams,
    models::{Dataset, Distribution, SearchHit, SearchResponse},
};

/// Async client for exploring data.gov datasets.
///
/// `DataGovClient` layers ergonomic helpers on top of
/// [`data_gov_catalog::CatalogClient`]. In addition to search and metadata
/// lookups it handles download destinations, progress reporting, and
/// status-reporter integration used by the `data-gov` CLI.
#[derive(Debug)]
pub struct DataGovClient {
    catalog: CatalogClient,
    config: DataGovConfig,
    http_client: reqwest::Client,
}

/// One file to fetch, and the directory it is required to land in.
///
/// `output_dir` travels with `output_path` so the transfer can confirm the
/// destination is still inside the directory the user chose, whatever the
/// filename was derived from.
struct DownloadJob<'a> {
    url: &'a str,
    output_dir: &'a Path,
    output_path: &'a Path,
    resource_name: Option<String>,
    dataset_name: Option<String>,
    allow_private_network: bool,
}

impl DataGovClient {
    /// Create a new DataGov client with default configuration.
    ///
    /// # Errors
    ///
    /// Returns [`DataGovError::ConfigError`] if the default configuration is
    /// rejected, and [`DataGovError::HttpError`] if the underlying HTTP
    /// client cannot be built - a missing or unusable TLS backend is the
    /// realistic cause.
    pub fn new() -> Result<Self> {
        Self::with_config(DataGovConfig::new())
    }

    /// Access the current configuration.
    pub fn config(&self) -> &DataGovConfig {
        &self.config
    }

    /// Create a new DataGov client with custom configuration.
    ///
    /// # Errors
    ///
    /// Returns [`DataGovError::ConfigError`] when `max_concurrent_downloads`
    /// or `download_timeout_secs` is zero. Both are `pub` fields on a struct
    /// that is not `#[non_exhaustive]`, so a struct literal can reach either
    /// one without the builder that would otherwise clamp it -- and neither
    /// zero has a useful meaning: a zero-permit semaphore never closes
    /// (#73), and a zero-second connect/read timeout fails every download
    /// immediately, in a way that reads as a network problem rather than a
    /// configuration one (#107). Rejecting here, rather than clamping, means
    /// the caller finds out at construction instead of at the first silent
    /// failure.
    pub fn with_config(config: DataGovConfig) -> Result<Self> {
        if config.max_concurrent_downloads == 0 {
            return Err(DataGovError::config_error(
                "max_concurrent_downloads must be at least 1, got 0",
            ));
        }
        if config.download_timeout_secs == 0 {
            return Err(DataGovError::config_error(
                "download_timeout_secs must be at least 1, got 0 (a zero-second \
                 connect/read timeout would fail every download immediately)",
            ));
        }

        let catalog = CatalogClient::new(config.catalog_config.clone());

        let allow_private = config.allow_private_network_downloads;
        // A stall timeout, not a deadline on the transfer. `timeout` runs from
        // the start of the connect until the body has finished, which caps how
        // large a file can be fetched rather than how long it may hang.
        let stall = std::time::Duration::from_secs(config.download_timeout_secs);
        let http_client = reqwest::Client::builder()
            .connect_timeout(stall)
            .read_timeout(stall)
            .user_agent(config.user_agent())
            .dns_resolver(util::GuardedResolver::new(allow_private))
            // Redirects are followed by `util::fetch_checked`, not here. A
            // reqwest policy runs synchronously, so it cannot resolve a name
            // and cannot judge a hop whose host is one.
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        Ok(Self {
            catalog,
            config,
            http_client,
        })
    }

    // === Search and Discovery ===

    /// Search for datasets on data.gov.
    ///
    /// # Arguments
    /// * `query` - Full-text query (searches titles, descriptions, keywords).
    ///   Pass an empty string to search without a text query.
    /// * `per_page` - Page size. Server default is 10.
    /// * `after` - Opaque cursor returned by a previous page's
    ///   [`SearchResponse::after`]. Pass `None` for the first page.
    /// * `organization` - Organization slug (e.g. `nasa`) to filter by.
    ///
    /// Pagination is cursor-based; there is no random-access offset.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use data_gov::DataGovClient;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = DataGovClient::new()?;
    /// let page = client.search("climate", Some(20), None, None).await?;
    /// let next = client.search("climate", Some(20), page.after.as_deref(), None).await?;
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`DataGovError::CatalogError`] if the Catalog API request
    /// fails, returns a non-2xx status, or answers with a body that is not a
    /// search envelope. `per_page` outside `1..=1000` is rejected before any
    /// request is made.
    pub async fn search(
        &self,
        query: &str,
        per_page: Option<i32>,
        after: Option<&str>,
        organization: Option<&str>,
    ) -> Result<SearchResponse> {
        let mut params = SearchParams::new();
        if !query.is_empty() {
            params = params.q(query);
        }
        if let Some(n) = per_page {
            params = params.per_page(n);
        }
        if let Some(cursor) = after {
            params = params.after(cursor);
        }
        if let Some(org) = organization {
            params = params.org_slug(org);
        }
        Ok(self.catalog.search(params).await?)
    }

    /// Fetch a single dataset by its data.gov slug.
    ///
    /// Returns `Err(ResourceNotFound)` if no dataset matches.
    ///
    /// # Errors
    ///
    /// Returns [`DataGovError::ResourceNotFound`] if no dataset carries that
    /// slug, which is an answer rather than a fault. Returns
    /// [`DataGovError::CatalogError`] if the lookup itself fails - network,
    /// non-2xx status other than 404, or an unparseable body - or if `slug`
    /// cannot be carried in a URL path segment.
    pub async fn get_dataset(&self, slug: &str) -> Result<SearchHit> {
        self.catalog
            .dataset_by_slug(slug)
            .await?
            .ok_or_else(|| DataGovError::resource_not_found(format!("slug {slug} not found")))
    }

    /// Fetch the DCAT-US 3 record for a harvest-record UUID.
    ///
    /// Returns `Err(ResourceNotFound)` if the harvest record has no
    /// populated transform (`source_transform` is null server-side). That is
    /// the common case, not a rare one -- see
    /// [`CatalogClient::harvest_record_transformed`] -- but this wrapper
    /// keeps a `Result<Dataset>` signature, the same shape [`Self::get_dataset`]
    /// uses for its own not-found case, rather than pushing an `Option` onto
    /// every caller.
    ///
    /// # Errors
    ///
    /// Returns [`DataGovError::ResourceNotFound`] if the harvest record has
    /// no populated transform, which is the common answer rather than a
    /// fault. Returns [`DataGovError::CatalogError`] if the lookup itself
    /// fails, or if `id` cannot be carried in a URL path segment.
    pub async fn get_dataset_by_harvest_record(&self, id: &str) -> Result<Dataset> {
        self.catalog
            .harvest_record_transformed(id)
            .await?
            .ok_or_else(|| {
                DataGovError::resource_not_found(format!(
                    "harvest record {id} has no populated transform"
                ))
            })
    }

    /// Fetch dataset title suggestions for interactive prompts.
    ///
    /// Implemented as a capped full-text search; the new API does not offer a
    /// dedicated dataset-autocomplete endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`DataGovError::CatalogError`] if the underlying search
    /// fails; this shares [`Self::search`]'s failure modes exactly, since it
    /// is implemented on top of it.
    pub async fn autocomplete_datasets(
        &self,
        partial: &str,
        limit: Option<i32>,
    ) -> Result<Vec<String>> {
        let page = self.search(partial, limit.or(Some(10)), None, None).await?;
        Ok(page
            .results
            .into_iter()
            .filter_map(|hit| hit.title)
            .collect())
    }

    /// List the publisher slugs for government organizations, capped to `limit`.
    ///
    /// # Errors
    ///
    /// Returns [`DataGovError::CatalogError`] if the organizations request
    /// fails, returns a non-2xx status, or answers with an unparseable body.
    /// A negative `limit` is treated as no limit rather than an error.
    pub async fn list_organizations(&self, limit: Option<i32>) -> Result<Vec<String>> {
        let orgs = self.catalog.organizations().await?;
        let iter = orgs.organizations.into_iter().filter_map(|o| o.slug);
        Ok(match limit {
            Some(n) if n >= 0 => iter.take(n as usize).collect(),
            _ => iter.collect(),
        })
    }

    // === Distribution Management ===

    /// Return distributions that look like downloadable files.
    ///
    /// A distribution qualifies when it carries a `downloadURL` (as opposed to
    /// API-only `accessURL` entries).
    pub fn get_downloadable_distributions(dataset: &Dataset) -> Vec<Distribution> {
        dataset
            .distribution
            .iter()
            .filter(|d| d.download_url.is_some())
            .cloned()
            .collect()
    }

    /// Pick a filesystem-friendly filename for a distribution.
    ///
    /// The result is always a single path component. `title`, `format`, and the
    /// URL all arrive in a harvested third-party record, so the derived name is
    /// reduced by [`sanitize_path_component`](crate::util::sanitize_path_component)
    /// before it is returned. A name that survives none of that reduction falls
    /// back to `data.<extension>`, carrying `index` when one was given.
    ///
    /// # Arguments
    /// * `distribution` - The distribution to generate a filename for.
    /// * `fallback_name` - Used when the distribution has no title and no URL
    ///   segment we can derive a name from.
    /// * `index` - Appended before the extension to disambiguate multi-file
    ///   batches with duplicate titles.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use data_gov::DataGovClient;
    /// # use data_gov::catalog::models::Distribution;
    /// # let distribution = Distribution {
    /// #     type_hint: None,
    /// #     title: Some("../../etc/passwd".to_string()),
    /// #     description: None,
    /// #     download_url: Some("https://example.gov/data".to_string()),
    /// #     access_url: None,
    /// #     media_type: None,
    /// #     format: Some("CSV".to_string()),
    /// #     license: None,
    /// #     described_by: None,
    /// #     described_by_type: None,
    /// # };
    /// // The title is "../../etc/passwd" and the format is "CSV".
    /// let name = DataGovClient::get_distribution_filename(&distribution, None, None);
    /// assert_eq!(name, "____etc_passwd.csv");
    /// ```
    pub fn get_distribution_filename(
        distribution: &Distribution,
        fallback_name: Option<&str>,
        index: Option<usize>,
    ) -> String {
        // Reduced here rather than at each call site, so the public helper
        // cannot hand a caller a name that escapes the directory it is joined
        // onto. Reduced before the index is applied, so a name that survives
        // none of the reduction falls back to a usable default rather than to
        // a bare index.
        let base = Self::base_filename(distribution, fallback_name);
        let mut safe = util::sanitize_path_component(&base);
        // The reduction already turns `.` into an empty string, so that arm
        // cannot fire today. It is kept because the reduction is a separate
        // public function: this is the caller stating what it will accept, not
        // an inference about what the reduction currently returns.
        if safe.is_empty() || safe == "." {
            safe = Self::default_filename(distribution);
        }

        let Some(i) = index else { return safe };
        match safe.rfind('.') {
            Some(dot) => {
                let (stem, extension) = safe.split_at(dot);
                format!("{stem}-{i}{extension}")
            }
            None => format!("{safe}-{i}"),
        }
    }

    /// Name a distribution whose own metadata reduces away to nothing.
    fn default_filename(distribution: &Distribution) -> String {
        let extension = Self::format_extension(distribution.format.as_deref())
            .unwrap_or_else(|| "dat".to_string());
        format!("data.{extension}")
    }

    /// Reduce `format` to an extension, or `None` when nothing usable is left.
    ///
    /// `format` is metadata from the same untrusted record as the title, and it
    /// is appended to a path, so it is a second way in.
    fn format_extension(format: Option<&str>) -> Option<String> {
        let extension = util::sanitize_path_component(&format?.to_lowercase());
        // As above: `.` cannot reach here, and the arm stays so this function
        // does not depend on that remaining true.
        if extension.is_empty() || extension == "." {
            return None;
        }
        Some(extension)
    }

    fn base_filename(distribution: &Distribution, fallback_name: Option<&str>) -> String {
        if let Some(title) = &distribution.title {
            return Self::apply_format_extension(title, distribution.format.as_deref());
        }
        if let Some(url) = distribution
            .download_url
            .as_deref()
            .or(distribution.access_url.as_deref())
            && let Ok(parsed) = Url::parse(url)
            && let Some(mut segments) = parsed.path_segments()
            && let Some(last) = segments.next_back()
            && !last.is_empty()
            && last.contains('.')
        {
            return last.to_string();
        }
        let stem = fallback_name.unwrap_or("data");
        match Self::format_extension(distribution.format.as_deref()) {
            Some(extension) => format!("{stem}.{extension}"),
            None => format!("{stem}.dat"),
        }
    }

    fn apply_format_extension(name: &str, format: Option<&str>) -> String {
        match Self::format_extension(format) {
            Some(extension) => {
                if name.to_lowercase().ends_with(&format!(".{extension}")) {
                    name.to_string()
                } else {
                    format!("{name}.{extension}")
                }
            }
            None => name.to_string(),
        }
    }

    // === File Downloads ===

    /// Download a single distribution to the specified directory.
    ///
    /// # Arguments
    /// * `distribution` - The distribution to download.
    /// * `output_dir` - Directory where the file will be saved. If `None`,
    ///   uses the configured base download directory.
    ///
    /// Returns the path where the file was written.
    ///
    /// # Errors
    ///
    /// Returns [`DataGovError::ValidationError`] if the distribution has no
    /// download URL, if that URL is not a permitted target, or if the
    /// derived filename would escape `output_dir`.
    /// Returns [`DataGovError::HttpError`] if the transfer fails or the body
    /// is shorter than its declared `Content-Length`, and
    /// [`DataGovError::IoError`] if the file cannot be written or renamed
    /// into place.
    pub async fn download_distribution(
        &self,
        distribution: &Distribution,
        output_dir: Option<&Path>,
    ) -> Result<PathBuf> {
        let url = match distribution.download_url.as_deref() {
            Some(url) => url,
            None => {
                if let Some(reporter) = self.config.status_reporter.as_ref() {
                    let event = DownloadFailed {
                        resource_name: distribution.title.clone(),
                        dataset_name: None,
                        output_path: None,
                        error: "Distribution has no downloadURL".to_string(),
                    };
                    reporter.on_download_failed(&event);
                }
                return Err(DataGovError::resource_not_found(
                    "Distribution has no downloadURL",
                ));
            }
        };

        let output_dir = output_dir
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.config.get_base_download_dir());
        let filename = Self::get_distribution_filename(distribution, None, None);
        let output_path = output_dir.join(filename);

        Self::perform_download(
            &self.http_client,
            DownloadJob {
                url,
                output_dir: &output_dir,
                output_path: &output_path,
                resource_name: distribution.title.clone(),
                dataset_name: None,
                allow_private_network: self.config.allow_private_network_downloads,
            },
            self.reporter(),
        )
        .await?;

        Ok(output_path)
    }

    /// Download multiple distributions concurrently.
    ///
    /// Returns one [`Result`] per distribution so callers can inspect partial
    /// failures.
    pub async fn download_distributions(
        &self,
        distributions: &[Distribution],
        output_dir: Option<&Path>,
    ) -> Vec<Result<PathBuf>> {
        if distributions.is_empty() {
            return vec![];
        }

        if distributions.len() == 1 {
            return vec![
                self.download_distribution(&distributions[0], output_dir)
                    .await,
            ];
        }

        if let Some(reporter) = self.config.status_reporter.as_ref() {
            let event = DownloadBatch {
                resource_count: distributions.len(),
                dataset_name: None,
            };
            reporter.on_download_batch(&event);
        }

        let output_dir = output_dir
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.config.get_base_download_dir());

        // `with_config` already refuses to build a client whose
        // `max_concurrent_downloads` is zero (#107), but this stays a
        // defense-in-depth backstop: a config assembled some other way must
        // still not produce a zero-permit semaphore, which is never closed
        // and would leave `acquire()` pending forever.
        let semaphore = Arc::new(tokio::sync::Semaphore::new(Self::download_permits(
            self.config.max_concurrent_downloads,
        )));

        let status_reporter = self.reporter();
        let allow_private_network = self.config.allow_private_network_downloads;
        let mut futures = Vec::with_capacity(distributions.len());

        for (index, distribution) in distributions.iter().enumerate() {
            let distribution = distribution.clone();
            let output_dir = output_dir.clone();
            let semaphore = semaphore.clone();
            let http_client = self.http_client.clone();
            let status_reporter = status_reporter.clone();

            let future = async move {
                let _permit = match semaphore.acquire().await {
                    Ok(permit) => permit,
                    Err(e) => {
                        if let Some(reporter) = status_reporter.as_ref() {
                            let event = DownloadFailed {
                                resource_name: distribution.title.clone(),
                                dataset_name: None,
                                output_path: None,
                                error: format!("Failed to acquire download slot: {e}"),
                            };
                            reporter.on_download_failed(&event);
                        }
                        return Err(DataGovError::download_error(format!(
                            "Semaphore error: {e}"
                        )));
                    }
                };

                let url = match distribution.download_url.as_deref() {
                    Some(url) => url,
                    None => {
                        if let Some(reporter) = status_reporter.as_ref() {
                            let event = DownloadFailed {
                                resource_name: distribution.title.clone(),
                                dataset_name: None,
                                output_path: None,
                                error: "Distribution has no downloadURL".to_string(),
                            };
                            reporter.on_download_failed(&event);
                        }
                        return Err(DataGovError::resource_not_found(
                            "Distribution has no downloadURL",
                        ));
                    }
                };

                let filename =
                    DataGovClient::get_distribution_filename(&distribution, None, Some(index));
                let output_path = output_dir.join(&filename);

                DataGovClient::perform_download(
                    &http_client,
                    DownloadJob {
                        url,
                        output_dir: &output_dir,
                        output_path: &output_path,
                        resource_name: distribution.title.clone(),
                        dataset_name: None,
                        allow_private_network,
                    },
                    status_reporter,
                )
                .await?;

                Ok(output_path)
            };

            futures.push(future);
        }

        futures::future::join_all(futures).await
    }

    fn reporter(&self) -> Option<Arc<dyn StatusReporter + Send + Sync>> {
        self.config.status_reporter.clone()
    }

    /// Number of concurrent download permits to allocate for `max`.
    ///
    /// Never zero. `with_config` already rejects a zero
    /// `max_concurrent_downloads` before a `DataGovClient` can exist
    /// (#107), but this stays a defense-in-depth backstop for a config
    /// assembled some other way: `Semaphore::new(0)` is never closed, so
    /// `acquire()` would stay pending forever with no error (#73).
    fn download_permits(max: usize) -> usize {
        max.max(1)
    }

    async fn perform_download(
        http_client: &reqwest::Client,
        job: DownloadJob<'_>,
        status_reporter: Option<Arc<dyn StatusReporter + Send + Sync>>,
    ) -> Result<()> {
        let DownloadJob {
            url,
            output_dir,
            output_path,
            resource_name,
            dataset_name,
            allow_private_network,
        } = job;

        let notify_failure =
            |message: String, status_reporter: &Option<Arc<dyn StatusReporter + Send + Sync>>| {
                if let Some(reporter) = status_reporter.as_ref() {
                    let event = DownloadFailed {
                        resource_name: resource_name.clone(),
                        dataset_name: dataset_name.clone(),
                        output_path: Some(output_path.to_path_buf()),
                        error: message.clone(),
                    };
                    reporter.on_download_failed(&event);
                }
            };

        // The destination is judged before anything is created on disk and
        // before the request leaves, so a refused URL costs nothing and leaves
        // nothing behind. `fetch_checked` judges it again, which is one extra
        // lookup and deliberate: it has to be safe called on its own, rather
        // than safe because this caller happened to check first.
        if let Err(err) = util::check_download_url(url, allow_private_network).await {
            notify_failure(err.to_string(), &status_reporter);
            return Err(err);
        }

        // Only the directory the user chose is created. Nothing a derived
        // filename asks for is created before that filename has been judged.
        if let Err(err) = tokio::fs::create_dir_all(output_dir).await {
            notify_failure(err.to_string(), &status_reporter);
            return Err(err.into());
        }

        if let Err(err) = util::ensure_inside(output_dir, output_path).await {
            notify_failure(err.to_string(), &status_reporter);
            return Err(err);
        }

        // Every hop of the redirect chain goes through the same check the URL
        // above did, including the last one, and no request is made for a hop
        // that has not passed it.
        let response = match util::fetch_checked(http_client, url, allow_private_network).await {
            Ok(resp) => resp,
            Err(err) => {
                notify_failure(err.to_string(), &status_reporter);
                return Err(err);
            }
        };

        if !response.status().is_success() {
            let message = format!("HTTP {} while downloading {}", response.status(), url);
            notify_failure(message.clone(), &status_reporter);
            return Err(DataGovError::download_error(message));
        }

        let total_size = response.content_length();

        if let Some(reporter) = status_reporter.as_ref() {
            let event = DownloadStarted {
                resource_name: resource_name.clone(),
                dataset_name: dataset_name.clone(),
                url: url.to_string(),
                output_path: output_path.to_path_buf(),
                total_bytes: total_size,
            };
            reporter.on_download_started(&event);
        }

        // The transfer goes to a temporary file beside the destination, so the
        // destination only ever holds a complete file. The two share a
        // directory because `rename` is atomic only within one filesystem.
        let temp_path = Self::partial_path(output_dir);
        let mut file = match File::create(&temp_path).await {
            Ok(file) => file,
            Err(err) => {
                notify_failure(err.to_string(), &status_reporter);
                return Err(err.into());
            }
        };

        let mut stream = response.bytes_stream();
        let mut progress = DownloadProgress {
            resource_name: resource_name.clone(),
            dataset_name: dataset_name.clone(),
            output_path: output_path.to_path_buf(),
            downloaded_bytes: 0,
            total_bytes: total_size,
        };

        while let Some(chunk_result) = stream.next().await {
            let chunk = match chunk_result {
                Ok(chunk) => chunk,
                Err(err) => {
                    Self::discard_partial(&temp_path).await;
                    notify_failure(err.to_string(), &status_reporter);
                    return Err(err.into());
                }
            };

            if let Err(err) = file.write_all(&chunk).await {
                Self::discard_partial(&temp_path).await;
                notify_failure(err.to_string(), &status_reporter);
                return Err(err.into());
            }

            progress.downloaded_bytes += chunk.len() as u64;

            if let Some(reporter) = status_reporter.as_ref() {
                reporter.on_download_progress(&progress);
            }
        }

        // Dropping a `tokio::fs::File` does not flush it, so without this the
        // last chunk can be lost and the transfer still reported as complete.
        if let Err(err) = file.flush().await {
            Self::discard_partial(&temp_path).await;
            notify_failure(err.to_string(), &status_reporter);
            return Err(err.into());
        }
        drop(file);

        if let Some(message) = Self::short_transfer(url, total_size, progress.downloaded_bytes) {
            Self::discard_partial(&temp_path).await;
            notify_failure(message.clone(), &status_reporter);
            return Err(DataGovError::download_error(message));
        }

        if let Err(err) = tokio::fs::rename(&temp_path, output_path).await {
            Self::discard_partial(&temp_path).await;
            notify_failure(err.to_string(), &status_reporter);
            return Err(err.into());
        }

        if let Some(reporter) = status_reporter.as_ref() {
            let event = DownloadFinished {
                resource_name,
                dataset_name,
                output_path: output_path.to_path_buf(),
            };
            reporter.on_download_finished(&event);
        }

        Ok(())
    }

    /// Say why `received` bytes do not satisfy a declared `expected` length.
    ///
    /// Returns `None` when the counts agree, or when the response declared no
    /// length at all - a chunked body has nothing to be measured against.
    ///
    /// # A backstop, not the primary check
    ///
    /// HTTP/1.1 and HTTP/2 both frame a body by its declared length, so hyper
    /// rejects a truncated body as an incomplete message before the transfer
    /// reaches this comparison. That is why it holds for every transport this
    /// client currently speaks and is still worth keeping: it is what catches a
    /// body that arrives in full but does not match what was promised, on any
    /// transport that does not frame by length. It is covered by unit tests
    /// rather than by an end-to-end one, because no HTTP version this client
    /// speaks can be made to reach it.
    fn short_transfer(url: &str, expected: Option<u64>, received: u64) -> Option<String> {
        let expected = expected?;
        if received == expected {
            return None;
        }
        Some(format!(
            "download of {url} ended after {received} of {expected} bytes"
        ))
    }

    /// A temporary path in `output_dir` for a transfer still in progress.
    ///
    /// The name is unique per process and per call, so two transfers heading
    /// for the same destination cannot write over each other, and it is hidden
    /// and suffixed `.part` so anything left behind by a killed process reads
    /// as unfinished rather than as data.
    fn partial_path(output_dir: &Path) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        output_dir.join(format!(".data-gov-{}-{serial}.part", std::process::id()))
    }

    /// Remove a transfer that did not finish, and say so if that fails.
    ///
    /// Best effort by design: the transfer has already failed, and being
    /// unable to tidy up is not a second failure to report to the caller. It
    /// is still worth saying out loud, because the alternative is a `.part`
    /// file nobody can account for.
    async fn discard_partial(temp_path: &Path) {
        match tokio::fs::remove_file(temp_path).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => eprintln!(
                "data-gov: could not remove the unfinished download {}: {err}",
                temp_path.display()
            ),
        }
    }

    /// Check that the base download directory exists and is writable.
    ///
    /// Callers may run this concurrently against the same directory -- the
    /// MCP server does, on every download where `outputDir` is omitted --
    /// so the write-test probe must never share a name across calls (#112).
    ///
    /// # Errors
    ///
    /// Returns [`DataGovError::ConfigError`] if the path exists but is not a
    /// directory, and [`DataGovError::IoError`] if the directory cannot be
    /// created, or if the write probe cannot be written or removed - which
    /// is what proves the directory is writable rather than merely present.
    pub async fn validate_download_dir(&self) -> Result<()> {
        let base_dir = self.config.get_base_download_dir();

        if !base_dir.exists() {
            tokio::fs::create_dir_all(&base_dir).await?;
        }

        if !base_dir.is_dir() {
            return Err(DataGovError::config_error(format!(
                "Download path is not a directory: {base_dir:?}"
            )));
        }

        let test_file = base_dir.join(Self::probe_file_name());
        tokio::fs::write(&test_file, b"test").await?;
        tokio::fs::remove_file(&test_file).await?;

        Ok(())
    }

    /// A file name for `validate_download_dir`'s write-test probe.
    ///
    /// Unique per process and per call, on the same principle as
    /// [`Self::partial_path`]: two concurrent probes in the same directory
    /// must never share a name. Before this, a fixed name (`.write_test`)
    /// meant one call's `remove_file` could race another's write and see
    /// `ENOENT` for a directory that was perfectly writable (#112).
    fn probe_file_name() -> String {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        format!(".data-gov-write-test-{}-{serial}", std::process::id())
    }

    /// Get the current base download directory.
    pub fn download_dir(&self) -> PathBuf {
        self.config.get_base_download_dir()
    }

    /// Get the underlying Catalog API client for advanced operations.
    pub fn catalog_client(&self) -> &CatalogClient {
        &self.catalog
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dist(title: Option<&str>, format: Option<&str>, url: Option<&str>) -> Distribution {
        Distribution {
            type_hint: None,
            title: title.map(str::to_string),
            description: None,
            download_url: url.map(str::to_string),
            access_url: None,
            media_type: None,
            format: format.map(str::to_string),
            license: None,
            described_by: None,
            described_by_type: None,
        }
    }

    #[test]
    fn distribution_filename_no_index() {
        let d = dist(
            Some("data"),
            Some("CSV"),
            Some("https://example.com/data.csv"),
        );
        let name = DataGovClient::get_distribution_filename(&d, None, None);
        assert_eq!(name, "data.csv");
    }

    #[test]
    fn distribution_filename_with_index() {
        let d = dist(
            Some("data"),
            Some("CSV"),
            Some("https://example.com/data.csv"),
        );
        assert_eq!(
            DataGovClient::get_distribution_filename(&d, None, Some(0)),
            "data-0.csv"
        );
        assert_eq!(
            DataGovClient::get_distribution_filename(&d, None, Some(2)),
            "data-2.csv"
        );
    }

    #[test]
    fn distribution_filename_already_has_extension() {
        let d = dist(
            Some("report.csv"),
            Some("CSV"),
            Some("https://example.com/report.csv"),
        );
        assert_eq!(
            DataGovClient::get_distribution_filename(&d, None, Some(3)),
            "report-3.csv"
        );
    }

    #[test]
    fn distribution_filename_falls_back_to_url_when_title_missing() {
        let d = dist(None, None, Some("https://example.com/downloads/report.csv"));
        assert_eq!(
            DataGovClient::get_distribution_filename(&d, None, None),
            "report.csv"
        );
    }

    #[test]
    fn distribution_filename_url_without_extension_uses_format_fallback() {
        let d = dist(None, Some("JSON"), Some("https://example.com/api/records"));
        assert_eq!(
            DataGovClient::get_distribution_filename(&d, None, None),
            "data.json"
        );
    }

    #[test]
    fn distribution_filename_no_title_no_url_returns_data_dat() {
        let d = dist(None, None, None);
        assert_eq!(
            DataGovClient::get_distribution_filename(&d, None, None),
            "data.dat"
        );
    }

    #[test]
    fn distribution_filename_uses_fallback_name() {
        let d = dist(None, Some("CSV"), None);
        assert_eq!(
            DataGovClient::get_distribution_filename(&d, Some("climate-dataset"), None),
            "climate-dataset.csv"
        );
    }

    #[test]
    fn distribution_filename_fallback_with_index_inserts_before_extension() {
        let d = dist(None, None, None);
        assert_eq!(
            DataGovClient::get_distribution_filename(&d, None, Some(2)),
            "data-2.dat"
        );
    }

    #[test]
    fn downloadable_distributions_excludes_access_only_entries() {
        let mut ds = Dataset {
            type_hint: None,
            title: None,
            description: None,
            identifier: None,
            access_level: None,
            modified: None,
            issued: None,
            publisher: None,
            contact_point: None,
            keyword: vec![],
            theme: vec![],
            distribution: vec![],
            landing_page: None,
            license: None,
            rights: None,
            spatial: None,
            temporal: None,
            accrual_periodicity: None,
            language: vec![],
            bureau_code: None,
            program_code: None,
            described_by: None,
            described_by_type: None,
            references: vec![],
            data_quality: None,
            system_of_records: None,
        };
        ds.distribution.push(dist(
            Some("csv"),
            Some("CSV"),
            Some("https://example.com/file.csv"),
        ));
        // API-only distribution — no downloadURL.
        let mut api_only = dist(Some("api"), Some("JSON"), None);
        api_only.access_url = Some("https://example.com/api".to_string());
        ds.distribution.push(api_only);

        let out = DataGovClient::get_downloadable_distributions(&ds);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title.as_deref(), Some("csv"));
    }

    #[test]
    fn a_transfer_matching_its_declared_length_is_accepted() {
        assert_eq!(
            DataGovClient::short_transfer("https://example.gov/data.csv", Some(4096), 4096),
            None,
            "a body that arrived in full is not short"
        );
    }

    #[test]
    fn a_transfer_short_of_its_declared_length_is_reported_with_both_counts() {
        let message = DataGovClient::short_transfer("https://example.gov/data.csv", Some(4096), 37)
            .expect("37 of 4096 bytes is not a completed download");
        assert!(message.contains("37"), "got: {message}");
        assert!(message.contains("4096"), "got: {message}");
        assert!(
            message.contains("https://example.gov/data.csv"),
            "the failure must name what was being fetched, got: {message}"
        );
    }

    /// More than was promised is as much a mismatch as less. A body that
    /// overruns its declared length is not the file the server described.
    #[test]
    fn a_transfer_longer_than_its_declared_length_is_reported() {
        assert!(
            DataGovClient::short_transfer("https://example.gov/data.csv", Some(10), 11).is_some(),
            "11 bytes under a declared 10 is a mismatch"
        );
    }

    #[test]
    fn a_transfer_with_no_declared_length_has_nothing_to_fall_short_of() {
        assert_eq!(
            DataGovClient::short_transfer("https://example.gov/data.csv", None, 0),
            None,
            "a chunked body declares no length, so no count can contradict it"
        );
        assert_eq!(
            DataGovClient::short_transfer("https://example.gov/data.csv", None, 9_000),
            None
        );
    }

    fn client_with_download_dir(dir: std::path::PathBuf) -> DataGovClient {
        let config = crate::config::DataGovConfig::default()
            .with_mode(crate::config::OperatingMode::Interactive)
            .with_download_dir(dir);
        DataGovClient::with_config(config).expect("test client must build")
    }

    #[tokio::test]
    async fn validate_download_dir_accepts_existing_writable_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let client = client_with_download_dir(tmp.path().to_path_buf());
        client
            .validate_download_dir()
            .await
            .expect("should succeed");
    }

    #[tokio::test]
    async fn validate_download_dir_creates_missing_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let nested = tmp.path().join("a").join("b").join("c");
        let client = client_with_download_dir(nested.clone());
        client
            .validate_download_dir()
            .await
            .expect("should succeed");
        assert!(nested.is_dir());
    }

    #[tokio::test]
    async fn validate_download_dir_rejects_path_that_is_a_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file_path = tmp.path().join("not-a-dir.txt");
        tokio::fs::write(&file_path, b"hello").await.expect("setup");

        let client = client_with_download_dir(file_path);
        let err = client.validate_download_dir().await.unwrap_err();
        match err {
            DataGovError::ConfigError { message } => {
                assert!(message.contains("not a directory"), "got: {message}");
            }
            other => panic!("expected ConfigError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn validate_download_dir_leaves_no_probe_file_behind() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let client = client_with_download_dir(tmp.path().to_path_buf());
        client
            .validate_download_dir()
            .await
            .expect("should succeed");
        let mut entries = tokio::fs::read_dir(tmp.path()).await.expect("read tempdir");
        assert!(
            entries.next_entry().await.expect("read entry").is_none(),
            "no probe file of any name should remain in the directory"
        );
    }

    /// #112: `validate_download_dir` used to probe with a fixed name,
    /// `.write_test`. `handle_download_resources` in the MCP server calls it
    /// on every download where `outputDir` is omitted -- the common case for
    /// an agent -- and the MCP run loop dispatches those concurrently since
    /// #65. Two overlapping calls on the same directory used to write the
    /// same file, then race to remove it: whichever call's `remove_file`
    /// lost the race saw `ENOENT` for a directory that was perfectly
    /// writable, and reported `isError: true` for no real reason.
    ///
    /// This spawns many concurrent calls against one directory and asserts
    /// every one succeeds -- a test that calls `validate_download_dir` once
    /// cannot observe this, because the race needs two probes racing on the
    /// same path at the same time.
    #[tokio::test]
    async fn concurrent_validate_download_dir_calls_never_collide_on_the_probe_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let client = Arc::new(client_with_download_dir(tmp.path().to_path_buf()));

        let mut tasks = Vec::new();
        for _ in 0..64 {
            let client = Arc::clone(&client);
            tasks.push(tokio::spawn(
                async move { client.validate_download_dir().await },
            ));
        }

        for (i, task) in tasks.into_iter().enumerate() {
            let result = task.await.expect("task must not panic");
            assert!(
                result.is_ok(),
                "concurrent call {i} must not fail on another call's probe file: {result:?}"
            );
        }
    }

    // === #107: with_config rejects the zero values a struct literal or a
    // clamp-free builder path can produce ===

    #[test]
    fn with_config_rejects_a_zero_download_timeout() {
        let config = crate::config::DataGovConfig {
            download_timeout_secs: 0,
            ..crate::config::DataGovConfig::default()
        };
        let err = DataGovClient::with_config(config)
            .expect_err("a zero-second connect/read timeout must be rejected, not clamped");
        match err {
            DataGovError::ConfigError { message } => {
                assert!(
                    message.contains("download_timeout_secs"),
                    "the error must name the field, got: {message}"
                );
            }
            other => panic!("expected ConfigError, got {other:?}"),
        }
    }

    #[test]
    fn with_config_rejects_a_zero_max_concurrent_downloads() {
        let config = crate::config::DataGovConfig {
            max_concurrent_downloads: 0,
            ..crate::config::DataGovConfig::default()
        };
        let err = DataGovClient::with_config(config)
            .expect_err("a zero-permit semaphore must be rejected, not built");
        match err {
            DataGovError::ConfigError { message } => {
                assert!(
                    message.contains("max_concurrent_downloads"),
                    "the error must name the field, got: {message}"
                );
            }
            other => panic!("expected ConfigError, got {other:?}"),
        }
    }

    /// The builder path and the struct-literal path must end in the same
    /// place. A clamp inside `with_max_concurrent_downloads` would hand a
    /// caller who asked for 0 a working client built from a number they
    /// never chose, with no error anywhere saying so.
    #[test]
    fn a_zero_from_the_builder_reaches_the_named_error_rather_than_being_clamped() {
        let config = crate::config::DataGovConfig::new().with_max_concurrent_downloads(0);

        let err = DataGovClient::with_config(config)
            .expect_err("a zero from the builder must be refused, not silently become 1");
        match err {
            DataGovError::ConfigError { message } => {
                assert!(
                    message.contains("max_concurrent_downloads"),
                    "the error must name the field, got: {message}"
                );
            }
            other => panic!("expected ConfigError, got {other:?}"),
        }
    }

    #[test]
    fn with_config_accepts_the_smallest_valid_values() {
        let config = crate::config::DataGovConfig {
            download_timeout_secs: 1,
            max_concurrent_downloads: 1,
            ..crate::config::DataGovConfig::default()
        };
        DataGovClient::with_config(config).expect("1 is a valid value for both fields");
    }

    /// #73's point-of-use clamp in `download_distributions` stays as a
    /// backstop even though `with_config` now refuses to build a client
    /// whose `max_concurrent_downloads` is zero: a config assembled some
    /// other way must still not produce a zero-permit semaphore, which is
    /// never closed and would stall `acquire()` forever with no error.
    #[test]
    fn download_permits_never_returns_zero() {
        assert_eq!(DataGovClient::download_permits(0), 1);
        assert_eq!(DataGovClient::download_permits(1), 1);
        assert_eq!(DataGovClient::download_permits(5), 5);
    }
}
