//! Reading `config.toml`: where it lives, and what it may hold.
//!
//! The file is optional. An absent one means "all defaults" and is not an
//! error, in the same way a 404 from the Catalog API means "no such dataset"
//! rather than "the request did not work".
//!
//! No secret is read from or written to this file. An API key lives in its own
//! mode-`0600` file; see CLAUDE.md, "Secrets".

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{DataGovError, Result};

use super::resolve::ConfigEnvironment;
use super::settings::{ConfigWarning, SettingKey};

/// The application's directory inside the platform configuration directory.
pub const CONFIG_DIR_NAME: &str = "data-gov";

/// The configuration file's name inside [`CONFIG_DIR_NAME`].
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// The settings `config.toml` may carry.
///
/// Every field is optional: a key the file omits is not a value, it is an
/// absence, and the layer below supplies it instead. That is what makes
/// precedence expressible rather than guessed at.
///
/// Keys this build does not recognise are reported through
/// [`ConfigWarning::UnknownKey`] and ignored, so a file written by a newer
/// release still loads on an older binary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct ConfigFile {
    /// Where downloads are written, before the per-dataset subdirectory.
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

impl ConfigFile {
    /// Parse TOML text.
    ///
    /// `source` names where the text came from, and appears in any error and
    /// in any warning, so a message points at something the reader can edit.
    /// Pass the file path when there is one.
    ///
    /// Unknown keys are collected into
    /// [`ParsedConfigFile::warnings`] and ignored.
    ///
    /// # Errors
    ///
    /// [`DataGovError::ConfigError`] when the text is not valid TOML, or when
    /// a recognised key holds a value of the wrong type. Both messages name
    /// `source`.
    pub fn parse(text: &str, source: &str) -> Result<ParsedConfigFile> {
        let table: toml::Table = toml::from_str(text)
            .map_err(|err| DataGovError::config_error(format!("{source}: invalid TOML: {err}")))?;

        let warnings = table
            .keys()
            .filter(|key| {
                !SettingKey::ALL
                    .iter()
                    .any(|setting| setting.config_key() == key.as_str())
            })
            .map(|key| ConfigWarning::UnknownKey {
                key: key.clone(),
                source: source.to_owned(),
            })
            .collect();

        let settings: ConfigFile = toml::Value::Table(table)
            .try_into()
            .map_err(|err| DataGovError::config_error(format!("{source}: {err}")))?;

        Ok(ParsedConfigFile { settings, warnings })
    }

    /// Read and parse the file at `path`.
    ///
    /// Returns `Ok(None)` when the file does not exist. An absent
    /// configuration file means "all defaults", not a failure.
    ///
    /// This performs a blocking read. Call it while starting up, before a
    /// Tokio runtime is driving anything, not from inside an `async fn`.
    ///
    /// # Errors
    ///
    /// [`DataGovError::ConfigError`] when the file exists but cannot be read
    /// (permissions, a directory in its place), or when its contents are not
    /// valid TOML. Every message names the path.
    pub fn read(path: &Path) -> Result<Option<ParsedConfigFile>> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(DataGovError::config_error(format!(
                    "could not read {}: {err}",
                    path.display()
                )));
            }
        };

        ConfigFile::parse(&text, &path.display().to_string()).map(Some)
    }
}

/// A parsed configuration file, and anything about it worth reporting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParsedConfigFile {
    /// The settings this build recognises.
    pub settings: ConfigFile,
    /// Keys this build does not recognise. Ignored, never fatal.
    pub warnings: Vec<ConfigWarning>,
}

/// Where the configuration file is, and anything noticed while locating it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConfigLocation {
    /// The full path to `config.toml`, when a configuration directory could be
    /// determined. `None` means there is nowhere to look, not that the file is
    /// missing.
    pub path: Option<PathBuf>,
    /// Non-fatal problems noticed while locating it.
    pub warnings: Vec<ConfigWarning>,
}

