//! The configuration precedence chain: flag > environment > file > default.
//!
//! There is one test per setting per level, so a failing run names the cell
//! that broke rather than a line number. The five settings and their keys
//! come from #86:
//!
//! | Key | `DataGovConfig` field | Environment variable |
//! |---|---|---|
//! | `download_dir` | `base_download_dir` | `DATA_GOV_DOWNLOAD_DIR` |
//! | `base_url` | `catalog_config.base_path` | `DATA_GOV_BASE_URL` |
//! | `max_concurrent_downloads` | `max_concurrent_downloads` | `DATA_GOV_MAX_CONCURRENT_DOWNLOADS` |
//! | `download_timeout_secs` | `download_timeout_secs` | `DATA_GOV_DOWNLOAD_TIMEOUT_SECS` |
//! | `user_agent` | `catalog_config.user_agent` | `DATA_GOV_USER_AGENT` |
//!
//! **Nothing here mutates the process environment.** Rust runs tests as
//! threads in one process, so a `set_var` in one test is visible to every
//! other test running at the same moment, and this workspace has already been
//! bitten by exactly that kind of shared-global race (#112). The environment
//! and the configuration file are injected instead:
//! [`ConfigEnvironment::from_pairs`] supplies the variables and
//! [`ConfigFile::parse`] supplies the file, so no test reads the developer's
//! own home directory, shell, or `config.toml`.

use data_gov::config::{
    ConfigEnvironment, ConfigFile, ConfigOverrides, ConfigResolver, ConfigWarning,
    ParsedConfigFile, SettingKey, SettingSource, locate_config_file,
};
use data_gov::{DataGovConfig, DataGovError, OperatingMode};
use std::path::{Path, PathBuf};

/// The Catalog API's published base URL, from resources.data.gov, not from
/// this workspace's own constant.
const PUBLISHED_BASE_URL: &str = "https://catalog.data.gov";

/// Parse TOML the way a real `config.toml` would be parsed.
fn config_file(toml: &str) -> ParsedConfigFile {
    ConfigFile::parse(toml, "config.toml").expect("the fixture TOML must parse")
}

/// An injected environment holding exactly these variables and nothing else.
fn environment(pairs: &[(&str, &str)]) -> ConfigEnvironment {
    ConfigEnvironment::from_pairs(pairs.iter().copied())
}

/// A file that sets every setting, so a test for a higher level proves the
/// higher level won rather than proving the file was empty.
const FILE_SETS_EVERYTHING: &str = "\
download_dir = \"/from/file\"
base_url = \"https://file.example.com\"
max_concurrent_downloads = 11
download_timeout_secs = 111
user_agent = \"from-file/1.0\"
";

/// An environment that sets every setting.
fn env_sets_everything() -> ConfigEnvironment {
    environment(&[
        ("DATA_GOV_DOWNLOAD_DIR", "/from/env"),
        ("DATA_GOV_BASE_URL", "https://env.example.com"),
        ("DATA_GOV_MAX_CONCURRENT_DOWNLOADS", "22"),
        ("DATA_GOV_DOWNLOAD_TIMEOUT_SECS", "222"),
        ("DATA_GOV_USER_AGENT", "from-env/1.0"),
    ])
}

/// Flags that set every setting.
fn flags_set_everything() -> ConfigOverrides {
    ConfigOverrides::default()
        .with_download_dir("/from/flag")
        .with_base_url("https://flag.example.com")
        .with_max_concurrent_downloads(33)
        .with_download_timeout_secs(333)
        .with_user_agent("from-flag/1.0")
}

// ---------------------------------------------------------------------------
// download_dir
// ---------------------------------------------------------------------------

#[test]
fn download_dir_flag_beats_environment_file_and_default() {
    let resolved = ConfigResolver::new()
        .with_flags(flags_set_everything())
        .with_environment(env_sets_everything())
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .with_mode(OperatingMode::CommandLine)
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(
        resolved.config().get_base_download_dir(),
        PathBuf::from("/from/flag")
    );
    assert_eq!(
        resolved.source_of(SettingKey::DownloadDir),
        SettingSource::Flag
    );
}

