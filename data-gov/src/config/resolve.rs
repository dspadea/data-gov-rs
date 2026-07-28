//! The precedence chain: command-line flag, then environment variable, then
//! configuration file, then built-in default.
//!
//! A setting a flag cannot override is a bug (CLAUDE.md, "Configuration and
//! file locations"), so every setting runs through the same four layers in the
//! same order, and the result records which layer supplied it.
//!
//! Everything the resolver reads is injected: [`ConfigOverrides`] carries the
//! flags, [`ConfigEnvironment`] carries the variables, and
//! [`ConfigResolver::with_config_file`] carries the file. A resolver built
//! with [`ConfigResolver::new`] touches nothing at all, which is what lets a
//! test drive the whole chain without a home directory, a shell, or a race
//! against another test's `set_var`.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::config::{DataGovConfig, OperatingMode, default_download_dir_for};
use crate::error::{DataGovError, Result};

use super::file::{ConfigFile, ParsedConfigFile, locate_config_file};
use super::settings::{ConfigWarning, ResolvedSetting, SettingKey, SettingSource};

/// Whether an environment was read from the process or supplied by a caller.
///
/// This decides one thing: whether locating the configuration file may fall
/// back to the platform directory. A supplied environment is the whole
/// environment, so it may not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum EnvironmentOrigin {
    /// Read from the running process.
    Process,
    /// Given by a caller. Nothing outside it may be consulted.
    #[default]
    Supplied,
}

/// The environment variables the resolver is allowed to read.
///
/// Nothing here reads the process environment unless
/// [`from_process`](Self::from_process) was called, and even that reads only
/// [`READ_VARIABLES`](Self::READ_VARIABLES) - so an unrelated variable can
/// never influence a resolution, and no secret is captured in passing.
///
/// Values are kept as [`OsString`], because on Unix an environment variable is
/// a byte string and so is a path. A value that is not valid Unicode is legal
/// and reaches [`get_os`](Self::get_os) intact; [`get`](Self::get) is for the
/// settings that genuinely need text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigEnvironment {
    vars: BTreeMap<String, OsString>,
    origin: EnvironmentOrigin,
}

impl ConfigEnvironment {
    /// The variable that relocates the configuration directory.
    pub const XDG_CONFIG_HOME: &'static str = "XDG_CONFIG_HOME";

    /// Every variable this crate reads: one per setting, plus
    /// [`XDG_CONFIG_HOME`](Self::XDG_CONFIG_HOME).
    pub const READ_VARIABLES: [&'static str; 6] = [
        "DATA_GOV_DOWNLOAD_DIR",
        "DATA_GOV_BASE_URL",
        "DATA_GOV_MAX_CONCURRENT_DOWNLOADS",
        "DATA_GOV_DOWNLOAD_TIMEOUT_SECS",
        "DATA_GOV_USER_AGENT",
        Self::XDG_CONFIG_HOME,
    ];

    /// Snapshot the process environment, reading only
    /// [`READ_VARIABLES`](Self::READ_VARIABLES).
    ///
    /// Values are taken with `var_os`, so a variable holding bytes that are
    /// not valid Unicode arrives intact rather than reading as unset.
    pub fn from_process() -> Self {
        let vars = Self::READ_VARIABLES
            .iter()
            .filter_map(|name| std::env::var_os(name).map(|value| ((*name).to_owned(), value)))
            .collect();
        Self {
            vars,
            origin: EnvironmentOrigin::Process,
        }
    }

    /// An environment holding exactly these variables, and nothing else.
    ///
    /// This is the whole environment as far as the resolver is concerned. In
    /// particular, locating the configuration file will not fall back to the
    /// platform directory: set `XDG_CONFIG_HOME` here to point it somewhere,
    /// or start from [`from_process`](Self::from_process).
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<OsString>,
    {
        let vars = pairs
            .into_iter()
            .map(|(name, value)| (name.into(), value.into()))
            .collect();
        Self {
            vars,
            origin: EnvironmentOrigin::Supplied,
        }
    }

