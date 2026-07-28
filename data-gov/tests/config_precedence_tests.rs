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

/// The other half of the default cell. CLAUDE.md's "Configuration and file
/// locations" table names `dirs::download_dir()` as the call for the user's
/// own Downloads folder, so that call is the specification here, not this
/// crate's incidental choice. `~/Downloads` is the fallback where the platform
/// names no folder.
#[test]
fn download_dir_default_is_the_downloads_folder_in_interactive_mode() {
    let resolved = ConfigResolver::new()
        .with_mode(OperatingMode::Interactive)
        .resolve()
        .expect("resolution must succeed");

    let expected = dirs::download_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Downloads")
    });

    assert_eq!(resolved.config().get_base_download_dir(), expected);
    assert_ne!(
        resolved.config().get_base_download_dir(),
        std::env::current_dir().expect("the test process has a cwd"),
        "the REPL must not silently download into whatever directory it was launched from"
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

    assert_eq!(
        resolved.config().catalog_config.base_path,
        PUBLISHED_BASE_URL
    );
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

/// The rendered value each entry in [`flags_set_everything`] must resolve to.
///
/// Kept next to the flags rather than derived from the result, so a resolver
/// that reported the right *source* against the wrong *value* is caught. That
/// is not hypothetical: a mutation that swapped the flag and environment
/// arguments of the layer picker left every source reading `flag` while every
/// value came from the environment, and a source-only assertion passed.
const FLAG_VALUES: [(SettingKey, &str); 5] = [
    (SettingKey::DownloadDir, "/from/flag"),
    (SettingKey::BaseUrl, "https://flag.example.com"),
    (SettingKey::MaxConcurrentDownloads, "33"),
    (SettingKey::DownloadTimeoutSecs, "333"),
    (SettingKey::UserAgent, "from-flag/1.0"),
];

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

    // Matching only the count would let a duplicated entry hide a missing
    // setting.
    let covered: Vec<SettingKey> = FLAG_VALUES.iter().map(|(key, _)| *key).collect();
    assert_eq!(
        covered,
        SettingKey::ALL.to_vec(),
        "every setting needs an expected flag value here, in SettingKey::ALL order"
    );

    for (key, expected) in FLAG_VALUES {
        let setting = resolved.setting(key);
        assert_eq!(
            setting.source,
            SettingSource::Flag,
            "{key} must be overridable by a command-line flag"
        );
        assert_eq!(
            setting.value, expected,
            "{key} must carry the value the flag set, not merely be labelled as if it did"
        );
    }
}

