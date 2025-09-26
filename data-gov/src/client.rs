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
use data_gov_ckan::{
    CkanClient,
    models::{Package, PackageSearchResult, Resource},
};

/// Async client for exploring data.gov datasets.
///
/// `DataGovClient` layers ergonomic helpers on top of the lower-level
/// [`data_gov_ckan::CkanClient`]. In addition to search and metadata lookups it
/// handles download destinations, progress reporting, and colour-aware output
/// that matches the `data-gov` CLI defaults.
#[derive(Debug)]
pub struct DataGovClient {
    ckan: CkanClient,
    config: DataGovConfig,
    http_client: reqwest::Client,
}

impl DataGovClient {
    /// Create a new DataGov client with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(DataGovConfig::new())
    }

    /// Create a new DataGov client with custom configuration
    pub fn with_config(config: DataGovConfig) -> Result<Self> {
        let ckan = CkanClient::new(config.ckan_config.clone());

        // Create HTTP client with timeout for downloads
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.download_timeout_secs))
            .user_agent(&config.user_agent)
            .build()?;

        Ok(Self {
            ckan,
            config,
            http_client,
        })
    }

    // === Search and Discovery ===

    /// Search for datasets on data.gov.
    ///
    /// # Arguments
    /// * `query` - Search terms (searches titles, descriptions, tags)
    /// * `limit` - Maximum number of results (default: 10, max: 1000)
    /// * `offset` - Number of results to skip for pagination (default: 0)
    /// * `organization` - Filter by organization name (optional)
    /// * `format` - Filter by resource format (optional, e.g., "CSV", "JSON")
    ///
    /// # Examples
    ///
    /// Basic search:
    /// ```rust,no_run
    /// # use data_gov::DataGovClient;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = DataGovClient::new()?;
    /// let results = client.search("climate data", Some(20), None, None, None).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Search with filters:
    /// ```rust,no_run
    /// # use data_gov::DataGovClient;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = DataGovClient::new()?;
    /// let results = client.search("energy", Some(10), None, Some("doe-gov"), Some("CSV")).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn search(
        &self,
        query: &str,
        limit: Option<i32>,
        offset: Option<i32>,
        organization: Option<&str>,
        format: Option<&str>,
    ) -> Result<PackageSearchResult> {
        // Build filter query for advanced filtering
        let fq = match (organization, format) {
            (Some(org), Some(fmt)) => Some(format!(
                r#"organization:"{}" AND res_format:"{}""#,
                org, fmt
            )),
            (Some(org), None) => Some(format!(r#"organization:"{}""#, org)),
            (None, Some(fmt)) => Some(format!(r#"res_format:"{}""#, fmt)),
            (None, None) => None,
        };

        let result = self
            .ckan
            .package_search(Some(query), limit, offset, fq.as_deref())
            .await?;

        Ok(result)
    }

    /// Fetch the full `package_show` payload for a dataset.
    pub async fn get_dataset(&self, dataset_id: &str) -> Result<Package> {
        let package = self.ckan.package_show(dataset_id).await?;
        Ok(package)
    }

    /// Fetch dataset name suggestions for interactive prompts.
    pub async fn autocomplete_datasets(
        &self,
        partial: &str,
        limit: Option<i32>,
    ) -> Result<Vec<String>> {
        let suggestions = self.ckan.dataset_autocomplete(Some(partial), limit).await?;
        Ok(suggestions.into_iter().filter_map(|s| s.name).collect())
    }

    /// List the publisher slugs for government organizations.
    pub async fn list_organizations(&self, limit: Option<i32>) -> Result<Vec<String>> {
        let orgs = self.ckan.organization_list(None, limit, None).await?;
        Ok(orgs)
    }

    /// Fetch organization name suggestions for interactive prompts.
    pub async fn autocomplete_organizations(
        &self,
        partial: &str,
        limit: Option<i32>,
    ) -> Result<Vec<String>> {
        let suggestions = self
            .ckan
            .organization_autocomplete(Some(partial), limit)
            .await?;
        Ok(suggestions.into_iter().filter_map(|s| s.name).collect())
    }

    // === Resource Management ===

    /// Return resources that look like downloadable files.
    ///
    /// The returned list is filtered to resources that expose a direct URL, are
    /// not marked as API endpoints, and advertise a file format.
    pub fn get_downloadable_resources(package: &Package) -> Vec<Resource> {
        package
            .resources
            .as_ref()
            .unwrap_or(&Vec::new())
            .iter()
            .filter(|resource| {
                // Has a URL and is not an API endpoint
                resource.url.is_some()
                    && resource.url_type.as_deref() != Some("api")
                    && resource.format.is_some()
            })
            .cloned()
            .collect()
    }

    /// Pick a filesystem-friendly filename for a resource download.
    pub fn get_resource_filename(resource: &Resource, fallback_name: Option<&str>) -> String {
        // Try resource name first
        if let Some(name) = &resource.name {
            if let Some(format) = &resource.format {
                if name.ends_with(&format!(".{}", format.to_lowercase())) {
                    return name.clone();
                } else {
                    return format!("{}.{}", name, format.to_lowercase());
                }
            }
            return name.clone();
        }

        // Try to extract filename from URL
        if let Some(url) = &resource.url
            && let Ok(parsed_url) = Url::parse(url)
            && let Some(mut segments) = parsed_url.path_segments()
            && let Some(filename) = segments.next_back()
            && !filename.is_empty()
            && filename.contains('.')
        {
            return filename.to_string();
        }

        // Use fallback with format extension
        let base_name = fallback_name.unwrap_or("data");
        if let Some(format) = &resource.format {
            format!("{}.{}", base_name, format.to_lowercase())
        } else {
            format!("{}.dat", base_name)
        }
    }

    // === File Downloads ===

    /// Download a single resource into the dataset-specific directory.
    ///
    /// Returns the path where the file was saved.
    pub async fn download_dataset_resource(
        &self,
        resource: &Resource,
        dataset_name: &str,
    ) -> Result<PathBuf> {
        let dataset_dir = self.config.get_dataset_download_dir(dataset_name);
        let filename = Self::get_resource_filename(resource, None);
        let output_path = dataset_dir.join(filename);

        let url = match resource.url.as_deref() {
            Some(url) => url,
            None => {
                if let Some(reporter) = self.config.status_reporter.as_ref() {
                    let event = DownloadFailed {
                        resource_name: resource.name.clone(),
                        dataset_name: Some(dataset_name.to_string()),
                        output_path: None,
                        error: "Resource has no URL".to_string(),
                    };
                    reporter.on_download_failed(&event);
                }
                return Err(DataGovError::resource_not_found("Resource has no URL"));
            }
        };

        Self::perform_download(
            &self.http_client,
            url,
            &output_path,
            resource.name.clone(),
            Some(dataset_name.to_string()),
            self.reporter(),
        )
        .await?;

        Ok(output_path)
    }

    /// Download a single resource to a specific path.
    ///
    /// Returns the path where the file was saved.
    pub async fn download_resource(
        &self,
        resource: &Resource,
        output_path: Option<PathBuf>,
    ) -> Result<PathBuf> {
        let url = match resource.url.as_deref() {
            Some(url) => url,
            None => {
                if let Some(reporter) = self.config.status_reporter.as_ref() {
                    let event = DownloadFailed {
                        resource_name: resource.name.clone(),
                        dataset_name: None,
                        output_path: None,
                        error: "Resource has no URL".to_string(),
                    };
                    reporter.on_download_failed(&event);
                }
                return Err(DataGovError::resource_not_found("Resource has no URL"));
            }
        };

        let output_path = match output_path {
            Some(path) => path,
            None => {
                let filename = Self::get_resource_filename(resource, None);
                self.config.get_base_download_dir().join(filename)
            }
        };

        Self::perform_download(
            &self.http_client,
            url,
            &output_path,
            resource.name.clone(),
            None,
            self.reporter(),
        )
        .await?;

        Ok(output_path)
    }

    /// Download multiple resources concurrently.
    ///
    /// Returns one [`Result`] per resource so callers can inspect partial failures.
    pub async fn download_resources(
        &self,
        resources: &[Resource],
        output_dir: Option<&Path>,
    ) -> Vec<Result<PathBuf>> {
        if resources.len() > 1 {
            if let Some(reporter) = self.config.status_reporter.as_ref() {
                let event = DownloadBatch {
                    resource_count: resources.len(),
                    dataset_name: None,
                };
                reporter.on_download_batch(&event);
            }

            let output_dir = output_dir
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| self.config.get_base_download_dir());
            let semaphore = Arc::new(tokio::sync::Semaphore::new(
                self.config.max_concurrent_downloads,
            ));
            let status_reporter = self.reporter();

            let mut futures = Vec::with_capacity(resources.len());

            for resource in resources {
                let resource = resource.clone();
                let output_dir = output_dir.clone();
                let semaphore = semaphore.clone();
                let http_client = self.http_client.clone();
                let status_reporter = status_reporter.clone();

                let future = async move {
                    let _permit = semaphore.acquire().await.unwrap();

                    let url = match resource.url.as_deref() {
                        Some(url) => url,
                        None => {
                            if let Some(reporter) = status_reporter.as_ref() {
                                let event = DownloadFailed {
                                    resource_name: resource.name.clone(),
                                    dataset_name: None,
                                    output_path: None,
                                    error: "Resource has no URL".to_string(),
                                };
                                reporter.on_download_failed(&event);
                            }
                            return Err(DataGovError::resource_not_found("Resource has no URL"));
                        }
                    };

                    let filename = DataGovClient::get_resource_filename(&resource, None);
                    let output_path = output_dir.join(&filename);

                    DataGovClient::perform_download(
                        &http_client,
                        url,
                        &output_path,
                        resource.name.clone(),
                        None,
                        status_reporter,
                    )
                    .await?;

                    Ok(output_path)
                };

                futures.push(future);
            }

            futures::future::join_all(futures).await
        } else if resources.len() == 1 {
            let resource = &resources[0];
            let output_path = match output_dir {
                Some(dir) => {
                    let filename = Self::get_resource_filename(resource, None);
                    Some(dir.join(filename))
                }
                None => None,
            };

            vec![self.download_resource(resource, output_path).await]
        } else {
            vec![]
        }
    }

    /// Download multiple resources into the dataset-specific directory.
    ///
    /// Returns one [`Result`] per resource so callers can inspect partial failures.
    pub async fn download_dataset_resources(
        &self,
        resources: &[Resource],
        dataset_name: &str,
    ) -> Vec<Result<PathBuf>> {
        if resources.len() > 1 {
            if let Some(reporter) = self.config.status_reporter.as_ref() {
                let event = DownloadBatch {
                    resource_count: resources.len(),
                    dataset_name: Some(dataset_name.to_string()),
                };
                reporter.on_download_batch(&event);
            }

            let base_dir = self.config.get_base_download_dir();
            let semaphore = Arc::new(tokio::sync::Semaphore::new(
                self.config.max_concurrent_downloads,
            ));
            let status_reporter = self.reporter();
            let mut futures = Vec::with_capacity(resources.len());

            for resource in resources {
                let resource = resource.clone();
                let dataset_name_owned = dataset_name.to_string();
                let base_dir = base_dir.clone();
                let semaphore = semaphore.clone();
                let http_client = self.http_client.clone();
                let status_reporter = status_reporter.clone();

                let future = async move {
                    let _permit = semaphore.acquire().await.unwrap();

                    let url = match resource.url.as_deref() {
                        Some(url) => url,
                        None => {
                            if let Some(reporter) = status_reporter.as_ref() {
                                let event = DownloadFailed {
                                    resource_name: resource.name.clone(),
                                    dataset_name: Some(dataset_name_owned.clone()),
                                    output_path: None,
                                    error: "Resource has no URL".to_string(),
                                };
                                reporter.on_download_failed(&event);
                            }
                            return Err(DataGovError::resource_not_found("Resource has no URL"));
                        }
                    };

                    let dataset_dir = base_dir.join(&dataset_name_owned);
                    let filename = DataGovClient::get_resource_filename(&resource, None);
                    let output_path = dataset_dir.join(&filename);

                    DataGovClient::perform_download(
                        &http_client,
                        url,
                        &output_path,
                        resource.name.clone(),
                        Some(dataset_name_owned.clone()),
                        status_reporter,
                    )
                    .await?;

                    Ok(output_path)
                };

                futures.push(future);
            }

            futures::future::join_all(futures).await
        } else if resources.len() == 1 {
            vec![
                self.download_dataset_resource(&resources[0], dataset_name)
                    .await,
            ]
        } else {
            vec![]
        }
    }

    fn reporter(&self) -> Option<Arc<dyn StatusReporter + Send + Sync>> {
        self.config.status_reporter.clone()
    }

    async fn perform_download(
        http_client: &reqwest::Client,
        url: &str,
        output_path: &Path,
        resource_name: Option<String>,
        dataset_name: Option<String>,
        status_reporter: Option<Arc<dyn StatusReporter + Send + Sync>>,
    ) -> Result<()> {
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

        if let Some(parent) = output_path.parent() {
            if let Err(err) = tokio::fs::create_dir_all(parent).await {
                notify_failure(err.to_string(), &status_reporter);
                return Err(err.into());
            }
        }

        let response = match http_client.get(url).send().await {
            Ok(resp) => resp,
            Err(err) => {
                notify_failure(err.to_string(), &status_reporter);
                return Err(err.into());
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
        let mut downloaded = 0u64;

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

            downloaded += chunk.len() as u64;

            if let Some(reporter) = status_reporter.as_ref() {
                let event = DownloadProgress {
                    resource_name: resource_name.clone(),
                    dataset_name: dataset_name.clone(),
                    output_path: output_path.to_path_buf(),
                    downloaded_bytes: downloaded,
                    total_bytes: total_size,
                };
                reporter.on_download_progress(&event);
            }
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

    /// Check if the base download directory exists and is writable
    pub async fn validate_download_dir(&self) -> Result<()> {
        let base_dir = self.config.get_base_download_dir();

        if !base_dir.exists() {
            tokio::fs::create_dir_all(&base_dir).await?;
        }

        if !base_dir.is_dir() {
            return Err(DataGovError::config_error(format!(
                "Download path is not a directory: {:?}",
                base_dir
            )));
        }

        let test_file = base_dir.join(".write_test");
        tokio::fs::write(&test_file, b"test").await?;
        tokio::fs::remove_file(&test_file).await?;

        Ok(())
    }

    /// Get the current base download directory
    pub fn download_dir(&self) -> PathBuf {
        self.config.get_base_download_dir()
    }

    /// Get the underlying CKAN client for advanced operations
    pub fn ckan_client(&self) -> &CkanClient {
        &self.ckan
    }
}

impl Default for DataGovClient {
    fn default() -> Self {
        Self::new().expect("Failed to create default DataGovClient")
    }
}
