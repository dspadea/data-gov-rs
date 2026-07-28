// A TLS backend is not optional: every endpoint this crate talks to is HTTPS.
// Without one, reqwest builds an HTTP-only connector, the crate compiles clean,
// and the first request fails at connect with "invalid URL, scheme is not http".
// Fail at build time instead, where the cause is legible.
#[cfg(not(any(feature = "native-tls", feature = "rustls-tls")))]
compile_error!(
    "no TLS backend selected: enable the `native-tls` feature (the default) or `rustls-tls`. \
     Building with `default-features = false` and neither feature produces a client that \
     cannot complete any HTTPS request."
);

mod handlers;
mod server;
mod tools;
mod types;

#[cfg(test)]
mod dispatch_tests;

use server::DataGovMcpServer;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();

    DataGovMcpServer::bootstrap().await?;

    Ok(())
}
