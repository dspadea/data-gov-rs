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
pub mod colors;
pub mod config;
pub mod error;

// Re-export main types for convenience
pub use client::DataGovClient;
pub use colors::{ColorHelper, ColorMode};
pub use config::{DataGovConfig, OperatingMode};
pub use error::{DataGovError, Result};

// pub trait CKANResponse: serde::de::DeserializeOwned {}

// // Extension trait for automatic conversion
// pub trait IntoCKANResponse {
//     fn into_ckan<T>(self) -> T
//     where
//         T: CKANResponse;
// }

// impl IntoCKANResponse for serde_json::Value {
//     fn into_ckan<T>(self) -> T
//     where
//         T: CKANResponse,
//     {
//         serde_json::from_value::<T>(self)
//             .expect("Failed to convert Value to target struct")
//     }
// }

// #[derive(serde::Deserialize, Debug)]
// pub struct PackageSearchResult {
//     pub help: String,
//     pub success: bool,
//     pub result: PackageSearchResultDetail,
// }

// impl CKANResponse for PackageSearchResult {}

// #[derive(serde::Deserialize, Debug)]
// pub struct PackageSearchResultDetail {
//     pub count: u32,
//     pub sort: Option<String>,
//     pub results: Vec<PackageSearchResultItem>,
//     // pub facets: Option<serde_json::Value>,
//     // pub search_facets: Option<serde_json::Value>,
// }

// #[derive(serde::Deserialize, Debug)]
// pub struct PackageSearchResultItem {
//     pub display_name: Option<String>,
//     pub id: String,
//     pub name: String,
//     pub state: String,
//     pub vocabulary_id: Option<String>,
// }

// impl PackageSearchResultItem {

//     // Metadata contains resource URLs and more
//     pub fn to_metadata_url(&self) -> String {
//         format!("https://catalog.data.gov/harvest/object/{}", self.id)
//     }
// }
