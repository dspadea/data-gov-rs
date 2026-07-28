//! Downloads land inside the directory the user chose (#45, #67).
//!
//! A distribution's `title` and `format` come from a harvested third-party
//! `data.json`, and the filename is derived from both. These tests state the
//! invariant over a hostile matrix of the two, at both levels it has to hold:
//!
//! - the derived filename is a single path component, whatever went in
//! - the file written is inside the output directory, whatever went in
//!
//! The matrix is deliberately cross-producted. The suite this replaces used a
//! single benign title and format everywhere, and its strongest assertion was
//! that three paths differed - which a removed sanitizer still satisfies.

use std::path::{Component, Path, PathBuf};

use data_gov::catalog::models::Distribution;
use data_gov::{DataGovClient, DataGovConfig, OperatingMode};
use tempfile::TempDir;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Titles that a filename must survive. Each is a separate escape technique.
const HOSTILE_TITLES: [&str; 12] = [
    "../../escaped",
    "..\\..\\escaped",
    "....//....//escaped",
    "sub/dir/escaped",
    "/etc/cron.d/evil",
    "C:\\Windows\\System32\\evil",
    "..",
    ".",
    "",
    "..%2f..%2fescaped",
    "....",
    "ordinary-report",
];

/// Formats that a filename must survive. `format` is appended as an extension
/// and comes from the same untrusted record as the title.
const HOSTILE_FORMATS: [&str; 6] = ["CSV", "../evil", "/etc/passwd", "", "..", "csv/../.."];

fn distribution(url: &str, title: &str, format: &str) -> Distribution {
    Distribution {
        type_hint: None,
        title: Some(title.to_string()),
        description: None,
        download_url: Some(url.to_string()),
        access_url: None,
        media_type: None,
        format: Some(format.to_string()),
        license: None,
        described_by: None,
        described_by_type: None,
    }
}

/// Assert `name` can only ever name a file inside the directory it is joined
/// onto: one component, no separators, no traversal, not a directory alias.
fn assert_single_safe_component(name: &str, case: &str) {
    assert!(!name.is_empty(), "{case}: the derived filename is empty");
    assert!(
        !name.contains('/') && !name.contains('\\'),
        "{case}: the derived filename `{name}` carries a path separator"
    );
    assert!(
        !name.contains(".."),
        "{case}: the derived filename `{name}` carries a parent-directory reference"
    );
    assert!(
        name != "." && name != "..",
        "{case}: the derived filename `{name}` names a directory, not a file"
    );

    let mut components = Path::new(name).components();
    assert!(
        matches!(components.next(), Some(Component::Normal(_))),
        "{case}: the derived filename `{name}` does not start with a plain component"
    );
    assert!(
        components.next().is_none(),
        "{case}: the derived filename `{name}` is more than one component"
    );
}

#[test]
fn a_derived_filename_is_always_one_safe_component() {
    for title in HOSTILE_TITLES {
        for format in HOSTILE_FORMATS {
            for index in [None, Some(0), Some(7)] {
                let dist = distribution("https://example.gov/data", title, format);
                let name = DataGovClient::get_distribution_filename(&dist, None, index);
                let case = format!("title={title:?} format={format:?} index={index:?}");
                assert_single_safe_component(&name, &case);
            }
        }
    }
}

#[test]
fn a_hostile_fallback_name_is_also_reduced_to_one_component() {
    for fallback in ["../../escaped", "/etc/passwd", "..", ""] {
        for index in [None, Some(3)] {
            let mut dist = distribution("https://example.gov/api/records", "unused", "CSV");
            dist.title = None;
            dist.download_url = None;
            dist.access_url = None;
            dist.format = None;
            let name = DataGovClient::get_distribution_filename(&dist, Some(fallback), index);
            assert_single_safe_component(&name, &format!("fallback={fallback:?} index={index:?}"));
        }
    }
}

#[test]
fn a_name_sanitized_away_falls_back_to_a_usable_default() {
    // Every character in the title and the format is stripped, so there is
    // nothing left to build a name from.
    let dist = distribution("https://example.gov/data", "!@#$%^&*()", "!@#$%");
    assert_eq!(
        DataGovClient::get_distribution_filename(&dist, None, None),
        "data.dat",
        "a name that sanitizes to nothing needs a default, not an empty string"
    );
    assert_eq!(
        DataGovClient::get_distribution_filename(&dist, None, Some(4)),
        "data-4.dat",
        "the batch default carries the index, so two of them do not collide"
    );
}

#[test]
fn a_single_dot_title_falls_back_rather_than_naming_the_directory() {
    let dist = distribution("https://example.gov/data", ".", "");
    let name = DataGovClient::get_distribution_filename(&dist, None, None);
    assert_single_safe_component(&name, "title=\".\" format=\"\"");
}

/// Build a client that may reach the loopback mock server (#51).
fn test_client(download_dir: &Path) -> DataGovClient {
    let config = DataGovConfig::default()
        .with_mode(OperatingMode::Interactive)
        .with_download_dir(download_dir)
        .with_download_timeout(10)
        .with_private_network_downloads(true);
    DataGovClient::with_config(config).expect("test client must build")
}

