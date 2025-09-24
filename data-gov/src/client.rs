use std::path::{Path, PathBuf};
use std::sync::Arc;
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use url::Url;

use data_gov_ckan::{CkanClient, models::{Package, Resource, PackageSearchResult}};
use crate::config::DataGovConfig;
use crate::error::{DataGovError, Result};

/// High-level client for interacting with data.gov
/// 
/// This client wraps the CKAN client and provides additional functionality
/// for downloading resources, managing files, and working with data.gov specifically.
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
    
    /// Search for datasets on data.gov
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
    /// ```rust
    /// let client = DataGovClient::new()?;
    /// let results = client.search("climate data", Some(20), None, None, None).await?;
    /// ```
    /// 
    /// Search with filters:
    /// ```rust
    /// let results = client.search("energy", Some(10), None, Some("doe-gov"), Some("CSV")).await?;
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
            (Some(org), Some(fmt)) => Some(format!(r#"organization:"{}" AND res_format:"{}""#, org, fmt)),
            (Some(org), None) => Some(format!(r#"organization:"{}""#, org)),
            (None, Some(fmt)) => Some(format!(r#"res_format:"{}""#, fmt)),
            (None, None) => None,
        };
        
        let result = self.ckan.package_search(
            Some(query),
            limit,
            offset,
            fq.as_deref(),
        ).await?;
        
        Ok(result)
    }
    
    /// Get detailed information about a dataset
    pub async fn get_dataset(&self, dataset_id: &str) -> Result<Package> {
        let package = self.ckan.package_show(dataset_id).await?;
        Ok(package)
    }
    
    /// Get autocomplete suggestions for dataset names
    pub async fn autocomplete_datasets(&self, partial: &str, limit: Option<i32>) -> Result<Vec<String>> {
        let suggestions = self.ckan.dataset_autocomplete(Some(partial), limit).await?;
        Ok(suggestions.into_iter().filter_map(|s| s.name).collect())
    }
    
    /// Get list of organizations (government agencies)
    pub async fn list_organizations(&self, limit: Option<i32>) -> Result<Vec<String>> {
        let orgs = self.ckan.organization_list(None, limit, None).await?;
        Ok(orgs)
    }
    
    /// Get autocomplete suggestions for organizations
    pub async fn autocomplete_organizations(&self, partial: &str, limit: Option<i32>) -> Result<Vec<String>> {
        let suggestions = self.ckan.organization_autocomplete(Some(partial), limit).await?;
        Ok(suggestions.into_iter().filter_map(|s| s.name).collect())
    }
    
    // === Resource Management ===
    
    /// Find downloadable resources in a dataset
    /// 
    /// Returns a list of resources that have URLs and are likely downloadable files
    pub fn get_downloadable_resources(package: &Package) -> Vec<Resource> {
        package.resources
            .as_ref()
            .unwrap_or(&Vec::new())
            .iter()
            .filter(|resource| {
                // Has a URL and is not an API endpoint
                resource.url.is_some() && 
                resource.url_type.as_deref() != Some("api") &&
                resource.format.is_some()
            })
            .cloned()
            .collect()
    }
    
    /// Get the best download filename for a resource
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
        if let Some(url) = &resource.url {
            if let Ok(parsed_url) = Url::parse(url) {
                if let Some(segments) = parsed_url.path_segments() {
                    if let Some(filename) = segments.last() {
                        if !filename.is_empty() && filename.contains('.') {
                            return filename.to_string();
                        }
                    }
                }
            }
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
    
    /// Download a resource to a file
    /// 
    /// # Arguments
    /// * `resource` - The resource to download
    /// * `output_path` - Where to save the file (if None, uses default download directory)
    /// 
    /// Returns the path where the file was saved
    pub async fn download_resource(
        &self,
        resource: &Resource,
        output_path: Option<PathBuf>,
    ) -> Result<PathBuf> {
        let url = resource.url
            .as_ref()
            .ok_or_else(|| DataGovError::resource_not_found("Resource has no URL"))?;
        
        let output_path = match output_path {
            Some(path) => path,
            None => {
                let filename = Self::get_resource_filename(resource, None);
                self.config.download_dir.join(filename)
            }
        };
        
        self.download_file(url, &output_path, resource.name.as_deref()).await?;
        Ok(output_path)
    }
    
    /// Download multiple resources concurrently
    /// 
    /// Returns a vector of results, each containing either the download path or an error
    pub async fn download_resources(
        &self,
        resources: &[Resource],
        output_dir: Option<&Path>,
    ) -> Vec<Result<PathBuf>> {
        let output_dir = output_dir.unwrap_or(&self.config.download_dir).to_path_buf();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.max_concurrent_downloads));
        
        // Create futures for all downloads
        let mut futures = Vec::new();
        
        for resource in resources {
            let resource = resource.clone();
            let output_dir = output_dir.clone();
            let semaphore = semaphore.clone();
            let http_client = self.http_client.clone();
            let config = self.config.clone();
            
            let future = async move {
                let _permit = semaphore.acquire().await.unwrap();
                
                let filename = Self::get_resource_filename(&resource, None);
                let output_path = output_dir.join(filename);
                
                // Create a temporary client for this download
                let temp_client = DataGovClient {
                    ckan: CkanClient::new(config.ckan_config.clone()),
                    config,
                    http_client,
                };
                
                temp_client.download_resource(&resource, Some(output_path)).await
            };
            
            futures.push(future);
        }
        
        // Execute all downloads concurrently
        futures::future::join_all(futures).await
    }
    
    /// Download a file from a URL with progress tracking
    async fn download_file(&self, url: &str, output_path: &Path, display_name: Option<&str>) -> Result<()> {
        // Create parent directories if they don't exist
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        
        let response = self.http_client.get(url).send().await?;
        
        if !response.status().is_success() {
            return Err(DataGovError::download_error(format!(
                "HTTP {} while downloading {}", 
                response.status(),
                url
            )));
        }
        
        let total_size = response.content_length();
        let display_name = display_name.unwrap_or("file");
        
        // Setup progress bar if enabled
        let progress_bar = if self.config.show_progress {
            let pb = ProgressBar::new(total_size.unwrap_or(0));
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
                    .unwrap_or_else(|_| ProgressStyle::default_bar())
                    .progress_chars("#>-")
            );
            pb.set_message(format!("Downloading {}", display_name));
            Some(pb)
        } else {
            None
        };
        
        let mut file = File::create(output_path).await?;
        let mut stream = response.bytes_stream();
        let mut downloaded = 0u64;
        
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            
            if let Some(pb) = &progress_bar {
                pb.set_position(downloaded);
            }
        }
        
        if let Some(pb) = progress_bar {
            pb.finish_with_message(format!("Downloaded {}", display_name));
        }
        
        Ok(())
    }
    
    // === Utility Methods ===
    
    /// Check if the download directory exists and is writable
    pub async fn validate_download_dir(&self) -> Result<()> {
        if !self.config.download_dir.exists() {
            tokio::fs::create_dir_all(&self.config.download_dir).await?;
        }
        
        if !self.config.download_dir.is_dir() {
            return Err(DataGovError::config_error(
                format!("Download path is not a directory: {:?}", self.config.download_dir)
            ));
        }
        
        // Test write permissions by creating a temporary file
        let test_file = self.config.download_dir.join(".write_test");
        tokio::fs::write(&test_file, b"test").await?;
        tokio::fs::remove_file(&test_file).await?;
        
        Ok(())
    }
    
    /// Get the current download directory
    pub fn download_dir(&self) -> &Path {
        &self.config.download_dir
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