    /// The raw value of `name`, when it is set to something other than the
    /// empty string.
    ///
    /// An empty variable reads as unset. `DATA_GOV_DOWNLOAD_DIR=` in a shell
    /// profile means "I am not setting this", not "download into the empty
    /// path".
    pub fn get_os(&self, name: &str) -> Option<&OsStr> {
        self.vars
            .get(name)
            .map(OsString::as_os_str)
            .filter(|value| !value.is_empty())
    }

    /// The value of `name` as text.
    ///
    /// `None` covers both "not set" and "set to bytes that are not valid
    /// Unicode". Where the difference matters - and it does, because dropping
    /// an explicit override in silence is a wrong answer - use
    /// [`get_os`](Self::get_os) and decide for yourself.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.get_os(name).and_then(OsStr::to_str)
    }

    /// Whether the platform configuration directory may be consulted when this
    /// environment names none.
    pub(super) fn may_use_platform_config_dir(&self) -> bool {
        self.origin == EnvironmentOrigin::Process
    }
}

/// The command-line layer of the precedence chain: only what a flag set on
/// this invocation.
///
/// Every field is `Option`, and `None` means the flag was not given rather
/// than "the flag was given as nothing". Values are not checked here; they are
/// checked by [`ConfigResolver::resolve`], which can name the layer a bad
/// value came from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConfigOverrides {
    /// Where downloads are written.
    pub download_dir: Option<PathBuf>,
    /// The Catalog API base URL.
    pub base_url: Option<String>,
    /// How many downloads may run at the same time.
    pub max_concurrent_downloads: Option<usize>,
    /// How long a download may stall, in seconds.
    pub download_timeout_secs: Option<u64>,
    /// The `User-Agent` sent with catalog requests and downloads.
    pub user_agent: Option<String>,
}

impl ConfigOverrides {
    /// Set the download directory from a flag.
    pub fn with_download_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.download_dir = Some(dir.into());
        self
    }

    /// Set the Catalog API base URL from a flag.
    pub fn with_base_url<S: Into<String>>(mut self, base_url: S) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Set the concurrent-download limit from a flag.
    pub fn with_max_concurrent_downloads(mut self, max: usize) -> Self {
        self.max_concurrent_downloads = Some(max);
        self
    }

    /// Set the download stall timeout from a flag.
    pub fn with_download_timeout_secs(mut self, secs: u64) -> Self {
        self.download_timeout_secs = Some(secs);
        self
    }

    /// Set the user agent from a flag.
    pub fn with_user_agent<S: Into<String>>(mut self, user_agent: S) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }
}

/// Resolves configuration through one chain: flag, environment, file, default.
///
/// # Examples
///
/// A front end reads the real environment and the real file:
///
/// ```no_run
/// use data_gov::config::{ConfigOverrides, ConfigResolver};
/// use data_gov::OperatingMode;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let resolved = ConfigResolver::from_process()?
///     .with_flags(ConfigOverrides::default().with_download_dir("/mnt/data"))
///     .with_mode(OperatingMode::CommandLine)
///     .resolve()?;
///
/// for warning in resolved.warnings() {
///     eprintln!("Warning: {warning}");
/// }
/// let config = resolved.into_config();
/// # Ok(())
/// # }
/// ```
///
/// A test drives the same chain without touching anything:
///
/// ```
/// use data_gov::config::{ConfigEnvironment, ConfigResolver, SettingKey, SettingSource};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let resolved = ConfigResolver::new()
///     .with_environment(ConfigEnvironment::from_pairs([(
///         "DATA_GOV_MAX_CONCURRENT_DOWNLOADS",
///         "8",
///     )]))
///     .resolve()?;
///
/// assert_eq!(resolved.config().max_concurrent_downloads, 8);
/// assert_eq!(
///     resolved.source_of(SettingKey::MaxConcurrentDownloads),
///     SettingSource::Environment
/// );
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Default)]
pub struct ConfigResolver {
    flags: ConfigOverrides,
    environment: ConfigEnvironment,
    file: Option<ParsedConfigFile>,
    mode: OperatingMode,
    warnings: Vec<ConfigWarning>,
}

impl ConfigResolver {
    /// A resolver that has been told nothing.
    ///
    /// No flags, no environment, no configuration file, and
    /// [`OperatingMode::Interactive`]. Every setting resolves to its built-in
    /// default until something is added.
    pub fn new() -> Self {
        Self::default()
    }