/// Assert the written path is inside `output_dir` once both are normalized.
fn assert_written_inside(output_dir: &Path, written: &Path, case: &str) {
    assert!(
        written.exists(),
        "{case}: nothing was written at {written:?}"
    );

    let root = std::fs::canonicalize(output_dir).expect("output dir must canonicalize");
    let parent = written
        .parent()
        .expect("a written file always has a parent directory");
    let parent = std::fs::canonicalize(parent).unwrap_or_else(|err| {
        panic!("{case}: the parent of {written:?} does not canonicalize: {err}")
    });

    assert!(
        parent.starts_with(&root),
        "{case}: wrote to {parent:?}, which is outside {root:?}"
    );
    assert_eq!(
        parent, root,
        "{case}: wrote to {parent:?}, which is not the directory that was chosen ({root:?})"
    );
}

/// One hostile case, downloaded on its own so `index` is `None`.
async fn single_case(server_uri: &str, base: &Path, case_id: usize, title: &str, format: &str) {
    let output_dir = base.join(format!("single-{case_id}"));
    std::fs::create_dir_all(&output_dir).expect("case dir");
    let client = test_client(&output_dir);
    let dist = distribution(&format!("{server_uri}/payload"), title, format);

    let case = format!("index=None title={title:?} format={format:?}");
    let written = client
        .download_distribution(&dist, Some(&output_dir))
        .await
        .unwrap_or_else(|err| panic!("{case}: the download must succeed, got {err}"));

    assert_written_inside(&output_dir, &written, &case);
    let body = std::fs::read(&written).expect("read back");
    assert_eq!(
        body, b"payload bytes",
        "{case}: wrong content at {written:?}"
    );
}

/// One hostile case in a batch, so the filename carries an index.
async fn batch_case(server_uri: &str, base: &Path, case_id: usize, title: &str, format: &str) {
    let output_dir = base.join(format!("batch-{case_id}"));
    std::fs::create_dir_all(&output_dir).expect("case dir");
    let client = test_client(&output_dir);

    // The benign entry takes index 0, so the hostile one takes index 1 and the
    // indexed filename path is the one under test.
    let distributions = vec![
        distribution(&format!("{server_uri}/payload"), "control", "CSV"),
        distribution(&format!("{server_uri}/payload"), title, format),
    ];

    let case = format!("index=Some(1) title={title:?} format={format:?}");
    let mut results = client
        .download_distributions(&distributions, Some(&output_dir))
        .await;
    assert_eq!(results.len(), 2, "{case}: one result per distribution");

    let written = results
        .pop()
        .expect("two results")
        .unwrap_or_else(|err| panic!("{case}: the download must succeed, got {err}"));

    assert_written_inside(&output_dir, &written, &case);
}

#[tokio::test]
async fn every_hostile_title_and_format_lands_inside_the_output_directory() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/payload$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"payload bytes".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    // Nested so that a traversal escaping the output directory still lands
    // inside the temp directory and is cleaned up with it.
    let base = tmp.path().join("a").join("b").join("c").join("d");
    std::fs::create_dir_all(&base).expect("base dir");

    let mut case_id = 0;
    for title in HOSTILE_TITLES {
        for format in HOSTILE_FORMATS {
            single_case(&server.uri(), &base, case_id, title, format).await;
            batch_case(&server.uri(), &base, case_id, title, format).await;
            case_id += 1;
        }
    }
}

#[tokio::test]
async fn an_absolute_title_does_not_replace_the_output_directory() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/payload$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"payload bytes".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let output_dir = tmp.path().join("chosen");
    std::fs::create_dir_all(&output_dir).expect("output dir");

    // An absolute component makes `Path::join` discard everything before it,
    // so the output directory disappears entirely. The target stays inside the
    // temp directory so the test cannot write outside it even while red.
    let hostile = format!("{}/hijacked", tmp.path().display());
    let dist = distribution(&format!("{}/payload", server.uri()), &hostile, "txt");

    let client = test_client(&output_dir);
    let written = client
        .download_distribution(&dist, Some(&output_dir))
        .await
        .expect("an absolute title must still produce a download inside the output directory");

    assert_written_inside(&output_dir, &written, "absolute title");
    assert!(
        !tmp.path().join("hijacked.txt").exists(),
        "the absolute title must not be honoured"
    );
}

#[tokio::test]
async fn downloading_the_same_distribution_twice_leaves_one_correct_file() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/first$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"first body".to_vec()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/second$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"second body".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let output_dir = tmp.path().to_path_buf();
    let client = test_client(&output_dir);

    let first = distribution(&format!("{}/first", server.uri()), "report", "csv");
    let one = client
        .download_distribution(&first, Some(&output_dir))
        .await
        .expect("first download");

    let second = distribution(&format!("{}/second", server.uri()), "report", "csv");
    let two = client
        .download_distribution(&second, Some(&output_dir))
        .await
        .expect("second download");

    assert_eq!(
        one, two,
        "the same title must resolve to the same destination, not to a second file"
    );
    assert_eq!(
        std::fs::read(&two).expect("read back"),
        b"second body",
        "the destination must hold the newer body"
    );

    let entries: Vec<PathBuf> = std::fs::read_dir(&output_dir)
        .expect("list output dir")
        .map(|entry| entry.expect("dir entry").path())
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "downloading twice must converge on one file, found {entries:?}"
    );
}
