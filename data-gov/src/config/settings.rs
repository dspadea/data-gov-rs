//! The vocabulary of the precedence chain: which settings exist, which layer
//! supplied one, and what a front end is told about a value it could not use.
//!
//! These types are what makes a precedence bug debuggable rather than
//! mysterious. `config show` prints them, and a test asserts on them.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

/// A setting that takes part in the precedence chain.
///
/// The key is what appears in `config.toml`; the variable is what overrides
/// the file. Both come from the setting itself, so a caller never spells
/// either by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum SettingKey {
    /// Where downloads are written, before the per-dataset subdirectory.
    DownloadDir,
    /// The Catalog API base URL.
    BaseUrl,
    /// How many downloads may run at the same time.
    MaxConcurrentDownloads,
    /// How long a download may stall, in seconds.
    DownloadTimeoutSecs,
    /// The `User-Agent` sent with catalog requests and downloads.
    UserAgent,
}

impl SettingKey {
    /// Every setting, in the order a front end reports them.
    ///
    /// Iterate this rather than naming settings one at a time, so a setting
    /// added later cannot be silently skipped.
    pub const ALL: [SettingKey; 5] = [
        SettingKey::DownloadDir,
        SettingKey::BaseUrl,
        SettingKey::MaxConcurrentDownloads,
        SettingKey::DownloadTimeoutSecs,
        SettingKey::UserAgent,
    ];

    /// The key this setting has in `config.toml`.
    pub fn config_key(self) -> &'static str {
        match self {
            SettingKey::DownloadDir => "download_dir",
            SettingKey::BaseUrl => "base_url",
            SettingKey::MaxConcurrentDownloads => "max_concurrent_downloads",
            SettingKey::DownloadTimeoutSecs => "download_timeout_secs",
            SettingKey::UserAgent => "user_agent",
        }
    }

    /// The environment variable that overrides `config.toml` for this setting.
    pub fn env_var(self) -> &'static str {
        match self {
            SettingKey::DownloadDir => "DATA_GOV_DOWNLOAD_DIR",
            SettingKey::BaseUrl => "DATA_GOV_BASE_URL",
            SettingKey::MaxConcurrentDownloads => "DATA_GOV_MAX_CONCURRENT_DOWNLOADS",
            SettingKey::DownloadTimeoutSecs => "DATA_GOV_DOWNLOAD_TIMEOUT_SECS",
            SettingKey::UserAgent => "DATA_GOV_USER_AGENT",
        }
    }

    /// This setting's position in [`Self::ALL`].
    pub(crate) fn index(self) -> usize {
        match self {
            SettingKey::DownloadDir => 0,
            SettingKey::BaseUrl => 1,
            SettingKey::MaxConcurrentDownloads => 2,
            SettingKey::DownloadTimeoutSecs => 3,
            SettingKey::UserAgent => 4,
        }
    }
}

impl fmt::Display for SettingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.config_key())
    }
}

impl FromStr for SettingKey {
    type Err = UnknownSettingKey;

    /// Turn a `config.toml` key back into a setting.
    ///
    /// A near miss is refused rather than guessed at: `config set download-dir`
    /// should say so, not quietly write a key nothing reads.
    fn from_str(key: &str) -> Result<Self, Self::Err> {
        SettingKey::ALL
            .into_iter()
            .find(|setting| setting.config_key() == key)
            .ok_or_else(|| UnknownSettingKey {
                key: key.to_owned(),
            })
    }
}

/// The error from parsing a string that names no setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSettingKey {
    /// The string that named nothing.
    pub key: String,
}

impl fmt::Display for UnknownSettingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown setting \"{}\". Known settings: ", self.key)?;
        for (position, setting) in SettingKey::ALL.iter().enumerate() {
            if position > 0 {
                f.write_str(", ")?;
            }
            f.write_str(setting.config_key())?;
        }
        Ok(())
    }
}

impl std::error::Error for UnknownSettingKey {}

/// Which layer of the precedence chain supplied a value.
///
/// Ordered highest first, so `Flag < Environment` reads as "a flag outranks
/// the environment".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum SettingSource {
    /// A command-line flag on this invocation. Always wins.
    Flag,
    /// An environment variable.
    Environment,
    /// The configuration file.
    File,
    /// The built-in default, when nothing else supplied a value.
    Default,
}