    /// A resolver that reads the process environment and the configuration
    /// file at its standard location.
    ///
    /// This performs a blocking read of the configuration file. Call it while
    /// starting up, not from inside an `async fn`.
    ///
    /// # Errors
    ///
    /// [`DataGovError::ConfigError`] when a configuration file exists but
    /// cannot be read or parsed. An absent file is not an error.
    pub fn from_process() -> Result<Self> {
        Self::new()
            .with_environment(ConfigEnvironment::from_process())
            .load_config_file()
    }

    /// Use these command-line flags as the highest-precedence layer.
    pub fn with_flags(mut self, flags: ConfigOverrides) -> Self {
        self.flags = flags;
        self
    }

    /// Use these environment variables as the second layer.
    pub fn with_environment(mut self, environment: ConfigEnvironment) -> Self {
        self.environment = environment;
        self
    }

    /// Use this already-parsed configuration file as the third layer.
    ///
    /// Its own warnings travel through to [`ResolvedConfig::warnings`].
    pub fn with_config_file(mut self, file: ParsedConfigFile) -> Self {
        self.file = Some(file);
        self
    }

    /// Locate the configuration file from the environment and read it.
    ///
    /// This is the only method here that touches the filesystem. An absent
    /// file leaves the file layer empty, which resolves to all defaults.
    ///
    /// # Errors
    ///
    /// [`DataGovError::ConfigError`] when a file exists but cannot be read or
    /// parsed.
    pub fn load_config_file(mut self) -> Result<Self> {
        let location = locate_config_file(&self.environment);
        self.warnings.extend(location.warnings);
        if let Some(path) = location.path {
            self.file = ConfigFile::read(&path)?;
        }
        Ok(self)
    }

    /// Set the operating mode.
    ///
    /// The mode decides only what the download directory falls back to when
    /// nothing set one. A directory that any layer supplied is honoured in
    /// both modes (#53).
    pub fn with_mode(mut self, mode: OperatingMode) -> Self {
        self.mode = mode;
        self
    }

    /// Resolve every setting.
    ///
    /// Repeating this on the same resolver gives the same answer and has no
    /// other effect.
    ///
    /// # Errors
    ///
    /// [`DataGovError::ConfigError`] when a layer supplied a value that cannot
    /// work: a non-numeric count, a zero concurrency limit or timeout, an
    /// empty user agent or download directory, or a base URL that is not
    /// `http` or `https`. Every message names the setting and the layer the
    /// value came from.
    pub fn resolve(&self) -> Result<ResolvedConfig> {
        let defaults = DataGovConfig::default();
        let file = self.file.as_ref().map(|parsed| &parsed.settings);

        let mut warnings = self.warnings.clone();
        if let Some(parsed) = &self.file {
            warnings.extend(parsed.warnings.iter().cloned());
        }

        let download_dir = self.resolve_download_dir(file, &mut warnings)?;
        let base_url = self.resolve_base_url(file, &defaults, &mut warnings)?;
        let max_concurrent_downloads =
            self.resolve_max_concurrent_downloads(file, &defaults, &mut warnings)?;
        let download_timeout_secs =
            self.resolve_download_timeout_secs(file, &defaults, &mut warnings)?;
        let user_agent = self.resolve_user_agent(file, &defaults, &mut warnings)?;

        // `defaults` is the built-in layer, so it is also the right base to
        // build on: reusing it means one `CatalogConfiguration` - and one
        // `reqwest::Client` inside it - rather than two.
        let config = defaults
            .with_mode(self.mode.clone())
            .with_download_dir(download_dir.0.clone())
            .with_base_url(base_url.0.clone())
            .with_user_agent(user_agent.0.clone())
            .with_max_concurrent_downloads(max_concurrent_downloads.0)
            .with_download_timeout(download_timeout_secs.0);

        let settings = [
            setting(
                SettingKey::DownloadDir,
                download_dir.0.display(),
                download_dir.1,
            ),
            // Masked: a base URL may carry credentials for an authenticated
            // mirror, and this value is what `config show` prints.
            setting(
                SettingKey::BaseUrl,
                redact_url_credentials(&base_url.0),
                base_url.1,
            ),
            setting(
                SettingKey::MaxConcurrentDownloads,
                max_concurrent_downloads.0,
                max_concurrent_downloads.1,
            ),
            setting(
                SettingKey::DownloadTimeoutSecs,
                download_timeout_secs.0,
                download_timeout_secs.1,
            ),
            setting(SettingKey::UserAgent, user_agent.0, user_agent.1),
        ];

        Ok(ResolvedConfig {
            config,
            settings,
            warnings,
        })
    }

