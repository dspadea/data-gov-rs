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

pub mod client;
pub mod models;

pub use client::{CatalogClient, CatalogError, Configuration, SearchParams};
