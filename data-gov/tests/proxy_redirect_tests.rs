//! A redirect to a host *name* is judged before it is followed (#51).
//!
//! Three layers guard a download destination: the check before the request
//! leaves, the redirect handling, and the DNS resolver. A redirect target that
//! is a name can only be judged after it is resolved, and the resolver is the
//! layer that would do it - but reqwest resolves the *proxy's* host, never the
//! destination's, when a proxy is configured. So with `HTTP_PROXY` set, a
//! redirect to a name reached the destination with nothing having judged it.
//!
//! Reproducing that needs `HTTP_PROXY` in the environment before the client is
//! built, and mutating the environment of a running process is neither safe in
//! Rust 2024 nor sound with tests in parallel. So the test re-runs itself in a
//! child process with the variable set, and the child does the real work.
//! Nothing leaves the machine: the only address anything connects to is the
//! fake proxy on loopback.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use data_gov::catalog::models::Distribution;
use data_gov::{DataGovClient, DataGovConfig, DataGovError, OperatingMode};
use tempfile::TempDir;

/// Set in the child process, so one test function can play both roles.
const CHILD_MARKER: &str = "DATA_GOV_PROXY_REDIRECT_CHILD";

/// The name the proxy redirects to. It resolves to loopback, which downloads
/// refuse, and the client never resolves it because the proxy stands in the
/// way - which is the whole point.
const REDIRECT_TARGET: &str = "http://localhost/latest/meta-data/iam/security-credentials/";

/// The first hop. A routable literal, so the pre-request check passes it
/// without a lookup and the proxy is what actually answers.
const FIRST_HOP: &str = "http://93.184.216.34/data.csv";

/// What the proxy serves once the redirect has been followed. Standing in for
/// the instance-metadata response an SSRF is after.
const SECRET_BODY: &[u8] = b"INSTANCE METADATA CREDENTIALS";

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

/// Answer one proxied request and close.
///
/// The first request gets a redirect to a name; every later one gets the
/// secret. A client that judges the name never asks for the second.
fn serve_one(mut socket: TcpStream, served: &AtomicUsize) {
    let mut head = Vec::new();
    let mut buffer = [0u8; 512];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match socket.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&buffer[..n]),
        }
    }

    let response = if served.fetch_add(1, Ordering::SeqCst) == 0 {
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {REDIRECT_TARGET}\r\n\
             Content-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes()
    } else {
        let mut bytes = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\
             Content-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
            SECRET_BODY.len()
        )
        .into_bytes();
        bytes.extend_from_slice(SECRET_BODY);
        bytes
    };

    let _ = socket.write_all(&response);
    let _ = socket.flush();
}

/// The parent role: run a fake proxy, then re-run this test with it configured.
fn drive_the_child() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the fake proxy");
    let proxy = format!("http://{}", listener.local_addr().expect("proxy address"));
    let served = Arc::new(AtomicUsize::new(0));

    let serving = Arc::clone(&served);
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            match socket {
                Ok(socket) => serve_one(socket, &serving),
                Err(_) => return,
            }
        }
    });

    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let output = Command::new(exe)
        .args([
            "--exact",
            "a_redirect_to_a_name_is_judged_when_a_proxy_stands_in_the_way",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_MARKER, "1")
        .env("HTTP_PROXY", &proxy)
        .env("http_proxy", &proxy)
        // A proxy exclusion or a socks proxy inherited from the developer's
        // shell would decide the outcome instead of the code under test.
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy")
        .output()
        .expect("the child test process must start");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        served.load(Ordering::SeqCst) > 0,
        "the child never reached the proxy, so it proved nothing.\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        output.status.success(),
        "the redirect to a name was not judged before it was followed.\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

/// The child role: the escape itself.
fn reproduce_the_escape() {
    let runtime = tokio::runtime::Runtime::new().expect("child runtime");
    runtime.block_on(async {
        let tmp = TempDir::new().expect("tempdir");
        // The default: private-network downloads are refused, so a destination
        // resolving to loopback may not be reached.
        let config = DataGovConfig::default()
            .with_mode(OperatingMode::Interactive)
            .with_download_dir(tmp.path())
            .with_download_timeout(5);
        let client = DataGovClient::with_config(config).expect("child client must build");

        let dist = distribution(FIRST_HOP, "credentials", "json");
        let outcome = client.download_distribution(&dist, Some(tmp.path())).await;

        match outcome {
            Err(DataGovError::ValidationError { message }) => {
                assert!(
                    message.contains("localhost"),
                    "the refusal must name the host it judged, got: {message}"
                );
            }
            Err(other) => {
                panic!("expected the hop to be refused by the destination check, got {other:?}")
            }
            Ok(path) => {
                let body = std::fs::read(&path).unwrap_or_default();
                panic!(
                    "the redirect to a name was followed: {path:?} holds {:?}",
                    String::from_utf8_lossy(&body)
                );
            }
        }

        let left_behind: Vec<_> = std::fs::read_dir(tmp.path())
            .expect("list the download directory")
            .map(|entry| entry.expect("dir entry").file_name())
            .collect();
        assert!(
            left_behind.is_empty(),
            "a refused download must leave nothing behind, found {left_behind:?}"
        );
    });
}

/// A 302 to a name is followed only after the name has been judged.
///
/// One function, two roles. Without the marker it is the parent: it stands up
/// the proxy and re-runs itself. With the marker it is the child, and the child
/// is where the escape either happens or does not.
#[test]
fn a_redirect_to_a_name_is_judged_when_a_proxy_stands_in_the_way() {
    if std::env::var_os(CHILD_MARKER).is_some() {
        reproduce_the_escape();
        return;
    }
    drive_the_child();
}