/// The directory `data-gov` keeps its configuration in.
///
/// `XDG_CONFIG_HOME` decides, when `env` has it set to an absolute path.
/// Otherwise the platform's own location is used, through the `dirs` crate:
///
/// | Platform | Directory |
/// |---|---|
/// | Linux | `$XDG_CONFIG_HOME/data-gov`, else `~/.config/data-gov` |
/// | macOS | `~/Library/Application Support/data-gov` |
/// | Windows | `%APPDATA%\data-gov` |
///
/// **macOS and Windows differ from `dirs` in one deliberate way.**
/// `dirs::config_dir()` ignores `XDG_CONFIG_HOME` on both, because neither
/// platform's own conventions know about it. This function honours it
/// everywhere, so one exported variable relocates the configuration on any
/// machine - which is what makes a container and a CI job able to isolate
/// themselves without a platform check.
///
/// **The platform fallback applies only to an environment read from the
/// process.** An environment built with
/// [`ConfigEnvironment::from_pairs`] is the whole environment, so when it
/// names no `XDG_CONFIG_HOME` the answer is "nowhere", reported through
/// [`ConfigWarning::NoConfigDir`] - never the real machine's directory. The
/// platform lookup reads `$HOME` and, failing that, the passwd database, so a
/// caller that believed it had injected everything would otherwise find itself
/// reading whatever `config.toml` the machine happens to hold.
///
/// A relative `XDG_CONFIG_HOME` is ignored, as the XDG base directory
/// specification requires, and reported through
/// [`ConfigWarning::RelativeXdgConfigHome`].
pub fn config_dir(env: &ConfigEnvironment) -> ConfigLocation {
    let mut warnings = Vec::new();

    // Read as bytes: a configuration directory is a path, and a path may hold
    // bytes that are not valid Unicode.
    let base = match env.get_os(ConfigEnvironment::XDG_CONFIG_HOME) {
        Some(value) if Path::new(value).is_absolute() => Some(PathBuf::from(value)),
        Some(value) => {
            warnings.push(ConfigWarning::RelativeXdgConfigHome {
                value: PathBuf::from(value),
            });
            platform_config_dir(env)
        }
        None => platform_config_dir(env),
    };

    match base {
        Some(base) => ConfigLocation {
            path: Some(base.join(CONFIG_DIR_NAME)),
            warnings,
        },
        None => {
            warnings.push(ConfigWarning::NoConfigDir);
            ConfigLocation {
                path: None,
                warnings,
            }
        }
    }
}

/// The platform's configuration directory, but only for an environment that
/// came from the process.
///
/// See [`config_dir`] for why a supplied environment does not get one.
fn platform_config_dir(env: &ConfigEnvironment) -> Option<PathBuf> {
    env.may_use_platform_config_dir()
        .then(dirs::config_dir)
        .flatten()
}