#[test]
fn download_dir_environment_beats_file_and_default() {
    let resolved = ConfigResolver::new()
        .with_environment(env_sets_everything())
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .with_mode(OperatingMode::CommandLine)
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(
        resolved.config().get_base_download_dir(),
        PathBuf::from("/from/env")
    );
    assert_eq!(
        resolved.source_of(SettingKey::DownloadDir),
        SettingSource::Environment
    );
}

#[test]
fn download_dir_file_beats_default() {
    let resolved = ConfigResolver::new()
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .with_mode(OperatingMode::CommandLine)
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(
        resolved.config().get_base_download_dir(),
        PathBuf::from("/from/file")
    );
    assert_eq!(
        resolved.source_of(SettingKey::DownloadDir),
        SettingSource::File
    );
}

#[test]
fn download_dir_default_applies_when_nothing_is_set() {
    let resolved = ConfigResolver::new()
        .with_mode(OperatingMode::CommandLine)
        .resolve()
        .expect("resolution must succeed");

    let working_directory = std::env::current_dir().expect("the test process has a cwd");
    assert_eq!(
        resolved.config().get_base_download_dir(),
        working_directory,
        "with nothing set, a one-shot command downloads to the working directory"
    );
    assert_eq!(
        resolved.source_of(SettingKey::DownloadDir),
        SettingSource::Default
    );
}

// ---------------------------------------------------------------------------
// base_url
// ---------------------------------------------------------------------------

#[test]
fn base_url_flag_beats_environment_file_and_default() {
    let resolved = ConfigResolver::new()
        .with_flags(flags_set_everything())
        .with_environment(env_sets_everything())
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(
        resolved.config().catalog_config.base_path,
        "https://flag.example.com"
    );
    assert_eq!(resolved.source_of(SettingKey::BaseUrl), SettingSource::Flag);
}

#[test]
fn base_url_environment_beats_file_and_default() {
    let resolved = ConfigResolver::new()
        .with_environment(env_sets_everything())
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(
        resolved.config().catalog_config.base_path,
        "https://env.example.com"
    );
    assert_eq!(
        resolved.source_of(SettingKey::BaseUrl),
        SettingSource::Environment
    );
}

#[test]
fn base_url_file_beats_default() {
    let resolved = ConfigResolver::new()
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(
        resolved.config().catalog_config.base_path,
        "https://file.example.com"
    );
    assert_eq!(resolved.source_of(SettingKey::BaseUrl), SettingSource::File);
}

#[test]
fn base_url_default_applies_when_nothing_is_set() {
    let resolved = ConfigResolver::new()
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.config().catalog_config.base_path, PUBLISHED_BASE_URL);
    assert_eq!(
        resolved.source_of(SettingKey::BaseUrl),
        SettingSource::Default
    );
}

// ---------------------------------------------------------------------------
// max_concurrent_downloads
// ---------------------------------------------------------------------------

#[test]
fn max_concurrent_downloads_flag_beats_environment_file_and_default() {
    let resolved = ConfigResolver::new()
        .with_flags(flags_set_everything())
        .with_environment(env_sets_everything())
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.config().max_concurrent_downloads, 33);
    assert_eq!(
        resolved.source_of(SettingKey::MaxConcurrentDownloads),
        SettingSource::Flag
    );
}

#[test]
fn max_concurrent_downloads_environment_beats_file_and_default() {
    let resolved = ConfigResolver::new()
        .with_environment(env_sets_everything())
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.config().max_concurrent_downloads, 22);
    assert_eq!(
        resolved.source_of(SettingKey::MaxConcurrentDownloads),
        SettingSource::Environment
    );
}

