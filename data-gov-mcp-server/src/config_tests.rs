//! #116: the MCP server resolves configuration through the same chain the
//! CLI uses.
//!
//! #86 gave `data-gov` a configuration file and one precedence chain - flag,
//! then environment, then `config.toml`, then the built-in default. The MCP
//! server did not use it: it read two environment variables by hand and built
//! `DataGovConfig` programmatically, so a setting a user persisted in
//! `config.toml` silently had no effect on the agent-facing front door, and
//! three environment variables the CLI honours were ignored outright.
//!
//! AGENTS.md states the library, the CLI, and the MCP server are three faces
//! over one client, and that a capability one has and another lacks is a
//! layering defect rather than a missing convenience. These tests hold the two
//! front doors to the same rules.
//!
//! The server has no command-line flags, so the chain it sees is environment,
//! then file, then default. Every test drives an explicit environment and an
//! explicit parsed file rather than the real process and the real filesystem,
//! so nothing here depends on the machine it runs on or races another test
//! over a process-global variable.

use std::path::PathBuf;

use data_gov::config::{ConfigEnvironment, ConfigFile, ConfigResolver};
use data_gov::{DataGovClient, DataGovError, OperatingMode};
use serde_json::json;
use wiremock::matchers::{method as wm_method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::server::{DEFAULT_REQUEST_TIMEOUT, DataGovMcpServer};

/// A resolver with no flags, the given environment pairs, and the given
/// `config.toml` text - never the real process or the real filesystem.
fn resolver(env_pairs: &[(&str, &str)], config_toml: Option<&str>) -> ConfigResolver {
    let mut resolver = ConfigResolver::new()
        .with_environment(ConfigEnvironment::from_pairs(env_pairs.iter().copied()))
        .with_mode(OperatingMode::CommandLine);
    if let Some(text) = config_toml {
        let parsed = ConfigFile::parse(text, "config.toml").expect("the test config.toml parses");
        resolver = resolver.with_config_file(parsed);
    }
    resolver
}

#[test]
fn a_config_file_download_dir_reaches_the_mcp_server() {
    let server =
        DataGovMcpServer::from_resolver(resolver(&[], Some("download_dir = \"/srv/datasets\"\n")))
            .expect("the server builds");

    assert_eq!(
        server.download_dir_for_test(),
        PathBuf::from("/srv/datasets"),
        "a download directory persisted in config.toml must reach the MCP \
         server, not just the CLI (#116)"
    );
}

#[test]
fn a_config_file_base_url_reaches_the_mcp_server() {
    let server = DataGovMcpServer::from_resolver(resolver(
        &[],
        Some("base_url = \"https://catalog.example.gov\"\n"),
    ))
    .expect("the server builds");

    assert_eq!(
        server.portal_base_url_for_test(),
        "https://catalog.example.gov",
        "config.toml must be able to redirect the catalog the server talks to"
    );
}

/// The host launching an MCP server owns its environment, and the precedence
/// chain already says environment beats file. Reading the file must not
/// quietly override what the host set.
#[test]
fn the_host_environment_beats_the_config_file() {
    let server = DataGovMcpServer::from_resolver(resolver(
        &[("DATA_GOV_DOWNLOAD_DIR", "/run/host-chosen")],
        Some("download_dir = \"/srv/datasets\"\n"),
    ))
    .expect("the server builds");

    assert_eq!(
        server.download_dir_for_test(),
        PathBuf::from("/run/host-chosen"),
        "the environment the host supplied must win over config.toml"
    );
}

/// `DATA_GOV_BASE_URL` and `DATA_GOV_USER_AGENT` were the only two settings
/// the server read before this change. They must keep working exactly as they
/// did, or the change breaks every existing deployment.
#[test]
fn the_two_environment_variables_the_server_already_read_still_work() {
    let server = DataGovMcpServer::from_resolver(resolver(
        &[
            ("DATA_GOV_BASE_URL", "https://catalog.example.gov"),
            ("DATA_GOV_USER_AGENT", "probe/1.0"),
        ],
        None,
    ))
    .expect("the server builds");

    assert_eq!(
        server.portal_base_url_for_test(),
        "https://catalog.example.gov"
    );
    assert_eq!(server.user_agent_for_test().as_deref(), Some("probe/1.0"));
}

/// Three environment variables the CLI honours were ignored by the server
/// outright - the exact shape of #53, where a setting is accepted in one
/// place and silently dropped in another.
#[test]
fn the_environment_variables_the_server_used_to_ignore_now_reach_it() {
    let server = DataGovMcpServer::from_resolver(resolver(
        &[
            ("DATA_GOV_DOWNLOAD_DIR", "/run/downloads"),
            ("DATA_GOV_MAX_CONCURRENT_DOWNLOADS", "7"),
            ("DATA_GOV_DOWNLOAD_TIMEOUT_SECS", "42"),
        ],
        None,
    ))
    .expect("the server builds");

    assert_eq!(
        server.download_dir_for_test(),
        PathBuf::from("/run/downloads")
    );
    assert_eq!(server.max_concurrent_downloads_for_test(), 7);
    assert_eq!(server.download_timeout_secs_for_test(), 42);
}

/// With nothing set anywhere, the server must still start on the built-in
/// defaults rather than refusing.
#[test]
fn an_empty_environment_and_no_file_still_builds_a_server() {
    let server = DataGovMcpServer::from_resolver(resolver(&[], None)).expect("the server builds");

    assert_eq!(
        server.portal_base_url_for_test(),
        "https://catalog.data.gov"
    );
}

/// A value that cannot work must stop the server at startup, naming the
/// setting, rather than producing a client that fails on first use.
#[test]
fn a_value_that_cannot_work_fails_startup_and_names_the_setting() {
    let outcome = DataGovMcpServer::from_resolver(resolver(
        &[("DATA_GOV_MAX_CONCURRENT_DOWNLOADS", "0")],
        None,
    ));

    let message = match outcome {
        Err(err) => err.to_string(),
        Ok(_) => panic!("a zero concurrency limit must not build a server"),
    };
    assert!(
        message.contains("max_concurrent_downloads"),
        "the failure must name the setting that is wrong, got: {message}"
    );
}

/// A broken `config.toml` is a fault the operator has to see, not something
/// to shrug off - the CLI already refuses to start on one.
#[test]
fn a_malformed_config_file_is_an_error_rather_than_being_ignored() {
    let outcome = ConfigFile::parse("download_dir = \n", "config.toml");

    assert!(
        matches!(outcome, Err(DataGovError::ConfigError { .. })),
        "a malformed config.toml must be a ConfigError, so the server can \
         refuse to start rather than run on settings the operator did not choose"
    );
}

/// A scratch directory unique to this process and thread.
fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "data-gov-mcp-config-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// #116's acceptance criterion: a `config.toml` setting must reach an MCP
/// **tool call**, not merely the server's configuration struct.
///
/// This drives the real `data_gov.downloadResources` handler with **no**
/// `outputDir` argument, so the only thing that can decide where the file
/// lands is the resolved default - which here comes from `config.toml` and
/// nowhere else.
///
/// One thing is set outside the chain, deliberately: downloads refuse
/// loopback addresses by default, and the mock catalog listens on one. That
/// opt-in is a property of the test harness rather than of the precedence
/// chain, and it is the same opt-in `test_support::test_server` takes.
#[tokio::test]
async fn a_config_file_download_dir_decides_where_a_tool_call_writes() {
    let mock = MockServer::start().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/api/dataset/config-probe"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "slug": "config-probe",
                "title": "Config Probe",
                "dcat": {
                    "@type": "dcat:Dataset",
                    "title": "Config Probe",
                    "distribution": [{
                        "@type": "dcat:Distribution",
                        "title": "readings",
                        "downloadURL": format!("{}/readings.csv", mock.uri()),
                        "mediaType": "text/csv"
                    }]
                }
            }],
            "sort": "relevance"
        })))
        .mount(&mock)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/readings.csv"))
        .respond_with(ResponseTemplate::new(200).set_body_string("a,b\n1,2\n"))
        .mount(&mock)
        .await;

    let chosen_dir = scratch_dir("download-dir");
    let config_toml = format!(
        "download_dir = {:?}\nbase_url = {:?}\n",
        chosen_dir.to_string_lossy(),
        mock.uri()
    );

    let resolved = resolver(&[], Some(&config_toml))
        .resolve()
        .expect("the config.toml resolves");
    let config = resolved.into_config().with_private_network_downloads(true);
    let server = DataGovMcpServer {
        data_gov: DataGovClient::with_config(config).expect("build the client"),
        portal_base_url: mock.uri(),
        request_timeout: DEFAULT_REQUEST_TIMEOUT,
        test_gate: None,
    };

    let value = server
        .dispatch(
            "tools/call",
            Some(json!({
                "name": "data_gov_download_resources",
                "arguments": {
                    "datasetId": "config-probe",
                    "datasetSubdirectory": false
                }
            })),
        )
        .await
        .expect("the download dispatches");

    let summary = &value["structuredContent"];
    let reported_dir = summary["downloadDirectory"].as_str().unwrap_or_default();
    // Assert on the directory the chain chose, not on a filename: the name is
    // derived from the distribution title elsewhere, and pinning it here would
    // make this test fail for a reason that has nothing to do with #116.
    let written_path = summary["downloads"][0]["path"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let landed_inside = PathBuf::from(&written_path).starts_with(&chosen_dir)
        && PathBuf::from(&written_path).exists();
    let _ = std::fs::remove_dir_all(&chosen_dir);

    assert_eq!(
        reported_dir,
        chosen_dir.to_string_lossy(),
        "with no outputDir argument, the directory config.toml named must be \
         the one the tool call used, which is the whole point of #116: {value}"
    );
    assert!(
        landed_inside,
        "the file must actually exist under the config.toml directory, not \
         merely be reported there. Reported path: {written_path}"
    );
}
