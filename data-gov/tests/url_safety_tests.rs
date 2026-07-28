//! Destination checks for catalog-supplied download URLs (#51).
//!
//! `distribution.downloadURL` arrives in harvested third-party DCAT metadata,
//! so it is hostile input. These tests state what the client may fetch:
//!
//! - `http` and `https` only, checked before the request leaves
//! - no loopback, link-local, RFC 1918, unique-local, or carrier-grade-NAT
//!   destination, whether the URL names an address or a host that resolves to
//!   one
//! - the same rules on every redirect hop, not only on the first URL
//! - an opt-in for the operator who really is pointing at a local mirror

use std::time::Duration;

use data_gov::catalog::models::Distribution;
use data_gov::{DataGovClient, DataGovConfig, DataGovError, OperatingMode};
use tempfile::TempDir;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The instance-metadata endpoint an SSRF against a cloud VM aims at.
const METADATA_URL: &str = "http://169.254.169.254/latest/meta-data/iam/security-credentials/";

fn client_for(download_dir: &std::path::Path, allow_private: bool) -> DataGovClient {
    let config = DataGovConfig::default()
        .with_mode(OperatingMode::Interactive)
        .with_download_dir(download_dir)
        .with_download_timeout(5)
        .with_private_network_downloads(allow_private);
    DataGovClient::with_config(config).expect("test client must build")
}

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

/// Run one download and require it to refuse rather than to hang or to fetch.
async fn refusal_for(client: &DataGovClient, dist: &Distribution, dir: &std::path::Path) -> String {
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        client.download_distribution(dist, Some(dir)),
    )
    .await
    .expect("the refusal must arrive without waiting on the network");

    match outcome {
        Err(DataGovError::ValidationError { message }) => message,
        Err(other) => panic!("expected a ValidationError refusal, got {other:?}"),
        Ok(path) => panic!("expected a refusal, but the download landed at {path:?}"),
    }
}

#[tokio::test]
async fn a_link_local_download_url_is_refused() {
    let tmp = TempDir::new().expect("tempdir");
    let client = client_for(tmp.path(), false);
    let dist = distribution(METADATA_URL, "credentials", "json");

    let message = refusal_for(&client, &dist, tmp.path()).await;

    assert!(
        message.contains("169.254.169.254"),
        "the refusal must name the destination it rejected, got: {message}"
    );
}

#[tokio::test]
async fn a_link_local_download_url_is_refused_even_with_the_opt_in() {
    let tmp = TempDir::new().expect("tempdir");
    let client = client_for(tmp.path(), true);
    let dist = distribution(METADATA_URL, "credentials", "json");

    let message = refusal_for(&client, &dist, tmp.path()).await;

    assert!(
        message.contains("169.254.169.254"),
        "the private-network opt-in must not open the metadata range, got: {message}"
    );
}

#[tokio::test]
async fn a_non_http_download_url_is_refused() {
    let tmp = TempDir::new().expect("tempdir");
    let client = client_for(tmp.path(), false);
    let dist = distribution("ftp://example.com/data.csv", "data", "csv");

    let message = refusal_for(&client, &dist, tmp.path()).await;

    assert!(
        message.contains("ftp"),
        "the refusal must name the scheme it rejected, got: {message}"
    );
}

#[tokio::test]
async fn a_loopback_download_url_is_refused_and_never_requested() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data.csv"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"secret".to_vec()))
        // The check has to happen before the request leaves, so the mock must
        // see nothing at all. wiremock verifies this expectation on drop.
        .expect(0)
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = client_for(tmp.path(), false);
    let dist = distribution(&format!("{}/data.csv", server.uri()), "data", "csv");

    let message = refusal_for(&client, &dist, tmp.path()).await;

    assert!(
        message.contains("127.0.0.1"),
        "the refusal must name the destination it rejected, got: {message}"
    );
}

#[tokio::test]
async fn a_host_name_that_resolves_to_loopback_is_refused() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data.csv"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"secret".to_vec()))
        .expect(0)
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = client_for(tmp.path(), false);
    // A name, not an address: matching on the literal text of the URL is not
    // enough, the host has to be resolved before it is judged.
    let url = format!("http://localhost:{}/data.csv", server.address().port());
    let dist = distribution(&url, "data", "csv");

    let message = refusal_for(&client, &dist, tmp.path()).await;

    assert!(
        message.contains("localhost"),
        "the refusal must name the host it rejected, got: {message}"
    );
}

#[tokio::test]
async fn the_opt_in_allows_a_loopback_download() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data.csv"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"local mirror".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = client_for(tmp.path(), true);
    let dist = distribution(&format!("{}/data.csv", server.uri()), "data", "csv");

    let written = client
        .download_distribution(&dist, Some(tmp.path()))
        .await
        .expect("the opt-in must restore downloads from a local mirror");

    let body = tokio::fs::read(&written).await.expect("read back");
    assert_eq!(
        body, b"local mirror",
        "the opt-in must deliver the mirror's bytes"
    );
}

#[tokio::test]
async fn a_redirect_to_a_link_local_address_is_refused() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data.csv"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", METADATA_URL))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    // The opt-in is what lets the first hop reach the mock on loopback. It must
    // not carry over to the metadata address the mock redirects to.
    let client = client_for(tmp.path(), true);
    let dist = distribution(&format!("{}/data.csv", server.uri()), "data", "csv");

    let message = refusal_for(&client, &dist, tmp.path()).await;

    assert!(
        message.contains("169.254.169.254"),
        "the redirect must be refused by name, not merely fail to connect, got: {message}"
    );
}