#[test]
fn max_concurrent_downloads_file_beats_default() {
    let resolved = ConfigResolver::new()
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.config().max_concurrent_downloads, 11);
    assert_eq!(
        resolved.source_of(SettingKey::MaxConcurrentDownloads),
        SettingSource::File
    );
}

#[test]
fn max_concurrent_downloads_default_applies_when_nothing_is_set() {
    let resolved = ConfigResolver::new()
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(
        resolved.config().max_concurrent_downloads,
        DataGovConfig::default().max_concurrent_downloads
    );
    assert_eq!(
        resolved.source_of(SettingKey::MaxConcurrentDownloads),
        SettingSource::Default
    );
}

// ---------------------------------------------------------------------------
// download_timeout_secs
// ---------------------------------------------------------------------------

#[test]
fn download_timeout_secs_flag_beats_environment_file_and_default() {
    let resolved = ConfigResolver::new()
        .with_flags(flags_set_everything())
        .with_environment(env_sets_everything())
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.config().download_timeout_secs, 333);
    assert_eq!(
        resolved.source_of(SettingKey::DownloadTimeoutSecs),
        SettingSource::Flag
    );
}

#[test]
fn download_timeout_secs_environment_beats_file_and_default() {
    let resolved = ConfigResolver::new()
        .with_environment(env_sets_everything())
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.config().download_timeout_secs, 222);
    assert_eq!(
        resolved.source_of(SettingKey::DownloadTimeoutSecs),
        SettingSource::Environment
    );
}

#[test]
fn download_timeout_secs_file_beats_default() {
    let resolved = ConfigResolver::new()
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.config().download_timeout_secs, 111);
    assert_eq!(
        resolved.source_of(SettingKey::DownloadTimeoutSecs),
        SettingSource::File
    );
}

#[test]
fn download_timeout_secs_default_applies_when_nothing_is_set() {
    let resolved = ConfigResolver::new()
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(
        resolved.config().download_timeout_secs,
        DataGovConfig::default().download_timeout_secs
    );
    assert_eq!(
        resolved.source_of(SettingKey::DownloadTimeoutSecs),
        SettingSource::Default
    );
}

// ---------------------------------------------------------------------------
// user_agent
// ---------------------------------------------------------------------------

#[test]
fn user_agent_flag_beats_environment_file_and_default() {
    let resolved = ConfigResolver::new()
        .with_flags(flags_set_everything())
        .with_environment(env_sets_everything())
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.config().user_agent(), "from-flag/1.0");
    assert_eq!(
        resolved.source_of(SettingKey::UserAgent),
        SettingSource::Flag
    );
}

#[test]
fn user_agent_environment_beats_file_and_default() {
    let resolved = ConfigResolver::new()
        .with_environment(env_sets_everything())
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.config().user_agent(), "from-env/1.0");
    assert_eq!(
        resolved.source_of(SettingKey::UserAgent),
        SettingSource::Environment
    );
}

#[test]
fn user_agent_file_beats_default() {
    let resolved = ConfigResolver::new()
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.config().user_agent(), "from-file/1.0");
    assert_eq!(
        resolved.source_of(SettingKey::UserAgent),
        SettingSource::File
    );
}

#[test]
fn user_agent_default_applies_when_nothing_is_set() {
    let resolved = ConfigResolver::new()
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(
        resolved.config().user_agent(),
        DataGovConfig::default().user_agent()
    );
    assert_eq!(
        resolved.source_of(SettingKey::UserAgent),
        SettingSource::Default
    );
}

// ---------------------------------------------------------------------------
// The chain itself, across the whole set of settings
// ---------------------------------------------------------------------------

/// "A setting a flag cannot override is a bug" (CLAUDE.md, "Configuration and
/// file locations"). Checking the whole set means a sixth setting added
/// without a flag layer fails here, rather than waiting for somebody to
/// notice the missing per-setting test.
#[test]
fn a_flag_overrides_every_setting() {
    let resolved = ConfigResolver::new()
        .with_flags(flags_set_everything())
        .with_environment(env_sets_everything())
        .with_config_file(config_file(FILE_SETS_EVERYTHING))
        .resolve()
        .expect("resolution must succeed");

    for key in SettingKey::ALL {
        assert_eq!(
            resolved.source_of(key),
            SettingSource::Flag,
            "{key} must be overridable by a command-line flag"
        );
    }
}