impl SettingSource {
    /// A short word naming this layer, for display.
    pub fn as_str(self) -> &'static str {
        match self {
            SettingSource::Flag => "flag",
            SettingSource::Environment => "environment",
            SettingSource::File => "file",
            SettingSource::Default => "default",
        }
    }

    /// Where a value from this layer came from, naming the variable or the
    /// file rather than the layer, so an error message points at something
    /// the reader can edit.
    pub fn origin(self, key: SettingKey) -> String {
        match self {
            SettingSource::Flag => "a command-line flag".to_owned(),
            SettingSource::Environment => key.env_var().to_owned(),
            SettingSource::File => "config.toml".to_owned(),
            SettingSource::Default => "the built-in default".to_owned(),
        }
    }
}

impl fmt::Display for SettingSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One resolved setting: what it is, what it resolved to, and which layer
/// supplied it.
///
/// The value is rendered for display. The typed value lives on the resolved
/// [`crate::DataGovConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ResolvedSetting {
    /// Which setting this is.
    pub key: SettingKey,
    /// The value it resolved to, rendered for display.
    pub value: String,
    /// The layer that supplied the value.
    pub source: SettingSource,
}

impl fmt::Display for ResolvedSetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} = {} ({})", self.key, self.value, self.source)
    }
}

