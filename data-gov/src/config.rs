use crate::ui::StatusReporter;
use data_gov_catalog::Configuration as CatalogConfiguration;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

/// Fallback user agent when `catalog_config.user_agent` holds `None`.
///
/// `CatalogConfiguration::default()` always sets `Some(..)`, so this is only
/// reached when a caller builds a `CatalogConfiguration` directly and clears
/// the field. It matches the catalog crate's own default text.
const DEFAULT_USER_AGENT: &str = concat!("data-gov-rs/", env!("CARGO_PKG_VERSION"));

/// Operating mode for the client
#[derive(Debug, Clone, PartialEq)]
pub enum OperatingMode {
    /// Interactive REPL mode - downloads to system Downloads directory
    Interactive,
    /// Command-line mode - downloads to current directory
    CommandLine,
}

/// Configuration for the Data.gov client
#[derive(Clone)]
pub struct DataGovConfig {
    /// Catalog API client configuration
    pub catalog_config: Arc<CatalogConfiguration>,
    /// Operating mode (affects base download directory)
    pub mode: OperatingMode,
    /// Base download directory for files (before dataset subdirectory)
    pub base_download_dir: PathBuf,
    /// Maximum concurrent downloads
    pub max_concurrent_downloads: usize,
    /// How long a download may stall, in seconds.
    ///
    /// This is a stall timeout, not a deadline on the whole transfer. It caps
    /// the connect phase, and it caps the wait for each read of the response
    /// body, resetting after every successful read. A large file that arrives
    /// slowly but steadily is not cut off; a connection that stops sending is.
    pub download_timeout_secs: u64,
    /// Permit downloads whose destination is on a private network.
    ///
    /// Download URLs arrive in harvested third-party metadata, so by default a
    /// download that resolves to loopback, an RFC 1918 network, a
    /// carrier-grade-NAT range, or an IPv6 unique-local address is refused.
    /// Set this to `true` when the client points at a mirror on the local
    /// network.
    ///
    /// Link-local destinations - `169.254.0.0/16` and `fe80::/10` - stay
    /// refused whatever this holds. That range carries the cloud
    /// instance-metadata services, and no mirror lives there.
    pub allow_private_network_downloads: bool,
    /// Optional status reporter for UI callbacks
    pub status_reporter: Option<Arc<dyn StatusReporter + Send + Sync>>,
}

impl fmt::Debug for DataGovConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DataGovConfig")
            .field("catalog_config", &self.catalog_config)
            .field("mode", &self.mode)
            .field("base_download_dir", &self.base_download_dir)
            .field("user_agent", &self.user_agent())
            .field("max_concurrent_downloads", &self.max_concurrent_downloads)
            .field("download_timeout_secs", &self.download_timeout_secs)
            .field(
                "allow_private_network_downloads",
                &self.allow_private_network_downloads,
            )
            .field(
                "status_reporter",
                &self
                    .status_reporter
                    .as_ref()
                    .map(|_| "Some(StatusReporter)"),
            )
            .finish()
    }
}

impl Default for DataGovConfig {
    fn default() -> Self {
        Self {
            catalog_config: Arc::new(CatalogConfiguration::default()),
            mode: OperatingMode::Interactive,
            base_download_dir: Self::get_default_download_dir(),
            max_concurrent_downloads: 3,
            download_timeout_secs: 300,
            allow_private_network_downloads: false,
            status_reporter: None,
        }
    }
}

impl DataGovConfig {
    /// Get the default download directory (system Downloads folder).
    fn get_default_download_dir() -> PathBuf {
        if let Some(download_dir) = dirs::download_dir() {
            download_dir
        } else {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            home.join("Downloads")
        }
    }

    /// Create a new configuration for data.gov.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create configuration with a custom base download directory.
    pub fn with_download_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.base_download_dir = dir.into();
        self
    }

    /// Set the operating mode.
    pub fn with_mode(mut self, mode: OperatingMode) -> Self {
        self.mode = mode;
        self
    }

    /// Get the base download directory based on operating mode.
    pub fn get_base_download_dir(&self) -> PathBuf {
        match self.mode {
            OperatingMode::Interactive => self.base_download_dir.clone(),
            OperatingMode::CommandLine => {
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            }
        }
    }

    /// Override the Catalog API base URL (e.g., for testing with a mock server).
    pub fn with_base_url<S: Into<String>>(mut self, base_url: S) -> Self {
        let mut catalog_config = (*self.catalog_config).clone();
        catalog_config.base_path = base_url.into();
        self.catalog_config = Arc::new(catalog_config);
        self
    }

    /// Set a custom user agent.
    ///
    /// `catalog_config.user_agent` is the single place this value lives (see
    /// [`Self::user_agent`]), so a catalog request and a download can never
    /// disagree about which identity they are sending (#106).
    pub fn with_user_agent<S: Into<String>>(mut self, user_agent: S) -> Self {
        let mut catalog_config = (*self.catalog_config).clone();
        catalog_config.user_agent = Some(user_agent.into());
        self.catalog_config = Arc::new(catalog_config);
        self
    }

    /// The user agent sent with catalog requests and downloads.
    ///
    /// Derived from `catalog_config.user_agent` -- the only place this value
    /// is stored -- rather than duplicated on `DataGovConfig` itself, so
    /// there is nowhere for a catalog request and a download to disagree
    /// about which identity they are sending (#106). Falls back to the crate
    /// default if a caller built `catalog_config` directly and cleared its
    /// user agent to `None`.
    pub fn user_agent(&self) -> &str {
        self.catalog_config
            .user_agent
            .as_deref()
            .unwrap_or(DEFAULT_USER_AGENT)
    }

    /// Set the maximum concurrent downloads.
    pub fn with_max_concurrent_downloads(mut self, max: usize) -> Self {
        self.max_concurrent_downloads = max.max(1);
        self
    }

    /// Set how long a download may stall before it is abandoned.
    ///
    /// See [`download_timeout_secs`](Self::download_timeout_secs): this bounds
    /// the connect phase and each read of the body, not the transfer as a
    /// whole.
    pub fn with_download_timeout(mut self, timeout_secs: u64) -> Self {
        self.download_timeout_secs = timeout_secs;
        self
    }

    /// Permit or refuse downloads whose destination is on a private network.
    ///
    /// The default is to refuse. See
    /// [`allow_private_network_downloads`](Self::allow_private_network_downloads)
    /// for the ranges this covers and for the one range it never opens.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use data_gov::DataGovConfig;
    /// // Point the client at a mirror on the local network.
    /// let config = DataGovConfig::new().with_private_network_downloads(true);
    /// assert!(config.allow_private_network_downloads);
    /// ```
    pub fn with_private_network_downloads(mut self, allow: bool) -> Self {
        self.allow_private_network_downloads = allow;
        self
    }

    /// Attach a status reporter for UI callbacks.
    pub fn with_status_reporter<R>(mut self, reporter: Arc<R>) -> Self
    where
        R: StatusReporter + Send + Sync + 'static,
    {
        self.status_reporter = Some(reporter);
        self
    }

    /// Remove any configured status reporter.
    pub fn without_status_reporter(mut self) -> Self {
        self.status_reporter = None;
        self
    }

    /// Borrow the configured status reporter.
    pub fn status_reporter(&self) -> Option<&Arc<dyn StatusReporter + Send + Sync>> {
        self.status_reporter.as_ref()
    }
}