/// Every setting reports a value and a source, so `config show` (#87) can
/// print a complete table without a hole in it.
#[test]
fn every_setting_reports_a_value_and_a_source() {
    let resolved = ConfigResolver::new()
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.settings().len(), SettingKey::ALL.len());
    for key in SettingKey::ALL {
        let setting = resolved.setting(key);
        assert_eq!(setting.key, key, "setting({key}) must return that setting");
        assert_eq!(setting.source, SettingSource::Default);
        assert!(
            !setting.value.is_empty(),
            "{key} must report the value it resolved to"
        );
    }
}

/// Resolution is a read. Repeating it changes nothing (CLAUDE.md, "Repeating
/// an operation must be safe").
#[test]
fn resolving_twice_gives_the_same_answer() {
    let resolver = ConfigResolver::new()
        .with_environment(env_sets_everything())
        .with_config_file(config_file(FILE_SETS_EVERYTHING));

    let first = resolver.resolve().expect("resolution must succeed");
    let second = resolver.resolve().expect("resolution must succeed");

    assert_eq!(first.settings(), second.settings());
    assert_eq!(first.warnings(), second.warnings());
}

// ---------------------------------------------------------------------------
// The configuration file
// ---------------------------------------------------------------------------

/// An absent file means "all defaults", not a failure. A normal negative
/// answer is not a failure (CLAUDE.md).
#[test]
fn an_absent_config_file_is_not_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("config.toml");

    let outcome = ConfigFile::read(&missing).expect("an absent file must not be an error");
    assert!(
        outcome.is_none(),
        "an absent file must read as 'no file', not as an empty one that failed"
    );
}

/// A resolver pointed at a configuration directory with no file in it still
/// resolves, and every setting comes from its default.
#[test]
fn an_absent_config_file_resolves_to_all_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");

    let resolved = ConfigResolver::new()
        .with_environment(environment(&[(
            "XDG_CONFIG_HOME",
            dir.path().to_str().expect("tempdir path is UTF-8"),
        )]))
        .load_config_file()
        .expect("an absent file must not fail the load")
        .resolve()
        .expect("resolution must succeed");

    for key in SettingKey::ALL {
        assert_eq!(
            resolved.source_of(key),
            SettingSource::Default,
            "{key} must fall back to its default when no file exists"
        );
    }
}

#[test]
fn malformed_toml_is_an_error_naming_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "download_dir = \n").expect("write the broken file");

    let err = ConfigFile::read(&path).expect_err("malformed TOML must be an error");

    assert!(
        matches!(err, DataGovError::ConfigError { .. }),
        "a broken configuration file is a configuration error, got {err:?}"
    );
    assert!(
        err.to_string().contains(&path.display().to_string()),
        "the error must name the file that could not be parsed, got: {err}"
    );
}

/// A value of the wrong type is a hard error, and the message names the key
/// so the user can find it in their own file.
#[test]
fn a_config_value_of_the_wrong_type_is_an_error_naming_the_key() {
    let err = ConfigFile::parse("max_concurrent_downloads = \"three\"\n", "config.toml")
        .expect_err("a string where a number belongs must be an error");

    assert!(
        err.to_string().contains("max_concurrent_downloads"),
        "the error must name the offending key, got: {err}"
    );
}

