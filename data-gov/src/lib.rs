//! High-level bindings for the U.S. [data.gov](https://data.gov) catalog.
//!
//! The `data-gov` crate bundles an ergonomic async client, CLI-friendly utilities,
//! and configuration helpers on top of the lower-level [`data_gov_catalog`]
//! crate. It is designed for read-only exploration workflows such as search,
//! dataset inspection, and downloading published distributions. The main entry
//! point is [`DataGovClient`], which requires a Tokio runtime.

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

/// Base URL for the public data.gov Catalog API.
///
/// Provided for convenience when constructing a
/// [`data_gov_catalog::Configuration`] directly.
pub const DATA_GOV_BASE_URL: &str = "https://catalog.data.gov";

// Re-export the catalog crate for direct access
pub use data_gov_catalog as catalog;

// Public modules
pub mod client;
pub mod config;
pub mod error;
pub mod ui;
pub mod util;

// Re-export main types for convenience
pub use client::DataGovClient;
pub use config::{DataGovConfig, OperatingMode};
pub use error::{DataGovError, Result};
pub use ui::{
    DownloadBatch, DownloadFailed, DownloadFinished, DownloadProgress, DownloadStarted,
    StatusReporter,
};