/// The path of `config.toml`, whether or not a file is there.
///
/// See [`config_dir`] for how the directory is chosen and for the macOS and
/// Windows note.
pub fn locate_config_file(env: &ConfigEnvironment) -> ConfigLocation {
    let mut location = config_dir(env);
    location.path = location.path.map(|dir| dir.join(CONFIG_FILE_NAME));
    location
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_parses_to_no_settings_and_no_warnings() {
        let parsed = ConfigFile::parse("", "config.toml").expect("an empty file is valid TOML");
        assert_eq!(parsed.settings, ConfigFile::default());
        assert!(parsed.warnings.is_empty());
    }

    /// Every key in the published table round-trips through the parser. A
    /// mistyped `#[serde]` name would drop one silently, which is exactly how
    /// #61 hid for a release.
    #[test]
    fn every_published_key_is_read_from_the_file() {
        let text = "\
download_dir = \"/downloads\"
base_url = \"https://example.com\"
max_concurrent_downloads = 4
download_timeout_secs = 90
user_agent = \"agent/1.0\"
";
        let parsed = ConfigFile::parse(text, "config.toml").expect("valid TOML");

        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(
            parsed.settings.download_dir,
            Some(PathBuf::from("/downloads"))
        );
        assert_eq!(
            parsed.settings.base_url.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(parsed.settings.max_concurrent_downloads, Some(4));
        assert_eq!(parsed.settings.download_timeout_secs, Some(90));
        assert_eq!(parsed.settings.user_agent.as_deref(), Some("agent/1.0"));
    }

    /// The keys the parser accepts are exactly the keys `SettingKey` names,
    /// checked one at a time so a missing field cannot hide behind a sibling.
    #[test]
    fn no_published_key_is_reported_as_unknown() {
        for key in SettingKey::ALL {
            let text = match key {
                SettingKey::DownloadDir => "download_dir = \"/x\"\n",
                SettingKey::BaseUrl => "base_url = \"https://x.example.com\"\n",
                SettingKey::MaxConcurrentDownloads => "max_concurrent_downloads = 2\n",
                SettingKey::DownloadTimeoutSecs => "download_timeout_secs = 2\n",
                SettingKey::UserAgent => "user_agent = \"x\"\n",
            };
            let parsed = ConfigFile::parse(text, "config.toml").expect("valid TOML");
            assert!(
                parsed.warnings.is_empty(),
                "{key} must be a recognised key, got {:?}",
                parsed.warnings
            );
        }
    }

    #[test]
    fn an_unreadable_path_is_an_error_naming_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A directory where a file belongs: it exists, so this is not
        // NotFound, and reading it fails.
        let err = ConfigFile::read(dir.path()).expect_err("reading a directory must fail");
        assert!(
            err.to_string().contains(&dir.path().display().to_string()),
            "got: {err}"
        );
    }

    #[test]
    fn an_absolute_xdg_config_home_decides_the_directory() {
        let env = ConfigEnvironment::from_pairs([("XDG_CONFIG_HOME", "/somewhere/config")]);
        let location = locate_config_file(&env);
        assert_eq!(
            location.path,
            Some(PathBuf::from("/somewhere/config/data-gov/config.toml"))
        );
        assert!(location.warnings.is_empty());
    }

    #[test]
    fn an_empty_xdg_config_home_is_treated_as_unset() {
        let env = ConfigEnvironment::from_pairs([("XDG_CONFIG_HOME", "")]);
        let location = locate_config_file(&env);
        assert!(
            !location
                .warnings
                .iter()
                .any(|warning| matches!(warning, ConfigWarning::RelativeXdgConfigHome { .. })),
            "an unset variable is not a relative path, got {:?}",
            location.warnings
        );
    }

    /// A supplied environment is the whole environment. Falling back to the
    /// platform directory here would read the real machine's `config.toml`
    /// for a caller that believed it had injected everything.
    #[test]
    fn a_supplied_environment_gets_no_platform_fallback() {
        for env in [
            ConfigEnvironment::from_pairs([("XDG_CONFIG_HOME", "")]),
            ConfigEnvironment::from_pairs([("XDG_CONFIG_HOME", "relative/path")]),
            ConfigEnvironment::default(),
        ] {
            let location = locate_config_file(&env);
            assert_eq!(
                location.path, None,
                "an injected environment naming no absolute XDG_CONFIG_HOME has nowhere to look"
            );
            assert!(
                location
                    .warnings
                    .iter()
                    .any(|warning| matches!(warning, ConfigWarning::NoConfigDir)),
                "having nowhere to look must be reported, got {:?}",
                location.warnings
            );
        }
    }

    /// The process environment does get the platform fallback: that is the
    /// path the CLI runs on, and it must find a real `config.toml`.
    #[test]
    fn a_process_environment_still_reaches_the_platform_directory() {
        // `dirs::config_dir()` answers on every platform this crate targets,
        // so an absent answer here would mean the fallback was skipped.
        let location = locate_config_file(&ConfigEnvironment::from_process());
        assert!(
            location.path.is_some(),
            "the CLI must be able to find its own configuration file, got {location:?}"
        );
    }
}