    /// The value `key` has in the environment layer, as text.
    ///
    /// A value set to bytes that are not valid Unicode cannot serve any of the
    /// textual settings, so it is reported and treated as unset rather than
    /// dropped in silence.
    fn env_text(&self, key: SettingKey, warnings: &mut Vec<ConfigWarning>) -> Option<&str> {
        let raw = self.environment.get_os(key.env_var())?;
        match raw.to_str() {
            Some(text) => Some(text),
            None => {
                warnings.push(ConfigWarning::NonUnicodeEnvironmentValue {
                    variable: key.env_var().to_owned(),
                });
                None
            }
        }
    }

    /// The value `key` has in the environment layer, parsed.
    ///
    /// # Errors
    ///
    /// [`DataGovError::ConfigError`] naming the variable and quoting the value
    /// when it does not parse.
    fn env_parsed<T>(&self, key: SettingKey, warnings: &mut Vec<ConfigWarning>) -> Result<Option<T>>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        match self.env_text(key, warnings) {
            None => Ok(None),
            Some(raw) => raw.parse::<T>().map(Some).map_err(|err| {
                DataGovError::config_error(format!(
                    "{key} from {}: cannot use \"{raw}\" ({err})",
                    key.env_var()
                ))
            }),
        }
    }

    /// Where downloads go, and which layer decided.
    ///
    /// When no layer supplied one, the mode decides and the value is
    /// materialised here, so what a front end reports and what a download
    /// actually uses cannot drift apart.
    fn resolve_download_dir(
        &self,
        file: Option<&ConfigFile>,
        warnings: &mut Vec<ConfigWarning>,
    ) -> Result<(PathBuf, SettingSource)> {
        let key = SettingKey::DownloadDir;
        // Read as bytes, not as text: a path may hold any bytes on Unix, and
        // reading through `to_str` would report a legal directory as unset.
        let from_env = self.environment.get_os(key.env_var()).map(PathBuf::from);
        let picked = layered(
            self.flags.download_dir.clone(),
            from_env,
            file.and_then(|file| file.download_dir.clone()),
        );

        match picked {
            Some((dir, source)) => {
                if dir.as_os_str().is_empty() {
                    return Err(empty_value(key, source));
                }
                // A directory whose name ends in a space is a legal path, so
                // the value is used exactly as given rather than trimmed the
                // way `base_url` and `user_agent` are. It is nearly always an
                // accident, so it is reported.
                if has_surrounding_whitespace(&dir) {
                    warnings.push(ConfigWarning::SurroundingWhitespace { key, source });
                }
                Ok((dir, source))
            }
            None => {
                let (dir, warning) = default_download_dir_for(&self.mode, std::env::current_dir());
                if let Some(warning) = warning {
                    warnings.push(warning);
                }
                Ok((dir, SettingSource::Default))
            }
        }
    }

    fn resolve_base_url(
        &self,
        file: Option<&ConfigFile>,
        defaults: &DataGovConfig,
        warnings: &mut Vec<ConfigWarning>,
    ) -> Result<(String, SettingSource)> {
        let key = SettingKey::BaseUrl;
        let picked = layered(
            self.flags.base_url.clone(),
            self.env_text(key, warnings).map(str::to_owned),
            file.and_then(|file| file.base_url.clone()),
        );

        let Some((base_url, source)) = picked else {
            return Ok((
                defaults.catalog_config.base_path.clone(),
                SettingSource::Default,
            ));
        };

        // Trim before validating, and keep the trimmed form. `url::Url::parse`
        // accepts surrounding whitespace and strips it, per WHATWG, so
        // validating the raw string and storing it would let "  https://x  "
        // pass and then be concatenated into every request URL - the catalog
        // client trims only trailing slashes - producing an
        // "invalid international domain name" that names nothing. Trailing
        // slashes are left alone: the catalog client already removes one when
        // it builds a URL, so rewriting them would only make what a front end
        // displays differ from what the user wrote.
        let base_url = base_url.trim().to_owned();
        if base_url.is_empty() {
            return Err(empty_value(key, source));
        }

        // Every message below quotes the value, and a base URL may legitimately
        // carry credentials for an authenticated mirror. Errors reach stderr
        // and logs, so they get the masked form.
        let shown = redact_url_credentials(&base_url);

        // WHATWG parsing also strips tabs and line breaks from *inside* a URL,
        // so a value containing one parses to a host the string does not hold.
        // Trimming the ends is not enough on its own.
        if base_url.chars().any(char::is_whitespace) {
            return Err(DataGovError::config_error(format!(
                "{key} from {}: a URL cannot contain whitespace, got \"{shown}\"",
                source.origin(key)
            )));
        }

        // A base URL that is not absolute http(s) produces a connect failure
        // on every later request, with nothing pointing back at the setting
        // that caused it.
        let parsed = url::Url::parse(&base_url).map_err(|err| {
            DataGovError::config_error(format!(
                "{key} from {}: \"{shown}\" is not a URL ({err})",
                source.origin(key)
            ))
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(DataGovError::config_error(format!(
                "{key} from {}: expected an http or https URL, got \"{shown}\"",
                source.origin(key)
            )));
        }

        Ok((base_url, source))
    }

    fn resolve_max_concurrent_downloads(
        &self,
        file: Option<&ConfigFile>,
        defaults: &DataGovConfig,
        warnings: &mut Vec<ConfigWarning>,
    ) -> Result<(usize, SettingSource)> {
        let key = SettingKey::MaxConcurrentDownloads;
        let picked = layered(
            self.flags.max_concurrent_downloads,
            self.env_parsed::<usize>(key, warnings)?,
            file.and_then(|file| file.max_concurrent_downloads),
        );

        let Some((max, source)) = picked else {
            return Ok((defaults.max_concurrent_downloads, SettingSource::Default));
        };

        // A zero-permit semaphore is never closed, so a download waits on it
        // forever with no error (#73). Refuse rather than clamp: a clamp
        // silently overrides what the caller asked for.
        if max == 0 {
            return Err(at_least_one(key, source));
        }
        Ok((max, source))
    }

    fn resolve_download_timeout_secs(
        &self,
        file: Option<&ConfigFile>,
        defaults: &DataGovConfig,
        warnings: &mut Vec<ConfigWarning>,
    ) -> Result<(u64, SettingSource)> {
        let key = SettingKey::DownloadTimeoutSecs;
        let picked = layered(
            self.flags.download_timeout_secs,
            self.env_parsed::<u64>(key, warnings)?,
            file.and_then(|file| file.download_timeout_secs),
        );

        let Some((secs, source)) = picked else {
            return Ok((defaults.download_timeout_secs, SettingSource::Default));
        };

        // A zero stall timeout fails every download at the first read, with no
        // explanation (#107).
        if secs == 0 {
            return Err(at_least_one(key, source));
        }
        Ok((secs, source))
    }

    fn resolve_user_agent(
        &self,
        file: Option<&ConfigFile>,
        defaults: &DataGovConfig,
        warnings: &mut Vec<ConfigWarning>,
    ) -> Result<(String, SettingSource)> {
        let key = SettingKey::UserAgent;
        let picked = layered(
            self.flags.user_agent.clone(),
            self.env_text(key, warnings).map(str::to_owned),
            file.and_then(|file| file.user_agent.clone()),
        );

        let Some((user_agent, source)) = picked else {
            return Ok((defaults.user_agent().to_owned(), SettingSource::Default));
        };

        // Surrounding whitespace has no meaning in a header value, and a
        // trailing newline is what `DATA_GOV_USER_AGENT=$(cat file)` produces.
        let user_agent = user_agent.trim().to_owned();
        if user_agent.is_empty() {
            return Err(empty_value(key, source));
        }

        // A control character left inside the value is not padding: it splits
        // the header. `reqwest` refuses it, but only as an opaque builder
        // error at the first request, which points at nothing.
        if user_agent.chars().any(char::is_control) {
            return Err(DataGovError::config_error(format!(
                "{key} from {}: a User-Agent cannot contain a line break or other control \
                 character",
                source.origin(key)
            )));
        }

        Ok((user_agent, source))
    }
}

