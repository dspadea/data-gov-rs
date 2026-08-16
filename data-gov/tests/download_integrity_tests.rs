//! What a download leaves on disk when it does not finish (#49, #50).
//!
//! These tests need a server that lies, stalls, and hangs up mid-body, which
//! `wiremock` will not do, so they drive a raw socket. `scripted_origin`
//! declares a `Content-Length`, writes the pieces it was given with a gap
//! between them, and then closes - whether or not it has sent what it promised.

use std::sync::Arc;
use std::time::{Duration, Instant};

use data_gov::catalog::models::Distribution;
use data_gov::{DataGovClient, DataGovConfig, DataGovError, OperatingMode};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serve `pieces`, `gap` apart, under a declared length of `declared`.
///
/// When the pieces add up to less than `declared`, the connection closes with
/// the body unfinished, which is what a dropped transfer looks like on the
/// wire. Returns the origin's base URL.
async fn scripted_origin(declared: usize, pieces: Vec<Vec<u8>>, gap: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("local addr").port();
    let pieces = Arc::new(pieces);

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let pieces = Arc::clone(&pieces);
            tokio::spawn(async move {
                // Drain the request head. Its content does not matter here.
                let mut head = Vec::new();
                let mut buffer = [0u8; 512];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut buffer).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buffer[..n]),
                    }
                }

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {declared}\r\n\
                     Content-Type: application/octet-stream\r\n\r\n"
                );
                if socket.write_all(response.as_bytes()).await.is_err() {
                    return;
                }
                for piece in pieces.iter() {
                    if socket.write_all(piece).await.is_err() || socket.flush().await.is_err() {
                        return;
                    }
                    tokio::time::sleep(gap).await;
                }
            });
        }
    });

    format!("http://127.0.0.1:{port}")
}

fn client_for(download_dir: &std::path::Path, timeout_secs: u64) -> DataGovClient {
    let config = DataGovConfig::default()
        .with_mode(OperatingMode::Interactive)
        .with_download_dir(download_dir)
        .with_download_timeout(timeout_secs)
        // The scripted origin listens on loopback, which downloads refuse by
        // default (#51).
        .with_private_network_downloads(true);
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

/// Every entry currently in `dir`, sorted, as file names.
fn entries(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("list directory")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into()
        })
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn a_transfer_cut_off_mid_body_leaves_nothing_at_the_destination() {
    let tmp = TempDir::new().expect("tempdir");
    let origin = scripted_origin(
        4096,
        vec![b"the first 40 bytes and then a hang up".to_vec()],
        Duration::ZERO,
    )
    .await;
    let client = client_for(tmp.path(), 10);
    let dist = distribution(&format!("{origin}/report"), "report", "csv");

    client
        .download_distribution(&dist, Some(tmp.path()))
        .await
        .expect_err("a body that stops early is not a completed download");

    assert!(
        !tmp.path().join("report.csv").exists(),
        "a partial transfer must not be left under the name of a complete file"
    );
}

#[tokio::test]
async fn a_transfer_cut_off_mid_body_leaves_no_temporary_file_behind() {
    let tmp = TempDir::new().expect("tempdir");
    let origin = scripted_origin(4096, vec![b"a partial body".to_vec()], Duration::ZERO).await;
    let client = client_for(tmp.path(), 10);
    let dist = distribution(&format!("{origin}/report"), "report", "csv");

    client
        .download_distribution(&dist, Some(tmp.path()))
        .await
        .expect_err("a body that stops early is not a completed download");

    assert_eq!(
        entries(tmp.path()),
        Vec::<String>::new(),
        "a failed transfer must clean up after itself"
    );
}

#[tokio::test]
async fn a_failed_transfer_does_not_disturb_the_complete_file_already_there() {
    let tmp = TempDir::new().expect("tempdir");
    let destination = tmp.path().join("report.csv");
    let original = b"the complete file from an earlier run";
    tokio::fs::write(&destination, original)
        .await
        .expect("seed the destination");

    let origin = scripted_origin(
        4096,
        vec![b"replacement that never arrives".to_vec()],
        Duration::ZERO,
    )
    .await;
    let client = client_for(tmp.path(), 10);
    let dist = distribution(&format!("{origin}/report"), "report", "csv");

    client
        .download_distribution(&dist, Some(tmp.path()))
        .await
        .expect_err("a body that stops early is not a completed download");

    assert_eq!(
        tokio::fs::read(&destination).await.expect("read back"),
        original,
        "the earlier complete file must survive a failed re-download"
    );
    assert_eq!(
        entries(tmp.path()),
        vec!["report.csv".to_string()],
        "nothing else may be left in the directory"
    );
}

