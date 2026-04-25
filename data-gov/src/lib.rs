//! High-level bindings for the U.S. [data.gov](https://data.gov) catalog.
//!
//! The `data-gov` crate bundles an ergonomic async client, CLI-friendly utilities,
//! and configuration helpers on top of the lower-level [`data_gov_catalog`]
//! crate. It is designed for read-only exploration workflows such as search,
//! dataset inspection, and downloading published distributions. The main entry
//! point is [`DataGovClient`], which requires a Tokio runtime.

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