/// A configuration, plus where every one of its settings came from.
///
/// The provenance is what makes a precedence bug debuggable: `config show`
/// prints it, and a caller surprised by a value can see which layer supplied
/// it without guessing.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    config: DataGovConfig,
    settings: [ResolvedSetting; SettingKey::ALL.len()],
    warnings: Vec<ConfigWarning>,
}

impl ResolvedConfig {
    /// The configuration these settings resolved to.
    pub fn config(&self) -> &DataGovConfig {
        &self.config
    }

    /// Take the configuration, leaving the provenance behind.
    pub fn into_config(self) -> DataGovConfig {
        self.config
    }

    /// Every setting, in [`SettingKey::ALL`] order, with the value it resolved
    /// to and the layer that supplied it.
    ///
    /// There is always exactly one entry per [`SettingKey`].
    pub fn settings(&self) -> &[ResolvedSetting] {
        &self.settings
    }

    /// The entry for one setting.
    pub fn setting(&self, key: SettingKey) -> &ResolvedSetting {
        // `settings` is built from `SettingKey::ALL` in order, and `index` is
        // that ordering, so this is in range for every key by construction -
        // pinned by `every_settings_index_addresses_its_own_slot_in_all`.
        &self.settings[key.index()]
    }

    /// Which layer supplied `key`.
    pub fn source_of(&self, key: SettingKey) -> SettingSource {
        self.setting(key).source
    }