/// A body that stops short of its `Content-Length` is a failed download.
///
/// The refusal comes from HTTP/1.1 framing: hyper rejects the truncated body as
/// an incomplete message, so the client's own declared-length comparison is
/// never reached over this transport. That comparison is a backstop and is
/// covered by unit tests on `short_transfer`; asserting the variant here keeps
/// this test honest about which layer actually decides.
#[tokio::test]
async fn a_body_shorter_than_its_declared_length_is_reported_as_a_transport_failure() {
    let tmp = TempDir::new().expect("tempdir");
    // Declares 4096 bytes and sends 5.
    let origin = scripted_origin(4096, vec![b"short".to_vec()], Duration::ZERO).await;
    let client = client_for(tmp.path(), 10);
    let dist = distribution(&format!("{origin}/report"), "report", "csv");

    let outcome = client.download_distribution(&dist, Some(tmp.path())).await;

    match outcome {
        Err(DataGovError::HttpError(err)) => {
            assert!(
                err.is_decode() || err.is_body(),
                "the failure must come from reading the body, got: {err:?}"
            );
        }
        Err(other) => panic!("expected a transport failure reading the body, got {other:?}"),
        Ok(path) => panic!(
            "a body shorter than its Content-Length was reported as a completed download at {path:?}"
        ),
    }

    assert_eq!(
        entries(tmp.path()),
        Vec::<String>::new(),
        "neither the destination nor a temporary file may be left behind"
    );
}

#[tokio::test]
async fn a_whole_body_still_lands_and_the_temporary_file_is_gone() {
    let tmp = TempDir::new().expect("tempdir");
    let body = b"a complete body".to_vec();
    let origin = scripted_origin(body.len(), vec![body.clone()], Duration::ZERO).await;
    let client = client_for(tmp.path(), 10);
    let dist = distribution(&format!("{origin}/report"), "report", "csv");

    let written = client
        .download_distribution(&dist, Some(tmp.path()))
        .await
        .expect("a complete body must land");

    assert_eq!(
        tokio::fs::read(&written).await.expect("read back"),
        body,
        "the destination must hold the whole body"
    );
    assert_eq!(
        entries(tmp.path()),
        vec!["report.csv".to_string()],
        "the temporary file must not survive a successful transfer"
    );
}

/// #50: `ClientBuilder::timeout` runs from connect until the body finishes, so
/// any transfer longer than the configured value fails however healthy the
/// connection is. A stall timeout is what the setting is documented to be.
#[tokio::test]
async fn a_slow_but_steady_transfer_outlasts_the_configured_timeout() {
    let tmp = TempDir::new().expect("tempdir");
    let pieces: Vec<Vec<u8>> = (0..15)
        .map(|i| format!("chunk-{i};").into_bytes())
        .collect();
    let declared: usize = pieces.iter().map(Vec::len).sum();
    // Fifteen pieces 200ms apart is 3s of transfer against a 2s timeout, and no
    // gap is longer than a tenth of it. The earlier shape left 600ms of slack
    // between the gap and the timeout, which a loaded runner can eat.
    let timeout = Duration::from_secs(2);
    let origin = scripted_origin(declared, pieces, Duration::from_millis(200)).await;
    let client = client_for(tmp.path(), timeout.as_secs());
    let dist = distribution(&format!("{origin}/report"), "report", "csv");

    let started = Instant::now();
    let written = client
        .download_distribution(&dist, Some(tmp.path()))
        .await
        .expect("a steady transfer must not be cut off for taking longer than the timeout");
    let elapsed = started.elapsed();

    assert!(
        elapsed > timeout,
        "the transfer has to outlast the timeout for this to prove anything, took {elapsed:?}"
    );
    assert_eq!(
        tokio::fs::read(&written).await.expect("read back").len(),
        declared,
        "the whole body must arrive"
    );
}

