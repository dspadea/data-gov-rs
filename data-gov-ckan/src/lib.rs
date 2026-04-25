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

#![allow(clippy::too_many_arguments)]

pub mod client;
pub mod models;

// Re-export the ergonomic client and configuration for easy access
pub use client::{ApiKey, BasicAuth, CkanClient, CkanError, Configuration};