    /// Non-fatal problems noticed while resolving.
    ///
    /// A front end prints these. They never stop an invocation.
    pub fn warnings(&self) -> &[ConfigWarning] {
        &self.warnings
    }
}

/// Pick the highest-precedence layer that supplied a value.
fn layered<T>(
    flag: Option<T>,
    environment: Option<T>,
    file: Option<T>,
) -> Option<(T, SettingSource)> {
    flag.map(|value| (value, SettingSource::Flag))
        .or_else(|| environment.map(|value| (value, SettingSource::Environment)))
        .or_else(|| file.map(|value| (value, SettingSource::File)))
}

/// Build a provenance entry, rendering the value for display.
fn setting<T: fmt::Display>(key: SettingKey, value: T, source: SettingSource) -> ResolvedSetting {
    ResolvedSetting {
        key,
        value: value.to_string(),
        source,
    }
}

/// Mask any credentials a URL carries, for a message or a display.
///
/// A base URL may point at an authenticated mirror, and both
/// [`ResolvedSetting::value`] and every error here are printed - `config show`
/// prints the first, stderr and any log the second. CLAUDE.md forbids a secret
/// reaching either.
///
/// The username survives when a password follows it, the way `git` and `curl`
/// render a credentialed remote, because it identifies the account without
/// granting anything. A bare userinfo component with no password is masked
/// whole: that shape is how a token is passed.
///
/// This works on the string rather than on a parsed `Url`, so a value that
/// failed to parse - the case that produces the error message - is masked too.
fn redact_url_credentials(value: &str) -> String {
    let Some(scheme_end) = value.find("://") else {
        return value.to_owned();
    };
    let authority_start = scheme_end + "://".len();
    let authority_end = value[authority_start..]
        .find(['/', '?', '#'])
        .map_or(value.len(), |offset| authority_start + offset);
    let authority = &value[authority_start..authority_end];

    let Some(at) = authority.rfind('@') else {
        return value.to_owned();
    };
    let masked = match authority[..at].split_once(':') {
        Some((user, _)) if !user.is_empty() => format!("{user}:***"),
        _ => "***".to_owned(),
    };

    format!(
        "{}{masked}{}{}",
        &value[..authority_start],
        &authority[at..],
        &value[authority_end..]
    )
}

/// Whether a path begins or ends with whitespace.
///
/// Checked on the raw bytes rather than on a lossy `to_string_lossy`, so a
/// path that is not valid UTF-8 is judged on what it actually holds.
fn has_surrounding_whitespace(path: &Path) -> bool {
    let bytes = path.as_os_str().as_encoded_bytes();
    match (bytes.first(), bytes.last()) {
        (Some(first), Some(last)) => first.is_ascii_whitespace() || last.is_ascii_whitespace(),
        _ => false,
    }
}

