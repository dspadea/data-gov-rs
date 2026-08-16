//! Configuration for the data.gov client, and the chain that arrives at one.
//!
//! [`DataGovConfig`] is the settled configuration a
//! [`DataGovClient`](crate::DataGovClient) runs on. A library consumer builds
//! one directly with the `with_*` builders.
//!
//! A front end - the CLI, the MCP server, any program that persists settings -
//! uses [`ConfigResolver`] instead. It resolves every setting through one
//! chain, **command-line flag, then environment variable, then configuration
//! file, then built-in default**, and records which of the four supplied each
//! value:
//!
//! | Key in `config.toml` | Field | Environment variable |
//! |---|---|---|
//! | `download_dir` | [`DataGovConfig::base_download_dir`] | `DATA_GOV_DOWNLOAD_DIR` |
//! | `base_url` | `catalog_config.base_path` | `DATA_GOV_BASE_URL` |
//! | `max_concurrent_downloads` | [`DataGovConfig::max_concurrent_downloads`] | `DATA_GOV_MAX_CONCURRENT_DOWNLOADS` |
//! | `download_timeout_secs` | [`DataGovConfig::download_timeout_secs`] | `DATA_GOV_DOWNLOAD_TIMEOUT_SECS` |
//! | `user_agent` | [`DataGovConfig::user_agent`] | `DATA_GOV_USER_AGENT` |
//!
//! The file lives at `<config>/data-gov/config.toml`. See
//! [`config_dir`] for where `<config>` is on each platform. An absent file
//! means "all defaults" and is not an error; a key this build does not
//! recognise warns and is ignored.
//!
//! **No secret belongs in `config.toml`.** An API key lives in its own
//! mode-`0600` file.

mod file;
mod resolve;
mod settings;

pub use file::{
    CONFIG_DIR_NAME, CONFIG_FILE_NAME, ConfigFile, ConfigLocation, ParsedConfigFile, config_dir,
    locate_config_file,
};
pub use resolve::{ConfigEnvironment, ConfigOverrides, ConfigResolver, ResolvedConfig};
pub use settings::{ConfigWarning, ResolvedSetting, SettingKey, SettingSource};

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
#[derive(Debug, Clone, Default, PartialEq)]
pub enum OperatingMode {
    /// Interactive REPL mode - downloads to the system Downloads directory
    /// when no directory was chosen.
    #[default]
    Interactive,
    /// Command-line mode - downloads to the working directory when no
    /// directory was chosen.
    CommandLine,
}

/// Configuration for the Data.gov client
#[derive(Clone)]
pub struct DataGovConfig {
    /// Catalog API client configuration
    pub catalog_config: Arc<CatalogConfiguration>,
    /// Operating mode. Decides the download directory only when none was
    /// chosen.
    pub mode: OperatingMode,
    /// The base download directory, before the per-dataset subdirectory, when
    /// one was chosen.
    ///
    /// `Some(..)` is honoured in **both** operating modes. `None` means nobody
    /// chose one, and [`mode`](Self::mode) decides: the user's Downloads
    /// folder in [`OperatingMode::Interactive`], the process working directory
    /// in [`OperatingMode::CommandLine`].
    ///
    /// The distinction between "unset" and "defaulted" is what makes the
    /// precedence chain expressible, and it is what fixes #53: an eager
    /// `PathBuf` here could not be told apart from a default, so
    /// [`get_base_download_dir`](Self::get_base_download_dir) discarded it
    /// whenever the mode was `CommandLine`.
    pub base_download_dir: Option<PathBuf>,
    /// Maximum concurrent downloads.
    ///
    /// Must be at least 1.
    /// [`DataGovClient::with_config`](crate::DataGovClient::with_config)
    /// refuses a zero, which would build a semaphore with no permits and
    /// stall every download with no error (#73).
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
            base_download_dir: None,
            max_concurrent_downloads: 3,
            download_timeout_secs: 300,
            allow_private_network_downloads: false,
            status_reporter: None,
        }
    }
}

