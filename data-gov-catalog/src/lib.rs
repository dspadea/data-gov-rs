//! Async client for the [data.gov](https://data.gov) Catalog API.
//!
//! The Catalog API replaced data.gov's CKAN action API in 2026. It exposes
//! full-text search over the federal dataset catalog together with organization
//! and keyword listings, spatial lookups, and direct access to individual
//! harvest records. Metadata is returned in the
//! [DCAT-US 3](https://resources.data.gov/resources/dcat-us/) vocabulary.
//!
//! Start with [`CatalogClient`]: construct a [`Configuration`] (the default
//! points at `https://catalog.data.gov`), wrap it in an `Arc`, and call one
//! of the async methods such as [`CatalogClient::search`] or
//! [`CatalogClient::organizations`].
//!
//! ```no_run
//! use data_gov_catalog::{CatalogClient, Configuration, SearchParams};
//! use std::sync::Arc;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = CatalogClient::new(Arc::new(Configuration::default()));
//! let page = client
//!     .search(SearchParams::new().q("climate").per_page(5))
//!     .await?;
//! println!("{} results on this page", page.results.len());
//! # Ok(()) }
//! ```

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

pub mod client;
pub mod models;

pub use client::{
    CatalogClient, CatalogError, Configuration, DEFAULT_CONNECT_TIMEOUT, DEFAULT_TIMEOUT,
    SearchParams, SortOrder, SpatialFilter,
};