/// The other half of #50: a connection that stops sending must still be cut
/// off, or replacing the total deadline would just remove the protection.
#[tokio::test]
async fn a_stalled_transfer_is_still_cut_off() {
    let tmp = TempDir::new().expect("tempdir");
    // One piece, then a 30s gap before the connection would close. The
    // declared length is never reached.
    let origin = scripted_origin(
        4096,
        vec![b"the start".to_vec(), b"never arrives".to_vec()],
        Duration::from_secs(30),
    )
    .await;
    let client = client_for(tmp.path(), 1);
    let dist = distribution(&format!("{origin}/report"), "report", "csv");

    let started = Instant::now();
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        client.download_distribution(&dist, Some(tmp.path())),
    )
    .await
    .expect("a stalled transfer must be cut off, not waited on");
    let elapsed = started.elapsed();

    outcome.expect_err("a stalled transfer must fail");
    assert!(
        elapsed < Duration::from_secs(15),
        "the stall must be cut off near the configured timeout, took {elapsed:?}"
    );
}

/// Watch `dir` until an entry appears in it, or `deadline` passes.
///
/// The dropped-transfer tests need this. Without it a run where the transfer
/// never started would find an empty directory and pass while proving
/// nothing, which is the one way those tests could go green for the wrong
/// reason.
async fn wait_for_first_entry(dir: std::path::PathBuf, deadline: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if !entries(&dir).is_empty() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    false
}

/// An origin that sends a little and then stalls, so the transfer is still
/// running when the caller drops it. Returns the origin's base URL.
async fn stalling_origin() -> String {
    scripted_origin(
        4096,
        vec![
            b"the first bytes and then a long wait".to_vec(),
            b"never arrives".to_vec(),
        ],
        Duration::from_secs(30),
    )
    .await
}

/// A caller that cancels a download - an MCP request that times out, or a
/// `notifications/cancelled` - drops the transfer future rather than returning
/// through any of its error paths, so nothing in the function body runs.
#[tokio::test]
async fn a_dropped_transfer_leaves_no_temporary_file_behind() {
    let tmp = TempDir::new().expect("tempdir");
    let origin = stalling_origin().await;
    // A ten second stall timeout, so the client's own timeout cannot fire
    // first and take the error path this test is not about.
    let client = client_for(tmp.path(), 10);
    let dist = distribution(&format!("{origin}/report"), "report", "csv");

    let watcher = tokio::spawn(wait_for_first_entry(
        tmp.path().to_path_buf(),
        Duration::from_secs(5),
    ));

    // The block bounds the timeout's own lifetime, so the transfer future is
    // dropped before the assertions read the directory.
    {
        let download = client.download_distribution(&dist, Some(tmp.path()));
        tokio::time::timeout(Duration::from_secs(1), download)
            .await
            .expect_err("the stalled transfer must still be running when it is dropped");
    }

    assert!(
        watcher.await.expect("watcher task"),
        "the transfer never reached the point of creating a file, so this run proves nothing"
    );
    assert_eq!(
        entries(tmp.path()),
        Vec::<String>::new(),
        "a dropped transfer must not leave its temporary file behind"
    );
}

/// Cancel, then retry: the second attempt must land, and the first must not
/// still be sitting in the directory beside it.
#[tokio::test]
async fn a_download_retried_after_a_cancelled_one_leaves_only_the_destination() {
    let tmp = TempDir::new().expect("tempdir");
    let stalled = stalling_origin().await;
    let client = client_for(tmp.path(), 10);
    let cancelled = distribution(&format!("{stalled}/report"), "report", "csv");

    let watcher = tokio::spawn(wait_for_first_entry(
        tmp.path().to_path_buf(),
        Duration::from_secs(5),
    ));
    {
        let download = client.download_distribution(&cancelled, Some(tmp.path()));
        tokio::time::timeout(Duration::from_secs(1), download)
            .await
            .expect_err("the stalled transfer must still be running when it is dropped");
    }
    assert!(
        watcher.await.expect("watcher task"),
        "the cancelled transfer never created a file, so this run proves nothing"
    );

    let body = b"a complete body".to_vec();
    let whole = scripted_origin(body.len(), vec![body.clone()], Duration::ZERO).await;
    let dist = distribution(&format!("{whole}/report"), "report", "csv");
    let written = client
        .download_distribution(&dist, Some(tmp.path()))
        .await
        .expect("the retry must land");

    assert_eq!(
        tokio::fs::read(&written).await.expect("read back"),
        body,
        "the retry must hold the whole body"
    );
    assert_eq!(
        entries(tmp.path()),
        vec!["report.csv".to_string()],
        "the cancelled transfer's temporary file must not outlive it"
    );
}