/// A file written by a newer version must still load on an older binary.
#[test]
fn an_unknown_config_key_warns_and_is_ignored() {
    let parsed = ConfigFile::parse(
        "max_concurrent_downloads = 7\nsetting_from_a_future_release = \"x\"\n",
        "config.toml",
    )
    .expect("an unknown key must not fail the parse");

    assert_eq!(
        parsed.settings.max_concurrent_downloads,
        Some(7),
        "the keys this build does know must still be read"
    );
    assert!(
        parsed.warnings.iter().any(|warning| matches!(
            warning,
            ConfigWarning::UnknownKey { key, .. } if key == "setting_from_a_future_release"
        )),
        "the unknown key must be reported, got {:?}",
        parsed.warnings
    );

    let resolved = ConfigResolver::new()
        .with_config_file(parsed)
        .resolve()
        .expect("an unknown key must not fail resolution");

    assert_eq!(resolved.config().max_concurrent_downloads, 7);
    assert!(
        resolved.warnings().iter().any(|warning| matches!(
            warning,
            ConfigWarning::UnknownKey { key, .. } if key == "setting_from_a_future_release"
        )),
        "the warning must survive into the resolved configuration so a front end can print it"
    );
}

// ---------------------------------------------------------------------------
// Where the file is found
// ---------------------------------------------------------------------------

#[test]
fn xdg_config_home_locates_the_config_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let app_dir = dir.path().join("data-gov");
    std::fs::create_dir_all(&app_dir).expect("create the app config directory");
    std::fs::write(app_dir.join("config.toml"), "download_timeout_secs = 42\n")
        .expect("write the config file");

    let env = environment(&[(
        "XDG_CONFIG_HOME",
        dir.path().to_str().expect("tempdir path is UTF-8"),
    )]);

    let location = locate_config_file(&env);
    assert_eq!(
        location.path.as_deref(),
        Some(app_dir.join("config.toml").as_path()),
        "the file lives at <XDG_CONFIG_HOME>/data-gov/config.toml"
    );

    let resolved = ConfigResolver::new()
        .with_environment(env)
        .load_config_file()
        .expect("the file must load")
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.config().download_timeout_secs, 42);
    assert_eq!(
        resolved.source_of(SettingKey::DownloadTimeoutSecs),
        SettingSource::File
    );
}

/// The XDG base directory specification says a relative `XDG_CONFIG_HOME` is
/// invalid and must be ignored. `dirs` does the same on Linux.
#[test]
fn a_relative_xdg_config_home_is_ignored_and_warns() {
    let location = locate_config_file(&environment(&[(
        "XDG_CONFIG_HOME",
        "not/an/absolute/path",
    )]));

    assert!(
        location.warnings.iter().any(|warning| matches!(
            warning,
            ConfigWarning::RelativeXdgConfigHome { value } if value == "not/an/absolute/path"
        )),
        "a relative XDG_CONFIG_HOME must be reported, got {:?}",
        location.warnings
    );
    assert!(
        location
            .path
            .as_ref()
            .is_none_or(|path| !path.starts_with("not")),
        "the relative value must not be used as a base, got {:?}",
        location.path
    );
}

// ---------------------------------------------------------------------------
// Values that cannot work
// ---------------------------------------------------------------------------

#[test]
fn zero_max_concurrent_downloads_from_the_file_is_refused_naming_the_setting_and_source() {
    let err = ConfigResolver::new()
        .with_config_file(config_file("max_concurrent_downloads = 0\n"))
        .resolve()
        .expect_err("a zero-permit semaphore never completes a download (#73)");

    let message = err.to_string();
    assert!(
        message.contains("max_concurrent_downloads"),
        "the error must name the setting, got: {message}"
    );
    assert!(
        message.contains("config.toml"),
        "the error must name where the bad value came from, got: {message}"
    );
}

#[test]
fn zero_download_timeout_from_the_environment_is_refused_naming_the_setting_and_source() {
    let err = ConfigResolver::new()
        .with_environment(environment(&[("DATA_GOV_DOWNLOAD_TIMEOUT_SECS", "0")]))
        .resolve()
        .expect_err("a zero stall timeout fails every download instantly (#107)");

    let message = err.to_string();
    assert!(
        message.contains("download_timeout_secs"),
        "the error must name the setting, got: {message}"
    );
    assert!(
        message.contains("DATA_GOV_DOWNLOAD_TIMEOUT_SECS"),
        "the error must name the variable that carried the bad value, got: {message}"
    );
}

