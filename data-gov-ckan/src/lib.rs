//! Async CKAN client for open-data portals.
//!
//! This crate exposes typed bindings for the read-only portions of the CKAN
//! Action API. [`CkanClient`] accepts a shared [`Configuration`] and exposes
//! async methods for search, dataset metadata, organizations, and autocomplete
//! endpoints. Data models are re-exported under [`models`].
//!
//! # Status
//!
//! **data.gov no longer exposes a CKAN API.** As of 2026, the data.gov catalog
//! is served by a purpose-built search API (see the `data-gov-catalog` crate).
//! This crate remains published because CKAN is still the backbone of many
//! other open-data portals (European, state, municipal, and university
//! instances), and the client works unchanged against any compliant CKAN
//! deployment. Point [`Configuration::base_path`] at your target instance.

// Every public item in this crate carries a doc comment, and that is
// enforced rather than remembered (#118).
#![deny(missing_docs)]
#![allow(clippy::too_many_arguments)]

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
/// The HTTP client, its configuration, and its error type.
pub mod client;
/// Types modelling the CKAN Action API's JSON payloads.
pub mod models;

// Re-export the ergonomic client and configuration for easy access
pub use client::{ApiKey, BasicAuth, CkanClient, CkanError, Configuration};
