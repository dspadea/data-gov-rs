//! High-level bindings for the U.S. [data.gov](https://data.gov) CKAN portal.
//!
//! The `data-gov` crate bundles an ergonomic async client, CLI-friendly utilities,
//! and configuration helpers on top of the lower-level [`data_gov_ckan`] crate. It
//! is designed for read-only exploration workflows such as search, dataset
//! inspection, and downloading published resources. The main entry point is
//! [`DataGovClient`], which requires a Tokio runtime.

/// Base URL for the public data.gov CKAN API (`/api/3`).
///
/// This constant is provided for convenience when you need to construct direct
/// HTTP calls or configure the lower-level [`data_gov_ckan::Configuration`].
pub const DATA_GOV_BASE_URL: &str = "https://catalog.data.gov/api/3";

// Re-export the CKAN crate for direct access
pub use data_gov_ckan as ckan;

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