fn empty_value(key: SettingKey, source: SettingSource) -> DataGovError {
    DataGovError::config_error(format!(
        "{key} from {}: expected a value, got an empty one",
        source.origin(key)
    ))
}

fn at_least_one(key: SettingKey, source: SettingSource) -> DataGovError {
    DataGovError::config_error(format!(
        "{key} from {}: must be at least 1, got 0",
        source.origin(key)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The variables read from the process are exactly the settings' own
    /// variables plus `XDG_CONFIG_HOME`. A setting whose variable is missing
    /// here would work when injected and do nothing in production.
    #[test]
    fn read_variables_covers_every_setting_and_xdg_config_home() {
        for key in SettingKey::ALL {
            assert!(
                ConfigEnvironment::READ_VARIABLES.contains(&key.env_var()),
                "{key} reads {} but the process snapshot never fetches it",
                key.env_var()
            );
        }
        assert!(
            ConfigEnvironment::READ_VARIABLES.contains(&ConfigEnvironment::XDG_CONFIG_HOME),
            "the configuration directory cannot be relocated without XDG_CONFIG_HOME"
        );
        assert_eq!(
            ConfigEnvironment::READ_VARIABLES.len(),
            SettingKey::ALL.len() + 1,
            "the snapshot must read nothing beyond the settings and XDG_CONFIG_HOME"
        );
    }

    #[test]
    fn redaction_masks_a_password_and_keeps_the_user_and_the_host() {
        assert_eq!(
            redact_url_credentials("https://svc-user:s3cr3t@mirror.example.com/api"),
            "https://svc-user:***@mirror.example.com/api"
        );
    }

    #[test]
    fn redaction_masks_a_bare_userinfo_component_whole() {
        // No password means the userinfo component is itself the credential.
        assert_eq!(
            redact_url_credentials("https://gho_thisisatoken@mirror.example.com"),
            "https://***@mirror.example.com"
        );
    }

    #[test]
    fn redaction_leaves_a_url_without_credentials_alone() {
        for url in [
            "https://catalog.data.gov",
            "http://127.0.0.1:8080/search?q=x",
            "https://gateway.example.com/technology/datagov/v4",
            "not-a-url-at-all",
        ] {
            assert_eq!(redact_url_credentials(url), url, "{url} was rewritten");
        }
    }

    /// An `@` after the authority belongs to the path or the query, not to a
    /// credential, and must not be mistaken for one.
    #[test]
    fn redaction_ignores_an_at_sign_outside_the_authority() {
        for url in [
            "https://catalog.data.gov/search?q=user@example.com",
            "https://catalog.data.gov/api/dataset/a@b",
            "https://catalog.data.gov#contact@example.com",
        ] {
            assert_eq!(redact_url_credentials(url), url, "{url} was rewritten");
        }
    }

    /// The password may itself contain an `@`, so the split is on the last one
    /// inside the authority, not the first.
    #[test]
    fn redaction_splits_on_the_last_at_sign_in_the_authority() {
        assert_eq!(
            redact_url_credentials("https://user:p@ss@mirror.example.com/x"),
            "https://user:***@mirror.example.com/x"
        );
    }

    #[test]
    fn an_empty_environment_variable_reads_as_unset() {
        let env = ConfigEnvironment::from_pairs([("DATA_GOV_USER_AGENT", "")]);
        assert_eq!(env.get("DATA_GOV_USER_AGENT"), None);
    }

    #[test]
    fn layered_prefers_the_flag_then_the_environment_then_the_file() {
        assert_eq!(
            layered(Some("flag"), Some("env"), Some("file")),
            Some(("flag", SettingSource::Flag))
        );
        assert_eq!(
            layered(None, Some("env"), Some("file")),
            Some(("env", SettingSource::Environment))
        );
        assert_eq!(
            layered(None, None, Some("file")),
            Some(("file", SettingSource::File))
        );
        assert_eq!(layered::<&str>(None, None, None), None);
    }
}
