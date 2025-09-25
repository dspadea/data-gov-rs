//! Async CKAN client optimized for the [data.gov](https://data.gov) portal.
//!
//! This crate exposes typed bindings for the read-only portions of the CKAN
//! API that power data.gov and similar open-data portals. Most consumers should
//! start with [`CkanClient`], which accepts a shared [`Configuration`] and
//! exposes async methods for search, dataset metadata, organizations, and
//! autocomplete endpoints. The crate re-exports the generated data models under
//! [`models`].

#![allow(unused_imports)]
#![allow(clippy::too_many_arguments)]

extern crate serde;
extern crate serde_json;
extern crate serde_repr;
extern crate url;

pub mod client;
pub mod models;

// Re-export the ergonomic client and configuration for easy access
pub use client::{ApiKey, BasicAuth, CkanClient, CkanError, Configuration};