/// Something noticed while resolving configuration that is worth saying, but
/// is not a reason to stop.
///
/// A front end prints these; nothing here fails an invocation. Anything that
/// should fail returns [`crate::DataGovError::ConfigError`] instead.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigWarning {
    /// The configuration file holds a key this build does not recognise.
    ///
    /// The key is ignored, so a file written by a newer version still loads
    /// on an older binary.
    UnknownKey {
        /// The unrecognised key.
        key: String,
        /// Where the file came from, for a message that names it.
        source: String,
    },
    /// The process working directory could not be read, so the
    /// command-line-mode download directory fell back to `"."`.
    CurrentDirUnavailable {
        /// Why the read failed.
        reason: String,
    },
    /// `XDG_CONFIG_HOME` was set to a relative path.
    ///
    /// The XDG base directory specification says an invalid value must be
    /// ignored, so the platform default was used instead.
    RelativeXdgConfigHome {
        /// The value that was ignored.
        value: PathBuf,
    },
    /// No configuration directory could be determined, so no configuration
    /// file was looked for.
    ///
    /// Either the platform names none, or the environment was supplied rather
    /// than read from the process and carries no `XDG_CONFIG_HOME` - in which
    /// case falling back to the real machine's directory would defeat the
    /// isolation the caller asked for.
    NoConfigDir,
    /// An environment variable is set to bytes that are not valid Unicode, so
    /// it could not be used and the layer below supplied the value.
    ///
    /// On Unix an environment variable is a byte string. A path can hold any
    /// bytes and is read as bytes, but a URL and a `User-Agent` cannot, so a
    /// non-Unicode value for one of those is unusable. Dropping it in silence
    /// would turn an explicit override into a default with nothing to explain
    /// the difference.
    NonUnicodeEnvironmentValue {
        /// The variable that could not be read as text.
        variable: String,
    },
    /// A setting's value has leading or trailing whitespace, and the value was
    /// used exactly as given.
    ///
    /// Only [`SettingKey::DownloadDir`] reaches this: a directory whose name
    /// ends in a space is a legal path, so the whitespace cannot be assumed to
    /// be an accident and thrown away. It usually is one -
    /// `DATA_GOV_DOWNLOAD_DIR=$(cat file)` carries the trailing newline - so
    /// it is reported rather than honoured in silence. Every other string
    /// setting has no use for surrounding whitespace and is trimmed instead.
    SurroundingWhitespace {
        /// The setting whose value is padded.
        key: SettingKey,
        /// The layer the padded value came from.
        source: SettingSource,
    },
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigWarning::UnknownKey { key, source } => write!(
                f,
                "{source}: unknown setting \"{key}\", ignored. It may belong to a newer release."
            ),
            ConfigWarning::CurrentDirUnavailable { reason } => write!(
                f,
                "could not read the working directory ({reason}); downloading to \".\" instead"
            ),
            ConfigWarning::RelativeXdgConfigHome { value } => write!(
                f,
                "XDG_CONFIG_HOME is not an absolute path ({}), ignored",
                value.display()
            ),
            ConfigWarning::NoConfigDir => f.write_str(
                "no configuration directory could be determined; no configuration file was read",
            ),
            ConfigWarning::NonUnicodeEnvironmentValue { variable } => write!(
                f,
                "{variable} is not valid Unicode and cannot be used here; \
                 the value below it applies instead"
            ),
            ConfigWarning::SurroundingWhitespace { key, source } => write!(
                f,
                "{key} from {} begins or ends with whitespace, and is used exactly as given",
                source.origin(*key)
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `index` and `ALL` are two statements of the same ordering, and a
    /// resolved configuration is an array indexed by the first and built from
    /// the second. Letting them drift would hand `config show` the wrong
    /// setting silently.
    #[test]
    fn every_settings_index_addresses_its_own_slot_in_all() {
        for key in SettingKey::ALL {
            assert_eq!(
                SettingKey::ALL[key.index()],
                key,
                "{key} must sit at index {} of SettingKey::ALL",
                key.index()
            );
        }
    }

    /// The keys and variables are the published contract in #86. They are
    /// spelled out here rather than derived, so a rename has to be deliberate.
    #[test]
    fn config_keys_and_environment_variables_match_the_published_table() {
        let published = [
            (
                SettingKey::DownloadDir,
                "download_dir",
                "DATA_GOV_DOWNLOAD_DIR",
            ),
            (SettingKey::BaseUrl, "base_url", "DATA_GOV_BASE_URL"),
            (
                SettingKey::MaxConcurrentDownloads,
                "max_concurrent_downloads",
                "DATA_GOV_MAX_CONCURRENT_DOWNLOADS",
            ),
            (
                SettingKey::DownloadTimeoutSecs,
                "download_timeout_secs",
                "DATA_GOV_DOWNLOAD_TIMEOUT_SECS",
            ),
            (SettingKey::UserAgent, "user_agent", "DATA_GOV_USER_AGENT"),
        ];

        assert_eq!(published.len(), SettingKey::ALL.len());
        for (key, config_key, env_var) in published {
            assert_eq!(key.config_key(), config_key);
            assert_eq!(key.env_var(), env_var);
        }
    }

    /// Every key and every variable is distinct: two settings sharing either
    /// would make one of them unreachable.
    #[test]
    fn no_two_settings_share_a_key_or_a_variable() {
        let mut keys: Vec<&str> = SettingKey::ALL.iter().map(|k| k.config_key()).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "two settings share a config.toml key");

        let mut vars: Vec<&str> = SettingKey::ALL.iter().map(|k| k.env_var()).collect();
        vars.sort_unstable();
        let before = vars.len();
        vars.dedup();
        assert_eq!(
            before,
            vars.len(),
            "two settings share an environment variable"
        );
    }

    /// A flag outranks the environment, which outranks the file, which
    /// outranks the default. The derived ordering says so.
    #[test]
    fn setting_sources_are_ordered_highest_precedence_first() {
        assert!(SettingSource::Flag < SettingSource::Environment);
        assert!(SettingSource::Environment < SettingSource::File);
        assert!(SettingSource::File < SettingSource::Default);
    }

    #[test]
    fn an_environment_origin_names_the_variable_the_reader_must_edit() {
        assert_eq!(
            SettingSource::Environment.origin(SettingKey::MaxConcurrentDownloads),
            "DATA_GOV_MAX_CONCURRENT_DOWNLOADS"
        );
        assert_eq!(
            SettingSource::File.origin(SettingKey::MaxConcurrentDownloads),
            "config.toml"
        );
    }

    #[test]
    fn an_unknown_key_warning_names_the_key_and_the_file() {
        let warning = ConfigWarning::UnknownKey {
            key: "setting_from_a_future_release".to_owned(),
            source: "config.toml".to_owned(),
        };
        let message = warning.to_string();
        assert!(
            message.contains("setting_from_a_future_release"),
            "{message}"
        );
        assert!(message.contains("config.toml"), "{message}");
    }
}