/// `config get <key>` and `config set <key>` (#87) receive the key as a
/// string typed by a user. Turning it back into a setting belongs here, next
/// to the keys themselves, not hand-rolled in the subcommand.
#[test]
fn a_config_key_string_maps_back_to_its_setting() {
    for key in SettingKey::ALL {
        assert_eq!(
            key.config_key().parse::<SettingKey>(),
            Ok(key),
            "{key} must round-trip through its own config.toml key"
        );
    }
    assert!(
        "download-dir".parse::<SettingKey>().is_err(),
        "a near miss must be refused, not guessed at"
    );
    assert!("".parse::<SettingKey>().is_err());
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

/// An environment somebody supplied is the whole environment. Falling back to
/// the platform location would let a test, a container, or an embedder read
/// the real machine's `config.toml` while believing it had injected
/// everything - and this module's own doc comment promises it does not.
#[test]
fn an_injected_environment_never_reaches_the_platform_config_directory() {
    let location = locate_config_file(&ConfigEnvironment::from_pairs([(
        "DATA_GOV_USER_AGENT",
        "irrelevant/1.0",
    )]));

    assert_eq!(
        location.path, None,
        "an injected environment with no XDG_CONFIG_HOME has nowhere to look"
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

#[test]
fn a_resolver_told_nothing_reads_no_configuration_file() {
    let resolved = ConfigResolver::new()
        .load_config_file()
        .expect("nothing to read is not a failure")
        .resolve()
        .expect("resolution must succeed");

    for key in SettingKey::ALL {
        assert_eq!(
            resolved.source_of(key),
            SettingSource::Default,
            "{key} must not have been supplied by a file on the machine running the tests"
        );
    }
}

// ---------------------------------------------------------------------------
// Credentials in a base URL
// ---------------------------------------------------------------------------

/// A base URL may legitimately carry credentials for an authenticated internal
/// mirror. They must never appear in what a front end displays (`config show`,
/// #87) or in an error message - CLAUDE.md forbids logging a secret at any
/// level, and both of those are printed.
#[test]
fn credentials_in_a_base_url_are_masked_in_the_reported_value() {
    let resolved = ConfigResolver::new()
        .with_environment(environment(&[(
            "DATA_GOV_BASE_URL",
            "https://svc-user:s3cr3t@mirror.example.com",
        )]))
        .resolve()
        .expect("a credentialed mirror URL is legitimate");

    let reported = &resolved.setting(SettingKey::BaseUrl).value;
    assert!(
        !reported.contains("s3cr3t"),
        "the password must not reach anything that gets printed, got: {reported}"
    );
    assert!(
        reported.contains("mirror.example.com"),
        "the host must still be legible, got: {reported}"
    );

    assert_eq!(
        resolved.config().catalog_config.base_path,
        "https://svc-user:s3cr3t@mirror.example.com",
        "the client still needs the real URL; only what is displayed is masked"
    );
}

#[test]
fn credentials_in_a_rejected_base_url_are_masked_in_the_error() {
    let err = ConfigResolver::new()
        .with_environment(environment(&[(
            "DATA_GOV_BASE_URL",
            "ftp://svc-user:s3cr3t@mirror.example.com",
        )]))
        .resolve()
        .expect_err("ftp is not a scheme this client speaks");

    let message = err.to_string();
    assert!(
        !message.contains("s3cr3t"),
        "an error message reaches stderr and logs; it must not carry the password, got: {message}"
    );
    assert!(
        message.contains("base_url"),
        "the error must still name the setting, got: {message}"
    );
}

/// A bare userinfo component with no password is how a token is usually
/// passed, so it is masked as a whole rather than kept as a username.
#[test]
fn a_bare_userinfo_token_in_a_base_url_is_masked_whole() {
    let resolved = ConfigResolver::new()
        .with_environment(environment(&[(
            "DATA_GOV_BASE_URL",
            "https://gho_thisisatoken@mirror.example.com",
        )]))
        .resolve()
        .expect("resolution must succeed");

    let reported = &resolved.setting(SettingKey::BaseUrl).value;
    assert!(
        !reported.contains("gho_thisisatoken"),
        "a bare userinfo component is a credential, got: {reported}"
    );
}

// ---------------------------------------------------------------------------
// Where the file is found
// ---------------------------------------------------------------------------

/// The XDG base directory specification says a relative `XDG_CONFIG_HOME` is
/// invalid and must be ignored. `dirs` does the same on Linux.
#[test]
fn a_relative_xdg_config_home_is_ignored_and_warns() {
    let location = locate_config_file(&environment(&[("XDG_CONFIG_HOME", "not/an/absolute/path")]));

    assert!(
        location.warnings.iter().any(|warning| matches!(
            warning,
            ConfigWarning::RelativeXdgConfigHome { value } if value.as_os_str() == "not/an/absolute/path"
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
// Surrounding whitespace
//
// An environment variable picks one up from `VAR=$(cat file)` or a stray space
// in a shell profile, and a `config.toml` line picks one up from a text
// editor. Every string setting can carry it, so each one states what it does
// with it.
// ---------------------------------------------------------------------------

/// The value the resolver reports must be the value it installed.
///
/// A validator that checks a cleaned-up copy and then stores the original
/// passes every source-only assertion while sending something else on the
/// wire, and `config show` would report the setting as fine.
fn installed_value(config: &DataGovConfig, key: SettingKey) -> String {
    match key {
        SettingKey::DownloadDir => config.get_base_download_dir().display().to_string(),
        SettingKey::BaseUrl => config.catalog_config.base_path.clone(),
        SettingKey::MaxConcurrentDownloads => config.max_concurrent_downloads.to_string(),
        SettingKey::DownloadTimeoutSecs => config.download_timeout_secs.to_string(),
        SettingKey::UserAgent => config.user_agent().to_owned(),
        other => panic!("{other} has no installed-value mapping in this test"),
    }
}

#[test]
fn every_reported_value_is_the_value_actually_installed() {
    let resolved = ConfigResolver::new()
        .with_environment(environment(&[
            ("DATA_GOV_DOWNLOAD_DIR", "  /padded/dir  "),
            ("DATA_GOV_BASE_URL", "  https://padded.example.com  "),
            ("DATA_GOV_MAX_CONCURRENT_DOWNLOADS", "4"),
            ("DATA_GOV_DOWNLOAD_TIMEOUT_SECS", "44"),
            ("DATA_GOV_USER_AGENT", "  padded/1.0  "),
        ]))
        .with_mode(OperatingMode::CommandLine)
        .resolve()
        .expect("resolution must succeed");

    for key in SettingKey::ALL {
        assert_eq!(
            resolved.setting(key).value,
            installed_value(resolved.config(), key),
            "{key} reports one value and installs another"
        );
    }
}

/// Whitespace has no meaning in a URL, and the catalog client builds a request
/// URL by concatenating onto `base_path` after trimming only trailing slashes.
/// A padded value therefore reaches `reqwest` intact and fails with
/// `invalid international domain name`, which names nothing.
#[test]
fn a_base_url_with_surrounding_whitespace_is_used_without_it() {
    let resolved = ConfigResolver::new()
        .with_environment(environment(&[(
            "DATA_GOV_BASE_URL",
            "  https://padded.example.com\n",
        )]))
        .resolve()
        .expect("surrounding whitespace must not fail the resolution");

    assert_eq!(
        resolved.config().catalog_config.base_path,
        "https://padded.example.com"
    );
    assert_eq!(
        resolved.setting(SettingKey::BaseUrl).value,
        "https://padded.example.com"
    );
}

/// A trailing slash is left alone: the catalog client already trims one when
/// it builds a URL, so rewriting the value would only make `config show`
/// disagree with what the user wrote, for no change on the wire.
#[test]
fn a_base_url_keeps_the_path_the_user_wrote() {
    let resolved = ConfigResolver::new()
        .with_config_file(config_file(
            "base_url = \"https://gateway.example.com/technology/datagov/v4\"\n",
        ))
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(
        resolved.config().catalog_config.base_path,
        "https://gateway.example.com/technology/datagov/v4",
        "a path-prefixed base URL must survive resolution unchanged"
    );
}

/// Whitespace has no meaning in a `User-Agent` either, and a padded one goes
/// out on every catalog request and every download.
#[test]
fn a_user_agent_with_surrounding_whitespace_is_used_without_it() {
    let resolved = ConfigResolver::new()
        .with_environment(environment(&[("DATA_GOV_USER_AGENT", "padded/1.0\n")]))
        .resolve()
        .expect("surrounding whitespace must not fail the resolution");

    assert_eq!(resolved.config().user_agent(), "padded/1.0");
}

/// A newline inside the value is not padding: it splits a header. `reqwest`
/// refuses it, but only as an opaque `builder error` at the first request,
/// which points at nothing.
#[test]
fn a_user_agent_containing_a_control_character_is_refused() {
    let err = ConfigResolver::new()
        .with_config_file(config_file(
            "user_agent = \"agent/1.0\\nX-Injected: yes\"\n",
        ))
        .resolve()
        .expect_err("a header value cannot carry a line break");

    let message = err.to_string();
    assert!(
        message.contains("user_agent"),
        "the error must name the setting, got: {message}"
    );
    assert!(
        message.contains("config.toml"),
        "the error must name where the value came from, got: {message}"
    );
}

/// `download_dir` is the one string setting where surrounding whitespace can
/// be deliberate: a directory named with a trailing space is a legal path. The
/// value is used as given, and reported, because it is almost always an
/// accident and silence is the failure this whole chain exists to remove.
#[test]
fn a_download_dir_with_surrounding_whitespace_is_kept_and_reported() {
    let padded = "/tmp/a directory named with a trailing space ";

    let resolved = ConfigResolver::new()
        .with_environment(environment(&[("DATA_GOV_DOWNLOAD_DIR", padded)]))
        .with_mode(OperatingMode::CommandLine)
        .resolve()
        .expect("resolution must succeed");

    assert_eq!(
        resolved.config().get_base_download_dir(),
        PathBuf::from(padded),
        "a path is used exactly as given; a trailing space may be part of the name"
    );
    assert!(
        resolved.warnings().iter().any(|warning| matches!(
            warning,
            ConfigWarning::SurroundingWhitespace { key, .. } if *key == SettingKey::DownloadDir
        )),
        "the whitespace must be reported, got {:?}",
        resolved.warnings()
    );
}

#[test]
fn a_download_dir_without_surrounding_whitespace_reports_nothing() {
    let resolved = ConfigResolver::new()
        .with_environment(environment(&[("DATA_GOV_DOWNLOAD_DIR", "/tmp/plain")]))
        .with_mode(OperatingMode::CommandLine)
        .resolve()
        .expect("resolution must succeed");

    assert!(
        resolved.warnings().is_empty(),
        "an ordinary path must not warn, got {:?}",
        resolved.warnings()
    );
}

/// WHATWG URL parsing removes tabs and line breaks from *inside* a URL as
/// well as around it, so `Url::parse` succeeds and reports a host the raw
/// string does not contain. Trimming the ends is not enough on its own.
#[test]
fn a_base_url_with_an_internal_line_break_is_refused() {
    let err = ConfigResolver::new()
        .with_environment(environment(&[(
            "DATA_GOV_BASE_URL",
            "https://exa\nmple.com",
        )]))
        .resolve()
        .expect_err("a URL the parser silently rewrites must not be accepted");

    let message = err.to_string();
    assert!(
        message.contains("base_url"),
        "the error must name the setting, got: {message}"
    );
}

// ---------------------------------------------------------------------------
// Values the environment can hold that are not text
//
// On Unix an environment variable is a byte string, and so is a path. A value
// that is not valid Unicode is legal, and dropping it silently turns an
// explicit override into a default with nothing to explain the difference.
// ---------------------------------------------------------------------------

/// A byte sequence that is a legal environment value and a legal path, and is
/// not valid UTF-8.
#[cfg(unix)]
fn non_unicode_value() -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff, 0xfe])
}

#[cfg(unix)]
#[test]
fn a_non_unicode_download_dir_from_the_environment_is_used_as_given() {
    let raw = non_unicode_value();

    let resolved = ConfigResolver::new()
        .with_environment(ConfigEnvironment::from_pairs([(
            "DATA_GOV_DOWNLOAD_DIR",
            raw.clone(),
        )]))
        .with_mode(OperatingMode::CommandLine)
        .resolve()
        .expect("a path that is not valid Unicode is still a path");

    assert_eq!(
        resolved.config().get_base_download_dir(),
        PathBuf::from(&raw),
        "a download directory must survive as bytes, not be dropped for not being UTF-8"
    );
    assert_eq!(
        resolved.source_of(SettingKey::DownloadDir),
        SettingSource::Environment
    );
}

#[cfg(unix)]
#[test]
fn a_non_unicode_textual_environment_value_is_reported_rather_than_dropped() {
    let resolved = ConfigResolver::new()
        .with_environment(ConfigEnvironment::from_pairs([(
            "DATA_GOV_USER_AGENT",
            non_unicode_value(),
        )]))
        .resolve()
        .expect("an unusable variable must not fail the whole resolution");

    assert_eq!(
        resolved.source_of(SettingKey::UserAgent),
        SettingSource::Default,
        "a User-Agent cannot be non-Unicode, so the layer below supplies it"
    );
    assert!(
        resolved.warnings().iter().any(|warning| matches!(
            warning,
            ConfigWarning::NonUnicodeEnvironmentValue { variable }
                if variable == "DATA_GOV_USER_AGENT"
        )),
        "dropping the variable must be reported, got {:?}",
        resolved.warnings()
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
