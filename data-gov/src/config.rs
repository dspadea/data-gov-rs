use std::path::PathBuf;
use std::sync::Arc;
use data_gov_ckan::{Configuration as CkanConfiguration, ApiKey};

/// Configuration for the Data.gov client
#[derive(Debug, Clone)]
pub struct DataGovConfig {
    /// CKAN client configuration
    pub ckan_config: Arc<CkanConfiguration>,
    /// Default download directory for files
    pub download_dir: PathBuf,
    /// User agent for HTTP requests
    pub user_agent: String,
    /// Maximum concurrent downloads
    pub max_concurrent_downloads: usize,
    /// Timeout for downloads in seconds
    pub download_timeout_secs: u64,
    /// Enable progress bars for downloads
    pub show_progress: bool,
}

impl Default for DataGovConfig {
    fn default() -> Self {
        Self {
            ckan_config: Arc::new(CkanConfiguration::default()),
            download_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            user_agent: "data-gov-rs/1.0".to_string(),
            max_concurrent_downloads: 3,
            download_timeout_secs: 300, // 5 minutes
            show_progress: true,
        }
    }
}

impl DataGovConfig {
    /// Create a new configuration for data.gov
    pub fn new() -> Self {
        Self::default()
    }

    /// Create configuration with custom download directory
    pub fn with_download_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.download_dir = dir.into();
        self
    }

    /// Add API key for higher rate limits
    pub fn with_api_key<S: Into<String>>(mut self, api_key: S) -> Self {
        let mut ckan_config = (*self.ckan_config).clone();
        ckan_config.api_key = Some(ApiKey {
            key: api_key.into(),
            prefix: None,
        });
        self.ckan_config = Arc::new(ckan_config);
        self
    }

    /// Set custom user agent
    pub fn with_user_agent<S: Into<String>>(mut self, user_agent: S) -> Self {
        self.user_agent = user_agent.into();
        let mut ckan_config = (*self.ckan_config).clone();
        ckan_config.user_agent = Some(self.user_agent.clone());
        self.ckan_config = Arc::new(ckan_config);
        self
    }

    /// Set maximum concurrent downloads
    pub fn with_max_concurrent_downloads(mut self, max: usize) -> Self {
        self.max_concurrent_downloads = max.max(1);
        self
    }

    /// Set download timeout
    pub fn with_download_timeout(mut self, timeout_secs: u64) -> Self {
        self.download_timeout_secs = timeout_secs;
        self
    }

    /// Enable or disable progress bars
    pub fn with_progress(mut self, show_progress: bool) -> Self {
        self.show_progress = show_progress;
        self
    }
}
