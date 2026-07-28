use futures::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
    models::{Dataset, Distribution, Organization, SearchHit, SearchResponse},
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
    pub fn new() -> Result<Self> {
        Self::with_config(DataGovConfig::new())
    }

    /// Access the current configuration.
    pub fn config(&self) -> &DataGovConfig {
        &self.config
    }

    /// Create a new DataGov client with custom configuration.
    pub fn with_config(config: DataGovConfig) -> Result<Self> {
        let catalog = CatalogClient::new(config.catalog_config.clone());

        let allow_private = config.allow_private_network_downloads;
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.download_timeout_secs))
            .user_agent(&config.user_agent)
            .dns_resolver(util::GuardedResolver::new(allow_private))
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                let target = attempt.url().clone();
                if attempt.previous().len() > util::MAX_REDIRECT_HOPS {
                    return attempt.error(util::RefusedDestination::new(format!(
                        "download abandoned after {} redirects",
                        util::MAX_REDIRECT_HOPS
                    )));
                }
                match util::check_url_without_dns(&target, allow_private) {
                    Ok(_) => attempt.follow(),
                    Err(message) => attempt.error(util::RefusedDestination::new(format!(
                        "redirect to `{target}` refused: {message}"
                    ))),
                }
            }))
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
    pub async fn get_dataset(&self, slug: &str) -> Result<SearchHit> {
        self.catalog
            .dataset_by_slug(slug)
            .await?
            .ok_or_else(|| DataGovError::resource_not_found(format!("slug {slug} not found")))
    }

    /// Fetch the DCAT-US 3 record for a harvest-record UUID.
    pub async fn get_dataset_by_harvest_record(&self, id: &str) -> Result<Dataset> {
        Ok(self.catalog.harvest_record_transformed(id).await?)
    }

    /// Fetch dataset title suggestions for interactive prompts.
    ///
    /// Implemented as a capped full-text search; the new API does not offer a
    /// dedicated dataset-autocomplete endpoint.
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
    pub async fn list_organizations(&self, limit: Option<i32>) -> Result<Vec<String>> {
        let orgs = self.catalog.organizations().await?;
        let iter = orgs.organizations.into_iter().filter_map(|o| o.slug);
        Ok(match limit {
            Some(n) if n >= 0 => iter.take(n as usize).collect(),
            _ => iter.collect(),
        })
    }

    /// Fetch full organization records for the catalog.
    pub async fn list_organization_records(&self) -> Result<Vec<Organization>> {
        Ok(self.catalog.organizations().await?.organizations)
    }

    /// Fetch organization name suggestions matching `partial`.
    ///
    /// Implemented as a client-side case-insensitive filter over
    /// [`CatalogClient::organizations`](data_gov_catalog::CatalogClient::organizations).
    pub async fn autocomplete_organizations(
        &self,
        partial: &str,
        limit: Option<i32>,
    ) -> Result<Vec<String>> {
        let needle = partial.to_lowercase();
        let orgs = self.catalog.organizations().await?;
        let matches = orgs.organizations.into_iter().filter(|o| {
            let name_hit = o
                .name
                .as_deref()
                .is_some_and(|n| n.to_lowercase().contains(&needle));
            let slug_hit = o
                .slug
                .as_deref()
                .is_some_and(|s| s.to_lowercase().contains(&needle));
            name_hit || slug_hit
        });
        let names = matches.filter_map(|o| o.name.or(o.slug));
        Ok(match limit {
            Some(n) if n >= 0 => names.take(n as usize).collect(),
            _ => names.collect(),
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

        // Clamped here rather than only in `with_max_concurrent_downloads`,
        // because the field is `pub` and a struct literal reaches it without
        // passing through the builder. A zero-permit semaphore is never
        // closed, so `acquire()` would stay pending forever.
        let semaphore = Arc::new(tokio::sync::Semaphore::new(
            self.config.max_concurrent_downloads.max(1),
        ));

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
        // nothing behind.
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

        let response = match http_client.get(url).send().await {
            Ok(resp) => resp,
            Err(err) => {
                // A refusal decided by the redirect policy or by the DNS
                // resolver arrives as the cause of a transport error. Report
                // the reason, not "error sending request".
                let err = match util::refusal_in(&err) {
                    Some(message) => DataGovError::validation_error(message),
                    None => DataGovError::from(err),
                };
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

        let mut file = match File::create(output_path).await {
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
                    notify_failure(err.to_string(), &status_reporter);
                    return Err(err.into());
                }
            };

            if let Err(err) = file.write_all(&chunk).await {
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

    /// Check that the base download directory exists and is writable.
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

        let test_file = base_dir.join(".write_test");
        tokio::fs::write(&test_file, b"test").await?;
        tokio::fs::remove_file(&test_file).await?;

        Ok(())
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
        assert!(!tmp.path().join(".write_test").exists());
    }
}