#[test]
fn a_non_numeric_environment_value_is_refused_naming_the_variable() {
    let err = ConfigResolver::new()
        .with_environment(environment(&[(
            "DATA_GOV_MAX_CONCURRENT_DOWNLOADS",
            "three",
        )]))
        .resolve()
        .expect_err("a non-numeric count must not be silently ignored");

    let message = err.to_string();
    assert!(
        message.contains("DATA_GOV_MAX_CONCURRENT_DOWNLOADS"),
        "the error must name the variable, got: {message}"
    );
    assert!(
        message.contains("three"),
        "the error must quote the value it could not use, got: {message}"
    );
}

#[test]
fn an_empty_user_agent_is_refused() {
    let err = ConfigResolver::new()
        .with_environment(environment(&[("DATA_GOV_USER_AGENT", "   ")]))
        .resolve()
        .expect_err("an empty User-Agent is not an identity");

    assert!(
        err.to_string().contains("user_agent"),
        "the error must name the setting, got: {err}"
    );
}

#[test]
fn a_base_url_that_is_not_http_is_refused() {
    let err = ConfigResolver::new()
        .with_config_file(config_file("base_url = \"ftp://catalog.example.com\"\n"))
        .resolve()
        .expect_err("the Catalog API is reached over http or https, nothing else");

    assert!(
        err.to_string().contains("base_url"),
        "the error must name the setting, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// #53: --download-dir is honoured in both operating modes
// ---------------------------------------------------------------------------

/// #53: `get_base_download_dir` returned `current_dir()` unconditionally in
/// `CommandLine` mode, so the flag was accepted and then discarded.
#[test]
fn download_dir_flag_is_honoured_in_command_line_mode() {
    let chosen = Path::new("/chosen/by/the/flag");

    let resolved = ConfigResolver::new()
        .with_flags(ConfigOverrides::default().with_download_dir(chosen))
        .with_mode(OperatingMode::CommandLine)
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.config().get_base_download_dir(), chosen);
    assert_ne!(
        resolved.config().get_base_download_dir(),
        std::env::current_dir().expect("the test process has a cwd"),
        "the flag must not be discarded in favour of the working directory"
    );
}

#[test]
fn download_dir_flag_is_honoured_in_interactive_mode() {
    let chosen = Path::new("/chosen/by/the/flag");

    let resolved = ConfigResolver::new()
        .with_flags(ConfigOverrides::default().with_download_dir(chosen))
        .with_mode(OperatingMode::Interactive)
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(resolved.config().get_base_download_dir(), chosen);
}

/// The library half of #53. The README publishes exactly this pair of calls.
#[test]
fn with_download_dir_is_honoured_in_command_line_mode() {
    let config = DataGovConfig::new()
        .with_mode(OperatingMode::CommandLine)
        .with_download_dir("/chosen/by/the/caller");

    assert_eq!(
        config.get_base_download_dir(),
        PathBuf::from("/chosen/by/the/caller"),
        "with_download_dir must not be inert in CommandLine mode (#53)"
    );
}

#[test]
fn with_download_dir_is_honoured_in_interactive_mode() {
    let config = DataGovConfig::new()
        .with_mode(OperatingMode::Interactive)
        .with_download_dir("/chosen/by/the/caller");

    assert_eq!(
        config.get_base_download_dir(),
        PathBuf::from("/chosen/by/the/caller")
    );
}

/// The fallback survives: with nothing chosen, a one-shot command still
/// writes into the working directory.
#[test]
fn command_line_mode_falls_back_to_the_working_directory_when_nothing_is_set() {
    let config = DataGovConfig::new().with_mode(OperatingMode::CommandLine);

    assert_eq!(
        config.get_base_download_dir(),
        std::env::current_dir().expect("the test process has a cwd")
    );
}