impl DataGovConfig {
    /// Create a new configuration for data.gov.
    pub fn new() -> Self {
        Self::default()
    }

    /// Choose the base download directory.
    ///
    /// The chosen directory is honoured in **both** operating modes. Before
    /// #53 this call was inert in [`OperatingMode::CommandLine`]: the flag and
    /// this builder were both accepted and then discarded in favour of the
    /// working directory.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use data_gov::{DataGovConfig, OperatingMode};
    /// # use std::path::PathBuf;
    /// let config = DataGovConfig::new()
    ///     .with_mode(OperatingMode::CommandLine)
    ///     .with_download_dir("/mnt/data");
    ///
    /// assert_eq!(config.get_base_download_dir(), PathBuf::from("/mnt/data"));
    /// ```
    pub fn with_download_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.base_download_dir = Some(dir.into());
        self
    }

    /// Set the operating mode.
    ///
    /// The mode decides the download directory only when none was chosen; see
    /// [`base_download_dir`](Self::base_download_dir).
    pub fn with_mode(mut self, mode: OperatingMode) -> Self {
        self.mode = mode;
        self
    }

    /// The directory downloads are written under, before the per-dataset
    /// subdirectory.
    ///
    /// A directory set through [`with_download_dir`](Self::with_download_dir)
    /// or through [`base_download_dir`](Self::base_download_dir) wins, in
    /// either mode. With none set, the mode decides: the user's Downloads
    /// folder in [`OperatingMode::Interactive`], the process working directory
    /// in [`OperatingMode::CommandLine`].
    ///
    /// When the working directory cannot be read, this falls back to `"."`.
    /// The fallback is always reported, through `tracing`, at `warn` (#53) --
    /// and through `tracing` only, so install a subscriber to receive it. A
    /// download that names no directory of its own comes through here, so the
    /// same warning can repeat: it goes somewhere an embedder can route,
    /// filter or silence, rather than onto a stderr the library does not own.
    pub fn get_base_download_dir(&self) -> PathBuf {
        if let Some(dir) = &self.base_download_dir {
            return dir.clone();
        }

        let (dir, warning) = default_download_dir_for(&self.mode, std::env::current_dir());
        if let Some(warning) = warning {
            tracing::warn!(
                warning = %warning,
                fallback = %dir.display(),
                "could not settle the download directory from the working directory"
            );
        }
        dir
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

    /// Set how many downloads may run at once.
    ///
    /// The value is stored as given, `0` included.
    /// [`DataGovClient::with_config`](crate::DataGovClient::with_config)
    /// refuses `0`, naming the setting, because a zero-permit semaphore
    /// never closes and would stall every download with no error (#73).
    /// Failing there beats clamping here: a clamp builds a working client
    /// from a number the caller never chose and says nothing about it. The
    /// sibling [`with_download_timeout`](Self::with_download_timeout)
    /// passes its own zero through to the same named error.
    pub fn with_max_concurrent_downloads(mut self, max: usize) -> Self {
        self.max_concurrent_downloads = max;
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

/// The user's Downloads folder, or `~/Downloads` where the platform does not
/// name one.
fn default_downloads_folder() -> PathBuf {
    if let Some(download_dir) = dirs::download_dir() {
        download_dir
    } else {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join("Downloads")
    }
}

/// The download directory to use when no layer chose one.
///
/// `current_dir` is the process working directory, or the error from trying to
/// read it. A failure there produces
/// [`ConfigWarning::CurrentDirUnavailable`] alongside the `"."` fallback, so
/// the caller can report it. Before #53 that error was discarded and `"."`
/// appeared with no explanation.
///
/// Taking `current_dir` as an argument rather than reading it is what makes
/// the failure branch testable at all: no test can make `getcwd` fail.
pub(crate) fn default_download_dir_for(
    mode: &OperatingMode,
    current_dir: std::io::Result<PathBuf>,
) -> (PathBuf, Option<ConfigWarning>) {
    match mode {
        OperatingMode::Interactive => (default_downloads_folder(), None),
        OperatingMode::CommandLine => match current_dir {
            Ok(dir) => (dir, None),
            Err(err) => (
                PathBuf::from("."),
                Some(ConfigWarning::CurrentDirUnavailable {
                    reason: err.to_string(),
                }),
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NullReporter;
    impl StatusReporter for NullReporter {}

    /// The builder is a convenience, never the enforcement. A clamp here
    /// would quietly substitute 1 for the 0 the caller asked for, hiding a
    /// value that [`crate::DataGovClient::with_config`] rejects by name -
    /// and the sibling setter `with_download_timeout` already passes its
    /// own zero straight through to that same named error.
    #[test]
    fn with_max_concurrent_downloads_keeps_the_zero_the_caller_asked_for() {
        let config = DataGovConfig::new().with_max_concurrent_downloads(0);

        assert_eq!(
            config.max_concurrent_downloads, 0,
            "the builder must not override the caller: 0 has to survive to \
             with_config, which refuses it by name"
        );
    }

    /// #77: `without_status_reporter` had zero callers, zero tests. It is
    /// the natural complement of `with_status_reporter`, which the CLI does
    /// use to wire up its UI callbacks, so it is kept rather than removed.
    /// A pure setter with two possible outcomes is cheap to fully prove.
    #[test]
    fn without_status_reporter_clears_a_previously_configured_reporter() {
        let config = DataGovConfig::new()
            .with_status_reporter(Arc::new(NullReporter))
            .without_status_reporter();
        assert!(
            config.status_reporter().is_none(),
            "without_status_reporter must clear whatever with_status_reporter set"
        );
    }

    #[test]
    fn without_status_reporter_is_a_no_op_when_none_was_configured() {
        let config = DataGovConfig::new().without_status_reporter();
        assert!(config.status_reporter().is_none());
    }

    /// A fresh configuration has chosen no directory, so the mode still
    /// decides. If `default()` filled this in eagerly, every consumer would
    /// look like one that had chosen a directory, and precedence would have
    /// nothing to work with.
    #[test]
    fn a_default_configuration_has_chosen_no_download_directory() {
        assert_eq!(DataGovConfig::default().base_download_dir, None);
    }

    #[test]
    fn command_line_mode_defaults_to_the_working_directory() {
        let (dir, warning) = default_download_dir_for(
            &OperatingMode::CommandLine,
            Ok(PathBuf::from("/somewhere/the/user/is")),
        );
        assert_eq!(dir, PathBuf::from("/somewhere/the/user/is"));
        assert_eq!(warning, None);
    }

    /// #53: the failure was swallowed by `unwrap_or_else(|_| ".".into())`, so
    /// a user whose working directory had been deleted got their files in a
    /// relative `"."` with nothing said about it.
    #[test]
    fn an_unreadable_working_directory_warns_instead_of_silently_becoming_dot() {
        let (dir, warning) = default_download_dir_for(
            &OperatingMode::CommandLine,
            Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
        );

        assert_eq!(dir, PathBuf::from("."));
        let warning = warning.expect("the failure must be reported, not discarded");
        assert!(
            matches!(warning, ConfigWarning::CurrentDirUnavailable { .. }),
            "got {warning:?}"
        );
        assert!(
            warning.to_string().contains("working directory"),
            "the message must say what could not be read, got: {warning}"
        );
    }

    /// Interactive mode never consults the working directory, so a failure to
    /// read it cannot reach the REPL's default.
    #[test]
    fn interactive_mode_ignores_the_working_directory_entirely() {
        let (from_error, warning) = default_download_dir_for(
            &OperatingMode::Interactive,
            Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
        );
        let (from_ok, _) =
            default_download_dir_for(&OperatingMode::Interactive, Ok(PathBuf::from("/elsewhere")));

        assert_eq!(from_error, from_ok);
        assert_eq!(warning, None);
        assert_ne!(from_error, PathBuf::from("/elsewhere"));
    }
}
