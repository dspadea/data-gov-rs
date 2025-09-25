use crate::colors::ColorMode;
use data_gov_ckan::{ApiKey, Configuration as CkanConfiguration};
use std::path::PathBuf;
use std::sync::Arc;

/// Operating mode for the client
#[derive(Debug, Clone, PartialEq)]
pub enum OperatingMode {
    /// Interactive REPL mode - downloads to system Downloads directory
    Interactive,
    /// Command-line mode - downloads to current directory
    CommandLine,
}

/// Configuration for the Data.gov client
#[derive(Debug, Clone)]
pub struct DataGovConfig {
    /// CKAN client configuration
    pub ckan_config: Arc<CkanConfiguration>,
    /// Operating mode (affects base download directory)
    pub mode: OperatingMode,
    /// Base download directory for files (before dataset subdirectory)
    pub base_download_dir: PathBuf,
    /// User agent for HTTP requests
    pub user_agent: String,
    /// Maximum concurrent downloads
    pub max_concurrent_downloads: usize,
    /// Timeout for downloads in seconds
    pub download_timeout_secs: u64,
    /// Enable progress bars for downloads
    pub show_progress: bool,
    /// Color mode for output
    pub color_mode: ColorMode,
}

impl Default for DataGovConfig {
    fn default() -> Self {
        Self {
            ckan_config: Arc::new(CkanConfiguration::default()),
            mode: OperatingMode::Interactive, // Default to interactive mode
            base_download_dir: Self::get_default_download_dir(),
            user_agent: "data-gov-rs/1.0".to_string(),
            max_concurrent_downloads: 3,
            download_timeout_secs: 300, // 5 minutes
            show_progress: true,
            color_mode: ColorMode::default(),
        }
    }
}

impl DataGovConfig {
    /// Get the default download directory (system Downloads folder)
    fn get_default_download_dir() -> PathBuf {
        // Try to get the user's Downloads directory
        if let Some(download_dir) = dirs::download_dir() {
            download_dir
        } else {
            // Fallback to home directory + Downloads
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            home.join("Downloads")
        }
    }

    /// Create a new configuration for data.gov
    pub fn new() -> Self {
        Self::default()
    }

    /// Create configuration with custom base download directory
    pub fn with_download_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.base_download_dir = dir.into();
        self
    }

    /// Set the operating mode
    pub fn with_mode(mut self, mode: OperatingMode) -> Self {
        self.mode = mode;
        self
    }

    /// Get the base download directory based on operating mode
    pub fn get_base_download_dir(&self) -> PathBuf {
        match self.mode {
            OperatingMode::Interactive => {
                // Use the configured base directory (usually system Downloads)
                self.base_download_dir.clone()
            }
            OperatingMode::CommandLine => {
                // Use current working directory for CLI mode
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            }
        }
    }

    /// Get the full download directory for a specific dataset
    pub fn get_dataset_download_dir(&self, dataset_name: &str) -> PathBuf {
        self.get_base_download_dir().join(dataset_name)
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

    /// Set color mode
    pub fn with_color_mode(mut self, color_mode: ColorMode) -> Self {
        self.color_mode = color_mode;
        self
    }
}