/// A host that cannot be resolved anywhere, by construction.
///
/// The first label is 70 octets. A DNS label carries its length in six bits, so
/// anything past 63 cannot be encoded in a query at all and the resolver
/// refuses it locally without asking the network. That matters here: a name
/// merely reserved as unresolvable (RFC 6761's `.invalid`) still depends on the
/// resolver honouring the reservation, and a resolver that answers every name
/// would turn this test into a live outbound connection.
const UNRESOLVABLE_HOST: &str =
    "this-label-is-too-long-to-be-encoded-in-a-dns-query-and-cannot-resolve.example";

/// A redirect target that is a *name* has to be resolved before it can be
/// judged, which a synchronous redirect callback cannot do. This states that
/// the hop goes through the same check the first URL does - the check that
/// resolves - rather than through whatever the connect path happens to catch.
#[tokio::test]
async fn a_redirect_to_a_name_is_judged_by_the_destination_check() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data.csv"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("http://{UNRESOLVABLE_HOST}/secret")),
        )
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    // The opt-in is what lets the first hop reach the mock on loopback, so the
    // verdict under test can only come from judging the second.
    let client = client_for(tmp.path(), true);
    let dist = distribution(&format!("{}/data.csv", server.uri()), "data", "csv");

    let message = refusal_for(&client, &dist, tmp.path()).await;

    assert!(
        message.contains(UNRESOLVABLE_HOST),
        "the refusal must name the host the hop pointed at, got: {message}"
    );
}

/// Every status reqwest treats as a redirect is followed here, and every one of
/// them is checked. A hop cap or a status the handling forgot about would leave
/// one of these unjudged.
#[tokio::test]
async fn every_redirect_status_is_checked_before_it_is_followed() {
    for status in [301, 302, 303, 307, 308] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/data.csv"))
            .respond_with(ResponseTemplate::new(status).insert_header("location", METADATA_URL))
            .mount(&server)
            .await;

        let tmp = TempDir::new().expect("tempdir");
        let client = client_for(tmp.path(), true);
        let dist = distribution(&format!("{}/data.csv", server.uri()), "data", "csv");

        let message = refusal_for(&client, &dist, tmp.path()).await;

        assert!(
            message.contains("169.254.169.254"),
            "HTTP {status} must be followed only after the target is judged, got: {message}"
        );
    }
}

/// A `Location` is a reference, not necessarily an absolute URL. Resolving it
/// against the current URL is what decides which host is actually judged.
#[tokio::test]
async fn a_relative_redirect_resolves_against_the_url_it_came_from() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/first/data.csv"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "../second/data.csv"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/second/data.csv"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"second hop".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = client_for(tmp.path(), true);
    let dist = distribution(&format!("{}/first/data.csv", server.uri()), "data", "csv");

    let written = client
        .download_distribution(&dist, Some(tmp.path()))
        .await
        .expect("a relative redirect must resolve and be followed");

    assert_eq!(
        tokio::fs::read(&written).await.expect("read back"),
        b"second hop",
        "the body must come from the hop the relative Location named"
    );
}

/// A redirect chain that ends in a real body still delivers it. Without this,
/// refusing every redirect would pass every other test in this file.
#[tokio::test]
async fn a_redirect_to_a_permitted_destination_is_followed_and_delivers_the_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/start"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/middle"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/middle"))
        .respond_with(ResponseTemplate::new(307).insert_header("location", "/end"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/end"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"the real body".to_vec()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = client_for(tmp.path(), true);
    let dist = distribution(&format!("{}/start", server.uri()), "data", "csv");

    let written = client
        .download_distribution(&dist, Some(tmp.path()))
        .await
        .expect("a permitted redirect chain must still deliver its body");

    assert_eq!(
        tokio::fs::read(&written).await.expect("read back"),
        b"the real body",
        "the body must come from the end of the chain"
    );
}

/// A 3xx with no `Location` is not a redirect anybody can follow. It has to be
/// reported as the failed download it is, not silently treated as a body.
#[tokio::test]
async fn a_redirect_without_a_location_is_reported_as_a_failure() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data.csv"))
        .respond_with(ResponseTemplate::new(302))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = client_for(tmp.path(), true);
    let dist = distribution(&format!("{}/data.csv", server.uri()), "data", "csv");

    let error = client
        .download_distribution(&dist, Some(tmp.path()))
        .await
        .expect_err("a 302 with nowhere to go is not a download");

    assert!(
        matches!(error, DataGovError::DownloadError { .. }),
        "expected a download failure naming the status, got {error:?}"
    );
    assert!(
        error.to_string().contains("302"),
        "the failure must name the status it could not use, got: {error}"
    );
}

#[tokio::test]
async fn an_endless_redirect_chain_is_refused() {
    let server = MockServer::start().await;
    let target = format!("{}/hop", server.uri());
    Mock::given(method("GET"))
        .and(path_regex(r"^/hop$"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", target.as_str()))
        .mount(&server)
        .await;

    let tmp = TempDir::new().expect("tempdir");
    let client = client_for(tmp.path(), true);
    let dist = distribution(&target, "data", "csv");

    let message = refusal_for(&client, &dist, tmp.path()).await;

    // Not merely "redirect": every other refusal this handling can produce
    // carries that word too, so matching it would also pass if the cap were
    // replaced by a resolution failure.
    assert!(
        message.contains("abandoned after"),
        "the refusal must be the hop cap, not some other redirect failure, got: {message}"
    );
    assert!(
        message.contains("10"),
        "the refusal must say how many hops were allowed, got: {message}"
    );
}
